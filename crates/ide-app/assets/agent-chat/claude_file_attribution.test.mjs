import assert from "node:assert/strict";
import test from "node:test";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, writeFile, rename, open } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

import {
  projectDirectMutation,
  completeDirectMutationBaseline,
  EvidenceBudget,
  readMutationState,
  compactionEventForSdkMessage,
  countTextLines,
  commandChangesSince,
  onceAsync,
  directEditCounts,
  fileChangeActivityEvent,
  fileAttributionHooks,
  finishCancelledTurn,
  finishTurnAfterFileReceipt,
  isClaudePlanArtifactPath,
  mergeTurnChange,
  mutationStateForContents,
  mutationStateUnchanged,
  readGitChangedFileSnapshot,
  readWorkspaceChangedFileSnapshot,
  resumeSessionIdForCommand,
  setTurnProjection,
} from "./claude_bridge.mjs";

function git(cwd, ...args) {
  return execFileSync("git", ["-C", cwd, ...args], { encoding: "utf8" });
}

async function repository() {
  const scratch = await mkdtemp(join(tmpdir(), "choro-file-tracking-"));
  const root = join(scratch, "repo");
  await mkdir(root);
  git(root, "init", "-q");
  return { scratch, root };
}

function commit(root) {
  git(root, "add", ".");
  git(root, "-c", "user.name=Test", "-c", "user.email=test@example.com", "commit", "-qm", "base");
}

test("Git observation includes staged, existing, deleted, renamed, and binary files with literal paths", async () => {
  const { scratch, root } = await repository();
  for (const name of ["existing.txt", "staged.txt", "deleted.txt", "old.txt", "unrelated.txt"]) {
    await writeFile(join(root, name), "before\n");
  }
  commit(root);
  await writeFile(join(root, "existing.txt"), "already dirty\n");
  await writeFile(join(root, "unrelated.txt"), "other work\n");
  const before = await readGitChangedFileSnapshot(root);
  await writeFile(join(root, "existing.txt"), "feature edit\n");
  await writeFile(join(root, "staged.txt"), "staged edit\n");
  git(root, "add", "staged.txt");
  await rename(join(root, "deleted.txt"), join(scratch, "deleted-fixture"));
  await rename(join(root, "old.txt"), join(root, "new.txt"));
  await writeFile(join(root, "tab\tand\nnewline.txt"), "one\ntwo\n");
  await writeFile(join(root, "image.bin"), Buffer.from([0, 1, 2]));
  const after = await readGitChangedFileSnapshot(root);
  const changed = [...after.keys()].filter((path) => after.get(path).patch !== before.get(path)?.patch).sort();
  assert.deepEqual(changed, ["deleted.txt", "existing.txt", "image.bin", "new.txt", "old.txt", "staged.txt", "tab\tand\nnewline.txt"]);
  assert.equal(after.get("deleted.txt").deletions, 1);
  assert.equal(after.get("staged.txt").additions, 1);
  assert.equal(after.get("tab\tand\nnewline.txt").additions, 2);
  assert.equal(after.get("image.bin").additions, 0);
});

test("unborn repositories count final content once across staging and later edits", async () => {
  const { root } = await repository();
  await writeFile(join(root, "new.txt"), "one\n");
  git(root, "add", "new.txt");
  await writeFile(join(root, "new.txt"), "two\n");
  const snapshot = await readGitChangedFileSnapshot(root);
  assert.equal(snapshot.get("new.txt").additions, 1);
  assert.equal(snapshot.get("new.txt").deletions, 0);
});

test("workspace observations include files in nested repositories without duplicates", async () => {
  const { root } = await repository();
  const nested = join(root, "api");
  await mkdir(nested);
  git(nested, "init", "-q");
  await writeFile(join(root, "front.txt"), "front\n");
  await writeFile(join(nested, "auth.txt"), "backend\n");
  const snapshot = await readWorkspaceChangedFileSnapshot(root);
  assert.deepEqual([...snapshot.keys()].sort(), ["api/auth.txt", "front.txt"]);
});

test("failed Git reads are errors rather than clean snapshots", async () => {
  const directory = await mkdtemp(join(tmpdir(), "choro-not-a-repo-"));
  await assert.rejects(readGitChangedFileSnapshot(directory));
});

test("tracking reads real source even when a configured text converter fails", async () => {
  const { root } = await repository();
  await writeFile(join(root, ".gitattributes"), "*.txt diff=custom\n");
  await writeFile(join(root, "file.txt"), "base\n");
  commit(root);
  git(root, "config", "diff.custom.textconv", "false");
  await writeFile(join(root, "file.txt"), "changed\n");
  const snapshot = await readGitChangedFileSnapshot(root);
  assert.equal(snapshot.get("file.txt").content, "changed\n");
  assert.equal(snapshot.get("file.txt").additions, 1);
});

test("reverts clear tracked observations while committing preserves the receipt history", async () => {
  const { root } = await repository();
  await writeFile(join(root, "file.txt"), "base\n");
  commit(root);
  await writeFile(join(root, "file.txt"), "changed\n");
  const dirty = await readWorkspaceChangedFileSnapshot(root);
  await writeFile(join(root, "file.txt"), "base\n");
  const reverted = await commandChangesSince(dirty, await readWorkspaceChangedFileSnapshot(root));
  assert.equal(reverted.length, 1);
  assert.equal(reverted[0].path, "file.txt");
  assert.equal(reverted[0].clears_projection, true);
  assert.equal(reverted[0].result_content, "base\n");
  await writeFile(join(root, "file.txt"), "changed\n");
  const beforeCommit = await readWorkspaceChangedFileSnapshot(root);
  commit(root);
  assert.deepEqual(await commandChangesSince(beforeCommit, await readWorkspaceChangedFileSnapshot(root)), []);
});

test("shell projections carry the actual content after an earlier direct edit", async () => {
  const { root } = await repository();
  await writeFile(join(root, "file.txt"), "base\n");
  commit(root);
  await writeFile(join(root, "file.txt"), "direct\n");
  const direct = await readGitChangedFileSnapshot(root);
  await writeFile(join(root, "file.txt"), "direct\nshell\n");
  const changes = await commandChangesSince(direct, await readGitChangedFileSnapshot(root));
  assert.equal(changes[0].baseline_hash, direct.get("file.txt").hash);
  assert.equal(changes[0].result_content, "direct\nshell\n");
  assert.equal(changes[0].additions, 2);
});

test("concurrent completion paths wait for one receipt before reporting idle", async () => {
  const events = [];
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const flush = onceAsync(async () => { events.push("start"); await gate; events.push("files"); });
  const result = finishTurnAfterFileReceipt(flush, () => events.push("result"));
  const shutdown = finishTurnAfterFileReceipt(flush, () => events.push("idle"));
  await Promise.resolve();
  assert.deepEqual(events, ["start"]);
  release();
  await Promise.all([result, shutdown]);
  assert.deepEqual(events, ["start", "files", "result", "idle"]);
});

test("Claude compaction starts on SDK status and ends on status or boundary", () => {
  const status = (value, extra = {}) => ({
    type: "system", subtype: "status", status: value, ...extra,
  });
  assert.deepEqual(compactionEventForSdkMessage(status("compacting")), {
    type: "compaction", active: true,
  });
  for (const message of [
    status(null),
    status("requesting"),
    status(null, { compact_result: "failed", compact_error: "Could not compact" }),
    { type: "system", subtype: "compact_boundary" },
  ]) {
    assert.deepEqual(compactionEventForSdkMessage(message), {
      type: "compaction", active: false,
    });
  }
  for (const message of [
    { type: "system", subtype: "init" },
    { type: "assistant" },
    { type: "stream_event", event: { type: "content_block_delta" } },
    { type: "result", subtype: "success" },
  ]) {
    assert.equal(compactionEventForSdkMessage(message), null);
  }
});

test("plan-to-build boundary resumes the live Claude session", () => {
  assert.equal(
    resumeSessionIdForCommand(null, "session-from-plan-turn"),
    "session-from-plan-turn",
  );
  assert.equal(
    resumeSessionIdForCommand("persisted-session", "live-session"),
    "persisted-session",
  );
});

test("Claude private plan artifacts are not reported as project changes", () => {
  assert.equal(
    isClaudePlanArtifactPath(
      "/Users/developer/.claude/plans/choro-memory-cosmic-pine.md",
    ),
    true,
  );
  assert.equal(isClaudePlanArtifactPath("src/plans/feature.md"), false);
});

test("structured edit counts come from the edit action, not global git state", () => {
  assert.deepEqual(
    directEditCounts(
      "Edit",
      { old_string: "old\nlines", new_string: "new\nlines\nhere" },
      { lines: 200 },
      { lines: 500 },
    ),
    [3, 2],
  );
  assert.equal(countTextLines("one\ntwo\n"), 2);
});

test("multiple direct tool actions merge only by their reported path", () => {
  const changes = new Map();
  mergeTurnChange(changes, {
    path: "src/a.rs",
    additions: 2,
    deletions: 1,
    baseline_hash: "base",
    result_hash: "middle",
  });
  mergeTurnChange(changes, {
    path: "src/a.rs",
    additions: 1,
    deletions: 0,
    baseline_hash: "middle",
    result_hash: "final",
  });
  mergeTurnChange(changes, {
    path: "src/b.rs",
    additions: 99,
    deletions: 0,
    baseline_hash: "other",
    result_hash: "other-final",
  });

  assert.deepEqual(changes.get("src/a.rs"), {
    path: "src/a.rs",
    additions: 3,
    deletions: 1,
    baseline_hash: "base",
    result_hash: "final",
    baseline_content: null,
    result_content: null,
  });
  assert.equal(changes.size, 2);
});

test("command worktree projections replace earlier totals", () => {
  const changes = new Map();
  setTurnProjection(changes, {
    path: "generated.css",
    additions: 1,
    deletions: 0,
  });
  setTurnProjection(changes, {
    path: "generated.css",
    additions: 2,
    deletions: 0,
  });

  assert.deepEqual(changes.get("generated.css"), {
    path: "generated.css",
    additions: 2,
    deletions: 0,
  });
});

test("completed tool mutations produce a stable live file row event", () => {
  const change = {
    path: "index.html",
    additions: 23,
    deletions: 34,
  };
  const event = fileChangeActivityEvent("tool-7", change, false);

  assert.equal(event.type, "file_change_activity");
  assert.equal(event.id, "claude:tool-7:index.html");
  assert.equal(event.observed, false);
  assert.deepEqual(event.file, change);
  assert.equal(typeof event.turn_id, "string");
});

test("terminal success, failure, and cancellation can only finish after the receipt flush", async () => {
  for (const outcome of ["success", "failure", "cancellation"]) {
    const events = [];
    await finishTurnAfterFileReceipt(
      async () => events.push("files"),
      () => events.push(outcome),
    );
    assert.deepEqual(events, ["files", outcome]);
  }
});

test("cancellation captures in-flight mutations before emitting the receipt", async () => {
  const events = [];
  await finishCancelledTurn(
    async () => events.push("pending mutation"),
    async () => events.push("files"),
    () => events.push("cancellation"),
  );
  assert.deepEqual(events, ["pending mutation", "files", "cancellation"]);
});

test("failed tools complete the same file-attribution baseline as successful tools", () => {
  const hooks = fileAttributionHooks();
  assert.equal(hooks.PostToolUse.length, 1);
  assert.equal(hooks.PostToolUseFailure.length, 1);
  assert.equal(
    hooks.PostToolUseFailure[0].hooks[0],
    hooks.PostToolUse[0].hooks[0],
  );
});

test("unchanged failed edits are ignored while large and binary files stay fingerprinted", () => {
  assert.equal(
    mutationStateUnchanged(
      { hash: "same", content: null },
      { hash: "same", content: null },
    ),
    true,
  );
  assert.equal(
    mutationStateUnchanged(
      { hash: "before", content: null },
      { hash: "after", content: null },
    ),
    false,
  );

  const large = mutationStateForContents(Buffer.alloc(2 * 1024 * 1024 + 1, 1));
  const binary = mutationStateForContents(Buffer.from([0, 1, 2, 3]));
  for (const state of [large, binary]) {
    assert.match(state.hash, /^[a-f0-9]{64}$/);
    assert.equal(state.content, null);
  }
});


test("direct mutation projections preserve unrelated text and reject ambiguous edits", () => {
  const before = "someone else's line\nold\n";
  assert.equal(projectDirectMutation("Edit", { old_string: "old", new_string: "new" }, before), "someone else's line\nnew\n");
  assert.equal(projectDirectMutation("Edit", { old_string: "old", new_string: "new" }, "old old"), null);
  assert.equal(projectDirectMutation("Edit", { old_string: "old", new_string: "new", replace_all: true }, "old old"), "new new");
  assert.equal(projectDirectMutation("NotebookEdit", { new_source: "text" }, before), null);
});

test("a discontinuity between edits cannot claim other writers' text", () => {
  const changes = new Map();
  mergeTurnChange(changes, {path:"shared.rs",additions:1,deletions:1,baseline_hash:"a",result_hash:"b",baseline_content:"a",result_content:"b"});
  mergeTurnChange(changes, {path:"shared.rs",additions:1,deletions:1,baseline_hash:"external",result_hash:"c",baseline_content:"external",result_content:"c"});
  assert.equal(changes.get("shared.rs").baseline_hash,null);
  assert.equal(changes.get("shared.rs").baseline_content,null);
});


test("failed and mismatched file tools stay observations; successful edits retain their own evidence", async () => {
  const root = await mkdtemp(join(tmpdir(), "choro-mutation-proof-"));
  const path = join(root, "shared.txt");
  await writeFile(path, "external\nold\n");
  const before = await readMutationState(path);
  const baseline = {toolName:"Edit",toolInput:{old_string:"old",new_string:"new"},states:new Map([[path,before]]),turnId:"original-turn",direct:new Map(),observed:new Map()};
  await writeFile(path, "external\nnew\n");
  const [confirmed] = await completeDirectMutationBaseline("ok", baseline, true);
  assert.equal(confirmed.observed, false);
  assert.equal(confirmed.file.result_content,"external\nnew\n");
  baseline.direct.clear();
  await writeFile(path,"other writer\nnew\n");
  const [mismatch] = await completeDirectMutationBaseline("mismatch",baseline,true);
  assert.equal(baseline.direct.size,0);
  assert.equal(mismatch.observed, true);
  assert.equal(mismatch.file.result_content,null);
  await completeDirectMutationBaseline("failed",baseline,false);
  assert.equal(baseline.direct.size,0);
});

test("large target files are not read or hashed for attribution", async () => {
  const root = await mkdtemp(join(tmpdir(), "choro-large-mutation-"));
  const path = join(root,"video.bin");
  const handle = await open(path,"w");
  await handle.truncate(3_000_000_000);await handle.close();
  const state = await readMutationState(path);
  assert.equal(state.content,null);assert.equal(state.hash,null);
  const baseline = {toolName:"Write",toolInput:{content:"large body"},states:new Map([[path,state]]),turnId:"large-turn",direct:new Map(),observed:new Map()};
  const [activity] = await completeDirectMutationBaseline("large-success",baseline,true);
  assert.equal(activity.observed,false);
  assert.equal(activity.file.result_content,null);
  assert.equal(baseline.observed.size,0);
});

test("pending evidence has byte and event bounds and reservations release once", () => {
  const budget = new EvidenceBudget(32, 2);
  const first = budget.reserve(20);
  assert.equal(budget.reserve(13), null);
  const second = budget.reserve(12);
  assert.equal(budget.reserve(0), null);
  first(); first();
  assert.equal(budget.bytes, 12);
  assert.equal(budget.count, 1);
  second();
  assert.equal(budget.bytes, 0);
});
