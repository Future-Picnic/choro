import assert from "node:assert/strict";
import test from "node:test";
import {mkdtemp} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {reviewRuntimeOptions, preflightReviewRuntime, toolPolicyDenial,validateReviewSpawn} from "./claude_bridge.mjs";

const names = ["review_context", "review_read", "review_search", "review_report"];
const server = (tools = names) => ({name:"choro",status:"connected",tools:tools.map(name=>({name}))});
const usage = () => ({systemTools:[],memoryFiles:[],mcpTools:names.map(name=>({name,serverName:"choro"}))});
const runtime = servers => ({initializationResult:async()=>({}),mcpServerStatus:async()=>servers,getContextUsage:async()=>usage()});

test("review role denies every native and unrelated service tool", () => {
  for (const name of names) assert.equal(toolPolicyDenial(`mcp__choro__${name}`,{review:true,readOnly:true}),null);
  for (const tool of ["Read","Glob","Grep","Bash","Write","Edit","WebSearch","WebFetch","AskUserQuestion","Agent","TeamCreate","EnterPlanMode","mcp__choro__task_read","mcp__choro__delegation_plan","mcp__other__review_read","mcp__choro__review_read_extra"]) assert.ok(toolPolicyDenial(tool,{review:true}),tool);
  const options=reviewRuntimeOptions(); assert.deepEqual(options.tools,[]); assert.deepEqual(options.plugins,[]); assert.equal(options.settings.disableAllHooks,true);
  assert.equal(options.settings.autoMemoryEnabled,false); assert.equal(options.settings.disableRemoteControl,true);
  assert.equal(options.strictMcpConfig,true); assert.deepEqual(options.settingSources,[]); assert.equal(options.allowDangerouslySkipPermissions,false);
});
test("review rejects missing duplicate or weakened provider launch restrictions",()=>{
  const args=["--tools","","--setting-sources=","--permission-mode","default","--strict-mcp-config","--settings",JSON.stringify(reviewRuntimeOptions().settings)];
  validateReviewSpawn(args);
  for (const extra of [["--tools","Read"],["--setting-sources=user"],["--permission-mode","bypassPermissions"],["--resume","old"],["--continue"],["--agent","other"],["--allow-dangerously-skip-permissions"],["--dangerously-skip-permissions=true"]]) {
    assert.throws(()=>validateReviewSpawn([...args,...extra]),/restrictions/);
  }
  assert.throws(()=>validateReviewSpawn(args.filter(a=>a!=="--strict-mcp-config")),/restrictions/);
  const unsafe=[...args];unsafe[unsafe.length-1]=JSON.stringify({disableAllHooks:false,autoMemoryEnabled:true,disableRemoteControl:false});
  assert.throws(()=>validateReviewSpawn(unsafe),/restrictions/);
});
test("review verifies exactly four scoped tools before submitting a prompt",async()=>{
  await preflightReviewRuntime(runtime([server()]),["choro"]);
  await assert.rejects(preflightReviewRuntime({},["choro"]),/cannot verify/);
  await assert.rejects(preflightReviewRuntime(runtime([server()]),[]),/exactly one/);
  await assert.rejects(preflightReviewRuntime(runtime([server(),{name:"external",status:"connected"}]),["choro"]),/unrelated/);
  await assert.rejects(preflightReviewRuntime(runtime([server([...names,"task_read"])]),["choro"]),/restrictions/);
  await assert.rejects(preflightReviewRuntime(runtime([server(names.slice(1))]),["choro"]),/restrictions/);
  for (const extra of [{systemTools:[{name:"Bash"}]},{deferredBuiltinTools:[{name:"Agent"}]},{memoryFiles:[{path:"unrelated/CLAUDE.md"}]},{skills:{includedSkills:1}},{mcpTools:[{name:"leak",serverName:"external"}]}]) {
    const candidate=runtime([server()]); candidate.getContextUsage=async()=>({...usage(),...extra});
    await assert.rejects(preflightReviewRuntime(candidate,["choro"]),/inherited/);
  }
});
test("review waits for scoped startup but times out without exposing context",async()=>{
  let polls=0;
  const candidate={initializationResult:async()=>({}),mcpServerStatus:async()=>++polls<3?[{name:"choro",status:"pending"}]:[server()],getContextUsage:async()=>usage()};
  await preflightReviewRuntime(candidate,["choro"],{timeoutMs:100,pollIntervalMs:1}); assert.equal(polls,3);
  await assert.rejects(preflightReviewRuntime(runtime([{name:"choro",status:"pending"}]),["choro"],{timeoutMs:10,pollIntervalMs:1}),/timed out/);
});

test("review verifies the real installed SDK without a model prompt or source",{skip:!process.env.CHORO_CLAUDE_PATH,timeout:30000},async()=>{
  const {query}=await import("@anthropic-ai/claude-agent-sdk");
  const cwd=await mkdtemp(join(tmpdir(),"choro-review-startup-"));
  const code=`const readline=require('node:readline'); const names=${JSON.stringify(names)};
    readline.createInterface({input:process.stdin}).on('line',line=>{const m=JSON.parse(line);if(m.id===undefined)return;
    const result=m.method==='initialize'?{protocolVersion:'2025-11-25',capabilities:{tools:{}},serverInfo:{name:'review-fixture',version:'1'}}:
      m.method==='tools/list'?{tools:names.map(name=>({name,description:name,inputSchema:{type:'object',properties:{}}}))}:{};
    process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:m.id,result})+'\\n');});`;
  for (const nativeTools of [[],["Read"]]) {
    let release;
    const prompt=async function*(){await new Promise(resolve=>{release=resolve;});};
    const options={...reviewRuntimeOptions(),tools:nativeTools,cwd,
      pathToClaudeCodeExecutable:process.env.CHORO_CLAUDE_PATH,systemPrompt:"Startup verification only. No source or model prompt.",
      mcpServers:{choro:{type:"stdio",command:process.execPath,args:["-e",code]}},
      canUseTool:async()=>({behavior:"deny",message:"Startup verification only"})};
    if(nativeTools.length) options.disallowedTools=[];
    let candidate;
    const start=async()=>{candidate=query({prompt:prompt(),options});await preflightReviewRuntime(candidate,["choro"],{timeoutMs:10000});};
    try {
      if(nativeTools.length) await assert.rejects(start(),/restrictions|inherited/);
      else await start();
    } finally {candidate?.close();release?.();}
  }
});
