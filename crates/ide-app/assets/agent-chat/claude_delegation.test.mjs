import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";
import { managedDelegationOptions, isChoroCoordinationTool, permissionModeFor, toolPolicyDenial, toolPolicyHook } from "./claude_bridge.mjs";

test("managed sessions restrict every native spawning and peer-coordination tool", () => {
  const options = managedDelegationOptions(true);
  for (const name of ["Agent", "Task", "TeamCreate", "TeamDelete", "SendMessage"]) {
    assert.ok(options.disallowedTools.includes(name));
  }
  assert.ok(!options.disallowedTools.includes("mcp__ide__delegation_message"));
});

test("ordinary sessions keep the provider's existing native helper policy", () => {
  assert.deepEqual(managedDelegationOptions(false), {});
  assert.notEqual(managedDelegationOptions(true).disallowedTools, managedDelegationOptions(true).disallowedTools);
});

test("only the exact Choro coordination tools avoid redundant permission prompts", () => {
  assert.equal(isChoroCoordinationTool("mcp__choro__delegation_complete"), true);
  assert.equal(isChoroCoordinationTool("mcp__ide__experts_list"), true);
  for (const name of ["Bash", "mcp__other__delegation_complete", "mcp__choro__delegation_delete", "mcp__choro__delegation_complete_extra"]) {
    assert.equal(isChoroCoordinationTool(name), false);
  }
});


test("only leads enter Plan mode; children retain their chosen access policy", () => {
  for (const access of ["default", "acceptEdits", "bypassPermissions"]) {
    assert.equal(permissionModeFor("plan", access, false), "plan");
    assert.equal(permissionModeFor("plan", access, true), access);
    assert.equal(permissionModeFor("default", access, true), access);
  }
  for (const name of ["EnterPlanMode", "ExitPlanMode"]) {
    assert.ok(managedDelegationOptions(true, true).disallowedTools.includes(name));
    assert.ok(!managedDelegationOptions(true).disallowedTools.includes(name));
    assert.match(toolPolicyDenial(name, { managed: true, child: true }), /lead/);
    assert.equal(toolPolicyDenial(name, { managed: true }), null);
  }
});

test("read-only consultations can deliver results and questions without implementation access", () => {
  const policy = { managed: true, child: true, consultation: true };
  for (const tool of ["Read", "Glob", "Grep", "WebSearch", "WebFetch", "AskUserQuestion", "mcp__ide__delegation_read", "mcp__choro__delegation_message", "mcp__ide__delegation_complete"]) {
    assert.equal(toolPolicyDenial(tool, policy), null, tool);
  }
  for (const tool of ["Bash", "Edit", "MultiEdit", "Write", "NotebookEdit", "mcp__other__delegation_complete", "mcp__choro__files_write", "Agent", "EnterPlanMode", "ExitPlanMode"]) {
    assert.ok(toolPolicyDenial(tool, policy), tool);
    const hook = toolPolicyHook(tool, policy).hookSpecificOutput;
    assert.equal(hook.hookEventName, "PreToolUse");
    assert.equal(hook.permissionDecision, "deny");
    assert.ok(hook.permissionDecisionReason);
  }
  // Enforced by a PreToolUse hook even when ordinary permission prompts are bypassed.
  assert.deepEqual(toolPolicyHook("Read", policy), {});
  assert.equal(toolPolicyDenial("Write", { managed: true, child: true }), null);
  assert.equal(toolPolicyDenial("Bash", {}), null);
});

test("a read-only review checklist never gains coordination mutations", () => {
  for (const consultation of [false, true]) {
    const policy = { managed: true, child: true, consultation, readOnly: true };
    assert.equal(toolPolicyDenial("Read", policy), null);
    for (const tool of ["Bash", "Write", "mcp__ide__delegation_complete", "mcp__choro__delegation_message"]) {
      assert.match(toolPolicyDenial(tool, policy), /read.only/i);
    }
  }
});

test("Studio rejects mutations and unrelated MCP even when normal access is bypassed", () => {
  const policy = { studio: true };
  for (const name of ["Read", "Glob", "Grep", "AskUserQuestion", "mcp__choro__studio_apply", "mcp__ide__studio_read", "mcp__choro__studio_project_read"]) {
    assert.equal(toolPolicyDenial(name, policy), null, name);
  }
  for (const name of ["Bash", "Write", "Edit", "NotebookEdit", "Agent", "mcp__penpot__update", "mcp__other__studio_apply", "mcp__choro__create_choro_doc", "mcp__choro__delegation_plan", "mcp__choro__studio_apply_extra", "mcp__other__studio_import_propose", "mcp__choro__studio_import_propose_extra", "mcp__choro__studio_handoff_read"]) {
    assert.ok(toolPolicyDenial(name, policy), name);
    assert.equal(toolPolicyHook(name, policy).hookSpecificOutput.permissionDecision, "deny");
  }
});

test("Studio preflight rejects missing capabilities, disconnected or unrestricted MCP before submitting a prompt", async () => {
  const {preflightStudioRuntime} = await import('./claude_bridge.mjs');
  const names = ['studio_context','studio_read','studio_apply','studio_snapshot','studio_review'];
  const server = {name:'choro',status:'connected',tools:names.map(name=>({name}))};
  const runtime = servers => ({initializationResult:async()=>({}),mcpServerStatus:async()=>servers});
  await preflightStudioRuntime(runtime([server]),['choro']);
  await assert.rejects(preflightStudioRuntime({},['choro']),/lacks startup verification/);
  await assert.rejects(preflightStudioRuntime(runtime([{...server,status:'failed'}]),['choro']),/server is failed/);
  await assert.rejects(preflightStudioRuntime(runtime([{...server,tools:[]}]),['choro']),/Missing/);
  await assert.rejects(preflightStudioRuntime(runtime([{...server,tools:[...server.tools,{name:'create_choro_doc'}]}]),['choro']),/Unexpected MCP tool: choro: create_choro_doc/);
  await assert.rejects(preflightStudioRuntime(runtime([server,{name:'other',status:'connected'}]),['choro']),/unexpected tool server/);
});

test("Studio waits for the SDK's pending MCP startup before validating design tools", async () => {
  const { preflightStudioRuntime } = await import("./claude_bridge.mjs");
  const names = ["studio_context", "studio_apply", "studio_snapshot", "studio_review"];
  const connected = { name: "choro", status: "connected", tools: names.map(name => ({ name })) };
  for (const initial of [[], [{ name: "choro", status: "pending" }]]) {
    let polls = 0;
    await preflightStudioRuntime({
      initializationResult: async () => ({}),
      mcpServerStatus: async () => ++polls === 1 ? initial : [connected],
    }, ["choro"], { pollIntervalMs: 1 });
    assert.equal(polls, 2);
  }
});

test("Studio still rejects failed or unrestricted servers after pending startup", async () => {
  const { preflightStudioRuntime } = await import("./claude_bridge.mjs");
  const names = ["studio_context", "studio_apply", "studio_snapshot", "studio_review"];
  for (const [server, message] of [
    [{ name: "choro", status: "failed" }, /server is failed/],
    [{ name: "choro", status: "connected", tools: [] }, /Missing studio_context/],
    [{ name: "choro", status: "connected", tools: [...names, "create_choro_doc"].map(name => ({ name })) }, /Unexpected MCP tool/],
    [{ name: "other", status: "connected" }, /unexpected tool server/],
  ]) {
    let polls = 0;
    await assert.rejects(preflightStudioRuntime({
      initializationResult: async () => ({}),
      mcpServerStatus: async () => ++polls === 1 ? [{ name: "choro", status: "pending" }] : [server],
    }, ["choro"], { pollIntervalMs: 1 }), message);
  }
});

test("Studio pending startup times out and stops polling without telling users to repair Claude", async () => {
  const { preflightStudioRuntime } = await import("./claude_bridge.mjs");
  let polls = 0;
  await assert.rejects(preflightStudioRuntime({
    initializationResult: async () => ({}),
    mcpServerStatus: async () => { polls++; return [{ name: "choro", status: "pending" }]; },
  }, ["choro"], { timeoutMs: 20, pollIntervalMs: 1 }), error => {
    assert.match(error.message, /did not finish connecting/);
    assert.doesNotMatch(error.message, /Update Claude Code|repair/);
    return true;
  });
  const finalPolls = polls;
  await new Promise(resolve => setTimeout(resolve, 10));
  assert.equal(polls, finalPolls);
});

test("Studio starts with the real Claude SDK without submitting a model prompt", {
  skip: !process.env.CHORO_CLAUDE_PATH,
  timeout: 20000,
}, async () => {
  const { query } = await import("@anthropic-ai/claude-agent-sdk");
  const { preflightStudioRuntime } = await import("./claude_bridge.mjs");
  const serverCode = `
    const readline = require('node:readline');
    const names = ['studio_context', 'studio_apply', 'studio_snapshot', 'studio_review'];
    readline.createInterface({ input: process.stdin }).on('line', line => {
      const message = JSON.parse(line);
      if (message.id === undefined) return;
      const result = message.method === 'initialize'
        ? { protocolVersion: '2025-11-25', capabilities: { tools: {} }, serverInfo: { name: 'choro-fixture', version: '1' } }
        : message.method === 'tools/list'
          ? { tools: names.map(name => ({ name, description: name, inputSchema: { type: 'object', properties: {} } })) }
          : {};
      const send = () => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: message.id, result }) + '\\n');
      if (message.method === 'initialize') setTimeout(send, 200);
      else send();
    });
  `;
  let releasePrompt;
  const prompt = async function* () {
    // Initialization and tool discovery must work before any user message.
    await new Promise(resolve => { releasePrompt = resolve; });
  };
  const runtime = query({ prompt: prompt(), options: {
    cwd: process.cwd(),
    pathToClaudeCodeExecutable: process.env.CHORO_CLAUDE_PATH,
    model: "claude-opus-5-5",
    tools: ["Read", "Glob", "Grep", "AskUserQuestion"],
    disallowedTools: ["Bash", "Edit", "Write", "MultiEdit", "NotebookEdit", "Agent", "Task", "TeamCreate", "TeamDelete", "SendMessage"],
    permissionMode: "default",
    strictMcpConfig: true,
    settingSources: [],
    mcpServers: { choro: { type: "stdio", command: process.execPath, args: ["-e", serverCode] } },
    canUseTool: async () => ({ behavior: "deny", message: "Startup test only" }),
  } });
  try {
    await preflightStudioRuntime(runtime, ["choro"], { timeoutMs: 15000 });
    const servers = await runtime.mcpServerStatus();
    assert.equal(servers.length, 1);
    assert.equal(servers[0].status, "connected");
  } finally {
    runtime.close();
    releasePrompt?.();
  }
});

test("Studio preflight accepts every tool exposed by Choro's Studio server", async () => {
  const { preflightStudioRuntime } = await import("./claude_bridge.mjs");
  const source = await readFile(new URL("../../../ide-mcp/src/studio.rs", import.meta.url), "utf8");
  const allowed = source.match(/fn allowed\(name: &str\) -> bool \{([\s\S]*?)\n\}/);
  assert.ok(allowed, "locate the server's Studio allowlist");
  const names = [...allowed[1].matchAll(/"([a-z_]+)"/g)].map(match => match[1]);
  assert.ok(names.includes("studio_import_propose"));
  for (const serverName of ["choro", "ide"]) {
    for (const name of names) {
      assert.equal(toolPolicyDenial(`mcp__${serverName}__${name}`, { studio: true }), null, name);
      assert.deepEqual(toolPolicyHook(`mcp__${serverName}__${name}`, { studio: true }), {}, name);
    }
    await preflightStudioRuntime({
      initializationResult: async () => ({}),
      mcpServerStatus: async () => [{ name: serverName, status: "connected", tools: names.map(name => ({ name })) }],
    }, [serverName]);
  }
});


test("asynchronous setup failure is reported and a later command still dispatches", async () => {
  const { handleCommandLine } = await import("./claude_bridge.mjs");
  const errors = [], received = [];
  await handleCommandLine('{"type":"send_turn"}', async () => {
    await Promise.resolve(); throw new Error("Studio preflight failed");
  }, error => errors.push(error.message));
  await handleCommandLine('{"type":"send_turn","text":"retry"}', async command => received.push(command.text), error => errors.push(error.message));
  assert.deepEqual(errors, ["Studio preflight failed"]);
  assert.deepEqual(received, ["retry"]);
});
