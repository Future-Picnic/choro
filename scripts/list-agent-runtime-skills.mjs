#!/usr/bin/env node
import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import process from "node:process";
import readline from "node:readline";
import { pathToFileURL } from "node:url";

const repoRoot = path.resolve(import.meta.dirname, "..");
const defaultCwd = process.cwd() === path.dirname(import.meta.filename) ? repoRoot : process.cwd();

const args = parseArgs(process.argv.slice(2));
const cwd = path.resolve(args.cwd || defaultCwd);
const provider = args.provider || "all";
const timeoutMs = Number(args.timeout || 15000);

if (!["all", "codex", "claude"].includes(provider)) {
  fail(`Unknown provider "${provider}". Use --provider all|codex|claude.`);
}

const report = {
  cwd,
  generatedAt: new Date().toISOString(),
  codex: null,
  claude: null,
};

if (provider === "all" || provider === "codex") {
  report.codex = await collectCodex({ cwd, timeoutMs });
}

if (provider === "all" || provider === "claude") {
  report.claude = await collectClaude({ cwd, timeoutMs, claudePath: args.claudePath });
}

if (args.json) {
  console.log(JSON.stringify(report, null, 2));
} else {
  printHuman(report);
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--json") {
      out.json = true;
    } else if (arg.startsWith("--provider=")) {
      out.provider = arg.slice("--provider=".length);
    } else if (arg === "--provider") {
      out.provider = argv[++i];
    } else if (arg.startsWith("--cwd=")) {
      out.cwd = arg.slice("--cwd=".length);
    } else if (arg === "--cwd") {
      out.cwd = argv[++i];
    } else if (arg.startsWith("--timeout=")) {
      out.timeout = arg.slice("--timeout=".length);
    } else if (arg === "--timeout") {
      out.timeout = argv[++i];
    } else if (arg.startsWith("--claude-path=")) {
      out.claudePath = arg.slice("--claude-path=".length);
    } else if (arg === "--claude-path") {
      out.claudePath = argv[++i];
    } else if (arg === "--help" || arg === "-h") {
      console.log(`Usage: scripts/list-agent-runtime-skills.mjs [options]

Options:
  --provider all|codex|claude   Runtime to inspect. Default: all
  --cwd PATH                    Project cwd sent to runtimes. Default: current dir
  --timeout MS                  Per-runtime timeout. Default: 15000
  --claude-path PATH            Optional Claude executable path for SDK
  --json                        Print raw diagnostic JSON
`);
      process.exit(0);
    } else {
      fail(`Unknown argument: ${arg}`);
    }
  }
  return out;
}

async function collectCodex({ cwd, timeoutMs }) {
  const result = {
    ok: false,
    error: null,
    command: "codex app-server --stdio",
    raw: null,
    skills: [],
  };

  let child;
  try {
    child = spawn("codex", ["app-server", "--stdio"], {
      cwd,
      stdio: ["pipe", "pipe", "pipe"],
    });
  } catch (error) {
    result.error = String(error?.message || error);
    return result;
  }

  const stderr = [];
  child.stderr.setEncoding("utf8");
  child.stderr.on("data", (chunk) => stderr.push(chunk));

  const rl = readline.createInterface({ input: child.stdout });
  const pending = new Map();
  rl.on("line", (line) => {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    if (message.id && pending.has(String(message.id))) {
      pending.get(String(message.id))(message);
    }
  });

  const request = async (method, params) => {
    const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
    const message = { jsonrpc: "2.0", id, method, params };
    child.stdin.write(`${JSON.stringify(message)}\n`);
    return waitFor(id, pending, timeoutMs);
  };

  const notify = (method, params) => {
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", method, params })}\n`);
  };

  try {
    const init = await request("initialize", {
      clientInfo: { name: "choro-diagnostic", title: "Choro Diagnostic", version: "0" },
      capabilities: { experimentalApi: true },
    });
    if (init.error) {
      throw new Error(`initialize failed: ${jsonOneLine(init.error)}`);
    }
    notify("initialized", {});

    const skills = await request("skills/list", { cwd });
    if (skills.error) {
      throw new Error(`skills/list failed: ${jsonOneLine(skills.error)}`);
    }

    result.raw = skills.result;
    result.skills = normalizeCodexSkills(skills.result);
    result.ok = true;
    return result;
  } catch (error) {
    result.error = String(error?.message || error);
    if (stderr.length) {
      result.stderr = stderr.join("").trim().slice(0, 4000);
    }
    return result;
  } finally {
    rl.close();
    child.kill();
  }
}

async function collectClaude({ cwd, timeoutMs, claudePath }) {
  const result = {
    ok: false,
    error: null,
    sdkModule: null,
    slashCommands: [],
    skills: [],
    plugins: [],
    rawSystemInit: null,
  };

  const sdkPath = [
    path.join(
      repoRoot,
      "crates/ide-app/assets/agent-chat/node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs",
    ),
    path.join(
      repoRoot,
      "agent-chat/node_modules/@anthropic-ai/claude-agent-sdk/sdk.mjs",
    ),
  ].find((candidate) => existsSync(candidate));
  if (!sdkPath) {
    result.error = `Claude SDK not found under ${repoRoot}`;
    return result;
  }

  result.sdkModule = sdkPath;

  const abortController = new AbortController();
  let queryHandle = null;
  const timer = setTimeout(() => abortController.abort(), timeoutMs);

  try {
    const { query } = await import(pathToFileURL(sdkPath).href);
    queryHandle = query({
      prompt: "Say ok.",
      options: {
        cwd,
        additionalDirectories: [cwd],
        ...(claudePath ? { pathToClaudeCodeExecutable: claudePath } : {}),
        permissionMode: "bypassPermissions",
        allowDangerouslySkipPermissions: true,
        maxTurns: 1,
        abortController,
        systemPrompt: { type: "preset", preset: "claude_code" },
      },
    });

    for await (const message of queryHandle) {
      if (
        message?.type === "system" &&
        (message.subtype === "init" || message.subtype === "system_init")
      ) {
        result.rawSystemInit = message;
        result.slashCommands = (message.slash_commands || message.slashCommands || []).map(
          String,
        );
        result.skills = (message.skills || []).map(String);
        result.plugins = Array.isArray(message.plugins) ? message.plugins : [];
        result.ok = true;
        queryHandle.close?.();
        break;
      }
      if (message?.type === "result" && (message.subtype === "error" || message.is_error)) {
        throw new Error(String(message.result || message.error || "Claude SDK returned error"));
      }
    }

    if (!result.ok) {
      throw new Error("Claude SDK ended before emitting system/init.");
    }
    return result;
  } catch (error) {
    result.error =
      error?.name === "AbortError"
        ? `Timed out after ${timeoutMs}ms waiting for Claude system/init.`
        : String(error?.message || error);
    return result;
  } finally {
    clearTimeout(timer);
    queryHandle?.close?.();
  }
}

function normalizeCodexSkills(raw) {
  const entries = [];
  if (Array.isArray(raw?.skills)) {
    entries.push(...raw.skills);
  } else if (Array.isArray(raw?.data)) {
    for (const group of raw.data) {
      if (Array.isArray(group?.skills)) {
        entries.push(...group.skills.map((skill) => ({ ...skill, cwd: group.cwd })));
      }
    }
  } else if (Array.isArray(raw)) {
    entries.push(...raw);
  }

  return entries
    .filter((skill) => skill && skill.enabled !== false)
    .map((skill) => ({
      name: String(skill.name || skill.id || skill.skill || skill.slug || ""),
      title: String(
        skill.title ||
          skill.displayName ||
          skill.display_name ||
          skill.interface?.displayName ||
          skill.name ||
          "",
      ),
      description:
        skill.interface?.shortDescription || skill.description || skill.summary || undefined,
      path: skill.path,
      cwd: skill.cwd,
      invocation: skill.invocation || skill.command || `$${skill.name || ""} `,
      source: skill.scope,
    }))
    .filter((skill) => skill.name);
}

function waitFor(id, pending, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(String(id));
      reject(new Error(`Timed out waiting for JSON-RPC response ${id}`));
    }, timeoutMs);
    pending.set(String(id), (message) => {
      clearTimeout(timer);
      pending.delete(String(id));
      resolve(message);
    });
  });
}

function printHuman(report) {
  console.log(`Runtime capability diagnostic`);
  console.log(`cwd: ${report.cwd}`);
  console.log("");
  if (report.codex) {
    printSection("Codex skills", report.codex, report.codex.skills, "skills");
  }
  if (report.claude) {
    printSection("Claude slash commands", report.claude, report.claude.slashCommands, "commands");
    if (report.claude.ok) {
      console.log(`Claude skills: ${report.claude.skills.length}`);
      for (const skill of report.claude.skills.slice(0, 80)) {
        console.log(`  - ${skill}`);
      }
      if (report.claude.skills.length > 80) {
        console.log(`  ... ${report.claude.skills.length - 80} more`);
      }
      console.log("");
    }
  }
}

function printSection(title, state, items, itemLabel) {
  console.log(`${title}: ${state.ok ? "ok" : "failed"}`);
  if (!state.ok) {
    console.log(`  error: ${state.error || "unknown"}`);
    if (state.stderr) {
      console.log(`  stderr: ${state.stderr}`);
    }
    console.log("");
    return;
  }
  console.log(`  ${itemLabel}: ${items.length}`);
  for (const item of items.slice(0, 80)) {
    if (typeof item === "string") {
      console.log(`  - ${item}`);
    } else {
      console.log(`  - ${item.invocation || ""}${item.title ? ` ${item.title}` : item.name}`);
      if (item.description) {
        console.log(`    ${item.description}`);
      }
    }
  }
  if (items.length > 80) {
    console.log(`  ... ${items.length - 80} more`);
  }
  console.log("");
}

function jsonOneLine(value) {
  return JSON.stringify(value);
}

function fail(message) {
  console.error(message);
  process.exit(2);
}
