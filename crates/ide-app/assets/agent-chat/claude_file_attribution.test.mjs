import assert from "node:assert/strict";
import test from "node:test";

import {
  countTextLines,
  directEditCounts,
  fileChangeActivityEvent,
  fileAttributionHooks,
  finishCancelledTurn,
  finishTurnAfterFileReceipt,
  mergeTurnChange,
  mutationStateForContents,
  mutationStateUnchanged,
  setTurnProjection,
} from "./claude_bridge.mjs";

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
