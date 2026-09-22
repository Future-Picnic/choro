import assert from "node:assert/strict";
import test from "node:test";
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
  for (const name of ["Bash", "Write", "Edit", "NotebookEdit", "Agent", "mcp__penpot__update", "mcp__other__studio_apply", "mcp__choro__create_choro_doc", "mcp__choro__delegation_plan", "mcp__choro__studio_apply_extra"]) {
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
  await assert.rejects(preflightStudioRuntime({},['choro']),/repair/);
  await assert.rejects(preflightStudioRuntime(runtime([{...server,status:'failed'}]),['choro']),/repair/);
  await assert.rejects(preflightStudioRuntime(runtime([{...server,tools:[]}]),['choro']),/Missing/);
  await assert.rejects(preflightStudioRuntime(runtime([{...server,tools:[...server.tools,{name:'create_choro_doc'}]}]),['choro']),/Unexpected/);
  await assert.rejects(preflightStudioRuntime(runtime([server,{name:'other',status:'connected'}]),['choro']),/repair/);
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
