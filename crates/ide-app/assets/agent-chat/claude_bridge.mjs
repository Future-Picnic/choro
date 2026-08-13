import { execFile } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { isAbsolute, join, relative, resolve } from "node:path";
import readline from "node:readline";
import { fileURLToPath } from "node:url";
import { query } from "@anthropic-ai/claude-agent-sdk";

const pendingUserInputs = new Map();
const pendingApprovals = new Map();
const blockIds = new Map();
const capturedPlanKeys = new Set();

let runtime = null;
let promptController = null;
let currentMessageId = null;
let currentSessionId = null;
let currentCwd = process.cwd();
let closing = false;
let planCaptured = false;
let activeTurnId = randomUUID();
let turnDirectChanges = new Map();
let turnObservedChanges = new Map();
let toolMutationBaselines = new Map();
let commandDiffBaselines = new Map();
let turnDiffEmitted = false;
let cancelRequested = false;
let currentAccessMode = "bypassPermissions";
let currentDesignAssistant = false;
let currentDesignPreviewReview = false;
let usageSessionId = null;
let usageRevision = 0;
let cumulativeUsage = emptyUsage();
const cumulativeModelUsage = new Map();

function emit(event) {
  process.stdout.write(`${JSON.stringify(event)}\n`);
}

function emitError(error) {
  emit({
    type: "error",
    message: error instanceof Error ? error.message : String(error),
  });
}

function emptyUsage() {
  return {
    reported_total_tokens: 0,
    input_tokens: 0,
    output_tokens: 0,
    reasoning_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cost_usd: 0,
  };
}

function finiteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? value : 0;
}

function sdkUsageTotals(usage) {
  const input = finiteNumber(usage?.input_tokens ?? usage?.inputTokens);
  const output = finiteNumber(usage?.output_tokens ?? usage?.outputTokens);
  const cacheRead = finiteNumber(
    usage?.cache_read_input_tokens ?? usage?.cacheReadInputTokens,
  );
  const cacheWrite = finiteNumber(
    usage?.cache_creation_input_tokens ?? usage?.cacheCreationInputTokens,
  );
  return {
    reported_total_tokens: input + output + cacheRead + cacheWrite,
    input_tokens: input,
    output_tokens: output,
    reasoning_tokens: 0,
    cache_read_tokens: cacheRead,
    cache_write_tokens: cacheWrite,
    cost_usd: 0,
  };
}

function addUsage(target, usage) {
  for (const key of [
    "reported_total_tokens",
    "input_tokens",
    "output_tokens",
    "reasoning_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
  ]) {
    target[key] += finiteNumber(usage[key]);
  }
}

function replaceUsageFromModels(modelUsage) {
  const entries = Object.entries(modelUsage || {});
  if (entries.length === 0) {
    return false;
  }

  const totals = emptyUsage();
  const models = new Map();
  for (const [modelId, usage] of entries) {
    const modelTotals = sdkUsageTotals(usage);
    addUsage(totals, modelTotals);
    models.set(modelId, modelTotals);
  }
  if (
    totals.reported_total_tokens === 0 ||
    totals.reported_total_tokens < cumulativeUsage.reported_total_tokens
  ) {
    return false;
  }
  cumulativeUsage = totals;
  cumulativeModelUsage.clear();
  for (const [modelId, modelTotals] of models) {
    cumulativeModelUsage.set(modelId, modelTotals);
  }
  return true;
}

function emitUsageSnapshot(sessionId, latestTurn) {
  emit({
    type: "usage",
    session_id: sessionId,
    totals: cumulativeUsage,
    latest_turn: latestTurn,
    models: Array.from(cumulativeModelUsage, ([model_id, totals]) => ({
      provider_id: "Claude",
      model_id,
      totals,
    })),
  });
}

async function refreshFullSessionUsage(activeRuntime, sessionId, revision, latestTurn) {
  const getUsage =
    activeRuntime?.usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET;
  if (typeof getUsage !== "function") {
    return;
  }
  try {
    const usage = await getUsage.call(activeRuntime);
    if (usageSessionId !== sessionId || usageRevision !== revision) {
      return;
    }
    if (replaceUsageFromModels(usage?.session?.model_usage)) {
      emitUsageSnapshot(sessionId, latestTurn);
    }
  } catch {
    // Result messages already provide exact turn usage on SDK versions where
    // the optional session-level control request is unavailable.
  }
}

function emitUsage(message) {
  const sessionId = message.session_id || message.sessionId || currentSessionId;
  if (!sessionId) {
    return;
  }
  if (usageSessionId !== sessionId) {
    usageSessionId = sessionId;
    cumulativeUsage = emptyUsage();
    cumulativeModelUsage.clear();
  }
  usageRevision += 1;
  const revision = usageRevision;

  const latestTurn = sdkUsageTotals(message.usage);
  if (latestTurn.reported_total_tokens === 0) {
    return;
  }

  addUsage(cumulativeUsage, latestTurn);
  for (const [modelId, usage] of Object.entries(message.modelUsage || {})) {
    const modelTotals = sdkUsageTotals(usage);
    const totals = cumulativeModelUsage.get(modelId) || emptyUsage();
    addUsage(totals, modelTotals);
    cumulativeModelUsage.set(modelId, totals);
  }

  emitUsageSnapshot(sessionId, latestTurn);
  void refreshFullSessionUsage(runtime, sessionId, revision, latestTurn);
}

function execGit(args) {
  return new Promise((resolve) => {
    execFile("git", ["-C", currentCwd, ...args], (error, stdout) => {
      if (error) {
        resolve("");
      } else {
        resolve(stdout || "");
      }
    });
  });
}

function createPromptController() {
  const queue = [];
  const waiters = [];
  let closed = false;

  return {
    enqueue(text) {
      if (closed) {
        return;
      }
      const message = {
        type: "user",
        message: {
          role: "user",
          content: text,
        },
        parent_tool_use_id: null,
      };
      const waiter = waiters.shift();
      if (waiter) {
        waiter(message);
      } else {
        queue.push(message);
      }
    },
    async *stream() {
      while (!closed && !closing) {
        if (queue.length > 0) {
          yield queue.shift();
          continue;
        }
        const next = await new Promise((resolve) => waiters.push(resolve));
        if (next) {
          yield next;
        }
      }
    },
    close() {
      closed = true;
      queue.length = 0;
      for (const waiter of waiters.splice(0)) {
        waiter(null);
      }
    },
  };
}

async function ensureRuntime(command) {
  currentAccessMode = command.accessMode || "bypassPermissions";
  currentDesignAssistant = Boolean(command.designAssistant);
  currentDesignPreviewReview = Boolean(command.designPreviewReview);
  const resumeSessionId = command.sessionId || command.session_id || null;
  if (runtime) {
    if (typeof runtime.setPermissionMode === "function") {
      await runtime.setPermissionMode(permissionModeFor(command.mode, command.accessMode));
    }
    if (typeof runtime.setModel === "function") {
      await runtime.setModel(command.model || undefined);
    }
    return;
  }

  currentCwd = command.cwd || currentCwd;
  currentSessionId = resumeSessionId || currentSessionId;
  promptController = createPromptController();
  runtime = query({
    prompt: promptController.stream(),
    options: {
      cwd: currentCwd,
      resume: resumeSessionId || undefined,
      additionalDirectories: [currentCwd],
      pathToClaudeCodeExecutable: command.claudePath,
      model: command.model || undefined,
      effort: command.effort || undefined,
      permissionMode: permissionModeFor(command.mode, command.accessMode),
      allowDangerouslySkipPermissions: true,
      includePartialMessages: true,
      canUseTool,
      hooks: fileAttributionHooks(),
      mcpServers: command.mcpServers || undefined,
      strictMcpConfig: currentDesignAssistant,
      settingSources: currentDesignAssistant ? [] : undefined,
      sandbox: currentDesignAssistant
        ? {
            enabled: true,
            failIfUnavailable: true,
            autoAllowBashIfSandboxed: false,
            allowUnsandboxedCommands: false,
            filesystem: {
              allowRead: [currentCwd],
              allowWrite: [currentCwd],
            },
          }
        : undefined,
      systemPrompt: {
        type: "preset",
        preset: "claude_code",
        append: command.systemPrompt || undefined,
      },
    },
  });

  void consumeRuntime(runtime);
}

function permissionModeFor(mode, accessMode) {
  if (mode === "plan") {
    return "plan";
  }
  return accessMode || "bypassPermissions";
}

async function canUseTool(toolName, input, options) {
  if (
    currentDesignAssistant &&
    typeof toolName === "string" &&
    toolName.startsWith("mcp__") &&
    !toolName.startsWith("mcp__choro__") &&
    !toolName.startsWith("mcp__ide__") &&
    !toolName.startsWith("mcp__penpot__")
  ) {
    return {
      behavior: "deny",
      message:
        "This Design Assistant is isolated to its Choro and Design tools. Other MCP servers are unavailable in this session.",
    };
  }

  if (toolName === "AskUserQuestion") {
    return handleAskUserQuestion(input, options);
  }

  if (toolName === "ExitPlanMode") {
    const plan = extractPlan(input);
    if (plan) {
      emitProposedPlan(options.toolUseID || randomUUID(), plan);
    }
    return {
      behavior: "deny",
      message:
        "The client captured your proposed plan. Stop here and wait for the user's feedback or implementation request in a later turn.",
    };
  }

  if (currentDesignAssistant) {
    if (["Read", "Glob", "Grep"].includes(toolName)) {
      return { behavior: "allow", updatedInput: input };
    }
    if (
      typeof toolName === "string" &&
      /^(mcp__(?:choro|ide)__(?:task_read|task_list|task_image))$/.test(
        toolName,
      )
    ) {
      return { behavior: "allow", updatedInput: input };
    }
    if (
      typeof toolName === "string" &&
      toolName.startsWith("mcp__penpot__")
    ) {
      return { behavior: "allow", updatedInput: input };
    }
    if (currentDesignPreviewReview) {
      // A Compare Review deliberately keeps the user in the design
      // conversation while making the implementation the write target. Honor
      // the user's normal project access mode for repository tools only.
      return handleToolPermission(toolName, input, options);
    }
    return {
      behavior: "deny",
      message:
        "This Design Assistant can inspect project context but can only make changes through its exact Design connection.",
    };
  }

  // Compatibility for bundles that registered the first-party server as `ide`.
  if (typeof toolName === "string" && toolName.startsWith("mcp__ide__")) {
    return { behavior: "allow", updatedInput: input };
  }

  // Project Preview only opens Choro's isolated native web surface. Allow it
  // without surfacing a shell-style permission prompt; filesystem and browser
  // safety still live in the agent and Preview target validation.
  if (
    typeof toolName === "string" &&
    ["mcp__choro__preview_open"].includes(toolName)
  ) {
    return { behavior: "allow", updatedInput: input };
  }

  return handleToolPermission(toolName, input, options);
}

function isEditTool(toolName) {
  return ["Edit", "MultiEdit", "Write", "NotebookEdit"].includes(toolName);
}

function projectRelativePath(path) {
  if (typeof path !== "string" || path.trim().length === 0) {
    return null;
  }
  const absolute = isAbsolute(path) ? resolve(path) : resolve(currentCwd, path);
  const local = relative(currentCwd, absolute);
  return local === "" || local.startsWith(`..${process.platform === "win32" ? "\\" : "/"}`)
    ? absolute
    : local;
}

function mutationPaths(toolName, input) {
  if (!isEditTool(toolName) || !input || typeof input !== "object") {
    return [];
  }
  const candidates = [
    input.file_path,
    input.filePath,
    input.notebook_path,
    input.notebookPath,
    input.path,
  ];
  return Array.from(
    new Set(candidates.map(projectRelativePath).filter(Boolean)),
  );
}

async function readMutationState(path) {
  const absolute = isAbsolute(path) ? path : join(currentCwd, path);
  try {
    return mutationStateForContents(await readFile(absolute));
  } catch {
    return { hash: "missing", lines: 0, content: "" };
  }
}

function mutationStateForContents(contents) {
  const hash = createHash("sha256").update(contents).digest("hex");
  // Attribution metadata stays bounded. Large/binary files retain a compact
  // fingerprint so edits and reverts remain distinguishable, but not content.
  if (contents.byteLength > 2 * 1024 * 1024 || contents.includes(0)) {
    return { hash, lines: 0, content: null };
  }
  const text = contents.toString("utf8");
  return { hash, lines: countTextLines(text), content: text };
}

function mutationStateUnchanged(before, after) {
  if (before?.hash != null && after?.hash != null) {
    return before.hash === after.hash;
  }
  return (
    before?.content != null &&
    after?.content != null &&
    before.content === after.content
  );
}

function countTextLines(text) {
  if (typeof text !== "string" || text.length === 0) {
    return 0;
  }
  return text.replace(/\r?\n$/, "").split(/\r\n|\r|\n/).length;
}

function directEditCounts(toolName, input, before, after) {
  if (toolName === "Edit") {
    return [countTextLines(input?.new_string), countTextLines(input?.old_string)];
  }
  if (toolName === "MultiEdit") {
    return (Array.isArray(input?.edits) ? input.edits : []).reduce(
      ([additions, deletions], edit) => [
        additions + countTextLines(edit?.new_string),
        deletions + countTextLines(edit?.old_string),
      ],
      [0, 0],
    );
  }
  if (toolName === "Write") {
    return [countTextLines(input?.content), before?.lines || 0];
  }
  if (toolName === "NotebookEdit") {
    return [countTextLines(input?.new_source || input?.source), before?.lines || 0];
  }
  return [Math.max(after?.lines || 0, 0), Math.max(before?.lines || 0, 0)];
}

function mergeTurnChange(target, change) {
  const existing = target.get(change.path);
  if (!existing) {
    target.set(change.path, change);
    return;
  }
  target.set(change.path, {
    path: change.path,
    additions: existing.additions + change.additions,
    deletions: existing.deletions + change.deletions,
    baseline_hash: existing.baseline_hash ?? change.baseline_hash ?? null,
    result_hash: change.result_hash ?? existing.result_hash ?? null,
    baseline_content:
      existing.baseline_content ?? change.baseline_content ?? null,
    result_content: change.result_content ?? existing.result_content ?? null,
  });
}

function setTurnProjection(target, change) {
  target.set(change.path, change);
}

function fileChangeActivityEvent(actionID, change, observed) {
  return {
    type: "file_change_activity",
    id: `claude:${actionID || "tool"}:${change.path}`,
    turn_id: activeTurnId,
    observed,
    file: change,
  };
}

function emitFileChangeActivity(actionID, change, observed) {
  emit(fileChangeActivityEvent(actionID, change, observed));
}

async function finishTurnAfterFileReceipt(flushChanges, finish) {
  await flushChanges();
  return finish();
}

async function captureToolMutationBaseline(input, toolUseID) {
  const toolName = input?.tool_name;
  const toolInput = input?.tool_input || {};
  const id = input?.tool_use_id || toolUseID || randomUUID();
  if (isEditTool(toolName)) {
    const paths = mutationPaths(toolName, toolInput);
    const states = new Map();
    await Promise.all(
      paths.map(async (path) => states.set(path, await readMutationState(path))),
    );
    toolMutationBaselines.set(id, { toolName, toolInput, states });
  } else if (toolName === "Bash") {
    // Commands do not expose authoritative write paths. A command-scoped diff
    // is still useful, but it is emitted separately as an observation.
    commandDiffBaselines.set(id, await readChangedFileSnapshot());
  }
  return {};
}

async function captureCompletedToolMutation(input, toolUseID) {
  const toolName = input?.tool_name;
  const id = input?.tool_use_id || toolUseID;
  if (isEditTool(toolName)) {
    const baseline = toolMutationBaselines.get(id);
    toolMutationBaselines.delete(id);
    if (!baseline) {
      return {};
    }
    await completeDirectMutationBaseline(id, baseline);
  } else if (toolName === "Bash") {
    const baseline = commandDiffBaselines.get(id);
    commandDiffBaselines.delete(id);
    if (baseline) {
      await completeCommandMutationBaseline(id, baseline);
    }
  }
  return {};
}

async function completeDirectMutationBaseline(id, baseline) {
  for (const [path, before] of baseline.states) {
    const after = await readMutationState(path);
    if (mutationStateUnchanged(before, after)) {
      continue;
    }
    const [additions, deletions] = directEditCounts(
      baseline.toolName,
      baseline.toolInput,
      before,
      after,
    );
    const change = {
      path,
      additions,
      deletions,
      baseline_hash: before.hash,
      result_hash: after.hash,
      baseline_content: before.content,
      result_content: after.content,
    };
    mergeTurnChange(turnDirectChanges, change);
    emitFileChangeActivity(id, change, false);
  }
}

async function completeCommandMutationBaseline(id, baseline, next) {
  const current = next || (await readChangedFileSnapshot());
  for (const [path, file] of current) {
    if (baseline.get(path)?.patch !== file.patch) {
      // Git reports the current worktree projection, not a delta for this
      // command. Keep only the latest projection for the turn.
      const change = {
        path,
        additions: file.additions,
        deletions: file.deletions,
        baseline_hash: null,
        result_hash: null,
        baseline_content: null,
        result_content: null,
      };
      setTurnProjection(turnObservedChanges, change);
      emitFileChangeActivity(id, change, true);
    }
  }
}

async function captureOutstandingToolMutations() {
  // Remove entries before awaiting reads. If a late SDK completion hook races
  // cancellation, only one path owns each baseline and the receipt stays exact.
  const direct = Array.from(toolMutationBaselines.entries());
  toolMutationBaselines.clear();
  for (const [id, baseline] of direct) {
    await completeDirectMutationBaseline(id, baseline);
  }

  const commands = Array.from(commandDiffBaselines.entries());
  commandDiffBaselines.clear();
  if (commands.length > 0) {
    const next = await readChangedFileSnapshot();
    for (const [id, baseline] of commands) {
      await completeCommandMutationBaseline(id, baseline, next);
    }
  }
}

async function finishCancelledTurn(capturePending, flushChanges, finish) {
  await capturePending();
  return finishTurnAfterFileReceipt(flushChanges, finish);
}

function fileAttributionHooks() {
  const completed = [{ hooks: [captureCompletedToolMutation] }];
  return {
    PreToolUse: [{ hooks: [captureToolMutationBaseline] }],
    PostToolUse: completed,
    // Tools can mutate the filesystem before reporting failure (for example,
    // `touch generated.txt && false`). Complete the same baseline in that path.
    PostToolUseFailure: completed,
  };
}

function handleToolPermission(toolName, input, options) {
  if (currentAccessMode === "bypassPermissions") {
    return {
      behavior: "allow",
      updatedInput: input,
    };
  }

  if (currentAccessMode === "acceptEdits" && isEditTool(toolName)) {
    return {
      behavior: "allow",
      updatedInput: input,
    };
  }

  const requestId = randomUUID();
  const displayName = options.displayName || options.title || toolName;
  const kind = isEditTool(toolName)
    ? "file_change"
    : toolName === "Bash"
      ? "command"
      : "permissions";
  const detail = summarizeToolInput(input) || options.description || "Run this action once.";
  emit({
    type: "pending_approval",
    request_id: requestId,
    kind,
    title: `Allow ${displayName}?`,
    detail,
  });

  return new Promise((resolve) => {
    const abort = () => {
      pendingApprovals.delete(requestId);
      resolve({
        behavior: "deny",
        message: "User cancelled tool execution.",
      });
    };
    options.signal?.addEventListener("abort", abort, { once: true });
    pendingApprovals.set(requestId, {
      resolve: (approved) => {
        options.signal?.removeEventListener("abort", abort);
        resolve(
          approved
            ? { behavior: "allow", updatedInput: input }
            : { behavior: "deny", message: "User denied tool execution." },
        );
      },
    });
  });
}

function handleAskUserQuestion(input, options) {
  const requestId = randomUUID();
  const rawQuestions = Array.isArray(input?.questions) ? input.questions : [];
  const questions = rawQuestions.map((question, index) => {
    const text =
      typeof question?.question === "string" && question.question.length > 0
        ? question.question
        : `Question ${index + 1}`;
    return {
      id: text,
      header:
        typeof question?.header === "string" && question.header.length > 0
          ? question.header
          : `Question ${index + 1}`,
      question: text,
      multi_select: question?.multiSelect === true || question?.multi_select === true,
      options: Array.isArray(question?.options)
        ? question.options.map((option) => ({
            label: typeof option?.label === "string" ? option.label : "",
            description:
              typeof option?.description === "string" ? option.description : "",
          }))
        : [],
    };
  });

  emit({
    type: "pending_user_input",
    request_id: requestId,
    questions,
  });

  return new Promise((resolve) => {
    const abort = () => {
      pendingUserInputs.delete(requestId);
      resolve({
        behavior: "deny",
        message: "User cancelled tool execution.",
      });
    };
    options.signal?.addEventListener("abort", abort, { once: true });
    pendingUserInputs.set(requestId, {
      questions,
      resolve: (answers) => {
        options.signal?.removeEventListener("abort", abort);
        const answerMap = {};
        questions.forEach((question, index) => {
          answerMap[question.id] = answers[index] || "";
        });
        resolve({
          behavior: "allow",
          updatedInput: {
            questions: input.questions,
            answers: answerMap,
          },
        });
      },
    });
  });
}

function extractPlan(input) {
  if (!input || typeof input !== "object") {
    return "";
  }
  for (const key of ["plan", "markdown", "content"]) {
    if (typeof input[key] === "string" && input[key].trim().length > 0) {
      return input[key].trim();
    }
  }
  return "";
}

async function consumeRuntime(activeRuntime) {
  try {
    emit({ type: "status", status: "running" });
    for await (const message of activeRuntime) {
      await handleSdkMessage(message);
    }
  } catch (error) {
    if (!closing && !cancelRequested) {
      emitError(error);
    }
  } finally {
    if (runtime === activeRuntime) {
      runtime = null;
      promptController?.close();
      promptController = null;
    }
    if (!closing) {
      emit({ type: "status", status: "idle" });
      if (!cancelRequested) {
        await emitChangedFiles();
      }
      cancelRequested = false;
    }
  }
}

async function closeRuntimeForPlanBoundary() {
  const activeRuntime = runtime;
  const activePromptController = promptController;
  if (!activeRuntime) {
    return;
  }
  activePromptController?.close();
  try {
    if (typeof activeRuntime.interrupt === "function") {
      await activeRuntime.interrupt();
    }
  } catch {
    // Closing below is the important boundary; interrupt is best-effort.
  }
  try {
    activeRuntime.close?.();
  } finally {
    if (runtime === activeRuntime) {
      runtime = null;
    }
    if (promptController === activePromptController) {
      promptController = null;
    }
  }
}

async function cancelActiveTurn() {
  cancelRequested = true;
  emit({ type: "status", status: "cancelling" });
  const activeRuntime = runtime;
  const activePromptController = promptController;
  activePromptController?.close();
  try {
    if (typeof activeRuntime?.interrupt === "function") {
      await activeRuntime.interrupt();
    }
  } catch {
    // Closing below is the hard boundary if interrupt is not supported.
  }
  // A cancelled turn can already have completed edits. Flush their immutable
  // receipt before closing the runtime and resetting the next turn's maps.
  try {
    await finishCancelledTurn(
      captureOutstandingToolMutations,
      emitChangedFiles,
      () => activeRuntime?.close?.(),
    );
  } finally {
    if (runtime === activeRuntime) {
      runtime = null;
    }
    if (promptController === activePromptController) {
      promptController = null;
    }
  }
}

async function handleSdkMessage(message) {
  const sessionId = message.session_id || message.sessionId;
  if (sessionId && sessionId !== currentSessionId) {
    currentSessionId = sessionId;
    emit({ type: "session_ready", session_id: sessionId });
  }

  if (message.type === "stream_event") {
    handleStreamEvent(message);
    return;
  }

  if (message.type === "assistant") {
    currentMessageId = message.message?.id || message.uuid || currentMessageId;
    for (const [index, block] of (message.message?.content || []).entries()) {
      if (block?.type === "tool_use") {
        emitWorkLog(
          block.id || `${message.uuid}-${index}`,
          block.name || "Tool call",
          "completed",
          summarizeToolInput(block.input),
        );
        if (block.name === "ExitPlanMode") {
          const plan = extractPlan(block.input);
          if (plan) {
            emitProposedPlan(block.id || randomUUID(), plan);
          }
        }
      }
    }
    return;
  }

  if (message.type === "result") {
    emitUsage(message);
    // File actions can complete before a later provider error. A terminal
    // result always closes the attribution window, regardless of success.
    await finishTurnAfterFileReceipt(emitChangedFiles, () => {
      if (message.subtype === "error" || message.is_error) {
        emitError(message.result || message.error || "Claude returned an error.");
      } else {
        emit({ type: "status", status: "idle" });
      }
    });
  }

  if (message.type === "system" && message.subtype === "permission_denied") {
    emitError(message.message || `${message.tool_name} was denied.`);
  }
}

function stringValue(value) {
  return typeof value === "string" ? value.trim() : "";
}

function handleStreamEvent(message) {
  const event = message.event;
  if (!event) {
    return;
  }

  if (event.type === "message_start") {
    currentMessageId = event.message?.id || message.uuid;
    blockIds.clear();
    return;
  }

  if (event.type === "content_block_start") {
    const index = event.index ?? 0;
    const block = event.content_block;
    const blockId = block?.id || `${currentMessageId || message.uuid}:${index}`;
    blockIds.set(index, blockId);
    if (block?.type === "tool_use") {
      emitWorkLog(blockId, block.name || "Tool call", "in_progress", "");
    }
    return;
  }

  if (event.type !== "content_block_delta") {
    return;
  }

  const index = event.index ?? 0;
  const blockId = blockIds.get(index) || `${currentMessageId || message.uuid}:${index}`;
  const delta = event.delta;
  if (delta?.type === "text_delta" && delta.text) {
    if (planCaptured) {
      return;
    }
    emit({
      type: "assistant_chunk",
      message_id: blockId,
      text: delta.text,
    });
  } else if (delta?.type === "thinking_delta" && delta.thinking) {
    if (planCaptured) {
      return;
    }
    emit({
      type: "thought_chunk",
      message_id: blockId,
      text: delta.thinking,
    });
  }
}

function emitProposedPlan(id, markdown) {
  const plan = typeof markdown === "string" ? markdown.trim() : "";
  if (!plan) {
    return;
  }
  const key = id ? `tool:${id}` : `plan:${plan}`;
  if (capturedPlanKeys.has(key)) {
    return;
  }
  capturedPlanKeys.add(key);
  planCaptured = true;
  emit({
    type: "proposed_plan",
    id: id || randomUUID(),
    markdown: plan,
  });
}

function emitWorkLog(id, title, status, detail) {
  emit({
    type: "work_log",
    id,
    collapse_key: id,
    kind: title === "Bash" ? "command" : "tool",
    title,
    status,
    detail: detail || null,
  });
}

function summarizeToolInput(input) {
  if (!input || typeof input !== "object") {
    return "";
  }
  if (typeof input.command === "string") {
    return input.command;
  }
  if (typeof input.file_path === "string") {
    return input.file_path;
  }
  if (typeof input.path === "string") {
    return input.path;
  }
  return "";
}

async function readChangedFileSnapshot() {
  const stdout = await execGit(["diff", "--numstat"]);
  const rows = stdout.trim()
    ? stdout
    .trim()
    .split("\n")
    .map((line) => {
      const [additions, deletions, ...pathParts] = line.split(/\t/);
      const path = pathParts.join("\t");
      return {
        path,
        additions: additions === "-" ? 0 : Number(additions) || 0,
        deletions: deletions === "-" ? 0 : Number(deletions) || 0,
      };
    })
        .filter((file) => file.path.length > 0)
    : [];

  const snapshot = new Map();
  await Promise.all(
    rows.map(async (file) => {
      const patch = await execGit(["diff", "--", file.path]);
      snapshot.set(file.path, { ...file, patch });
    }),
  );

  const untrackedStdout = await execGit(["ls-files", "--others", "--exclude-standard"]);
  const untrackedPaths = untrackedStdout
    .split("\n")
    .map((path) => path.trim())
    .filter(Boolean);

  await Promise.all(
    untrackedPaths.map(async (path) => {
      if (snapshot.has(path)) {
        return;
      }
      try {
        const contents = await readFile(join(currentCwd, path), "utf8");
        const additions = contents.length
          ? contents.replace(/\r?\n$/, "").split(/\r\n|\r|\n/).length
          : 0;
        snapshot.set(path, {
          path,
          additions,
          deletions: 0,
          patch: `untracked:${path}\n${contents}`,
        });
      } catch {
        // Ignore unreadable or binary untracked files for chat summaries.
      }
    }),
  );

  return snapshot;
}

async function emitChangedFiles() {
  if (turnDiffEmitted) {
    return;
  }
  turnDiffEmitted = true;
  const files = Array.from(turnDirectChanges.values());
  const exactPaths = new Set(files.map((file) => file.path));
  const observed_files = Array.from(turnObservedChanges.values()).filter(
    (file) => !exactPaths.has(file.path),
  );
  if (files.length > 0 || observed_files.length > 0) {
    emit({
      type: "changed_files",
      turn_id: activeTurnId,
      attribution_version: 1,
      files,
      observed_files,
    });
  }
}

async function handleCommand(command) {
  if (command.type === "send_turn") {
    cancelRequested = false;
    if (planCaptured && command.mode !== "plan") {
      await closeRuntimeForPlanBoundary();
    }
    planCaptured = false;
    capturedPlanKeys.clear();
    activeTurnId = randomUUID();
    turnDirectChanges = new Map();
    turnObservedChanges = new Map();
    toolMutationBaselines = new Map();
    commandDiffBaselines = new Map();
    turnDiffEmitted = false;
    await ensureRuntime(command);
    emit({ type: "status", status: "running" });
    promptController?.enqueue(command.text || "");
  } else if (command.type === "submit_user_input") {
    const pending = pendingUserInputs.get(command.request_id);
    if (pending) {
      pendingUserInputs.delete(command.request_id);
      pending.resolve(Array.isArray(command.answers) ? command.answers : []);
    }
  } else if (command.type === "resolve_approval") {
    const pending = pendingApprovals.get(command.request_id);
    if (pending) {
      pendingApprovals.delete(command.request_id);
      pending.resolve(command.approved === true);
    }
  } else if (command.type === "cancel_turn") {
    await cancelActiveTurn();
  } else if (command.type === "shutdown") {
    closing = true;
    promptController?.close();
    runtime?.close?.();
    process.exit(0);
  }
}

if (resolve(process.argv[1] || "") === resolve(fileURLToPath(import.meta.url))) {
  readline
    .createInterface({ input: process.stdin, crlfDelay: Infinity })
    .on("line", (line) => {
      if (!line.trim()) {
        return;
      }
      try {
        void handleCommand(JSON.parse(line));
      } catch (error) {
        emitError(error);
      }
    });
}

export {
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
};
