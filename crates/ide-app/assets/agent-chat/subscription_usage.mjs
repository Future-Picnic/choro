// Read account allowances through the installed agents. Never send a prompt,
// start a turn, inspect credentials, or print raw provider responses.
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { homedir } from "node:os";
import { pathToFileURL } from "node:url";

function accountFingerprint(account) {
  if (!account?.email) return null;
  // Keep identifying fields private; only this opaque, in-memory cache key is
  // passed to the app. Include the account/workspace when the provider reports it.
  return createHash("sha256").update(JSON.stringify([
    account.email, account.organization ?? account.accountId ?? null,
    account.apiProvider ?? account.type ?? null,
  ])).digest("hex");
}

function windowValue(label, used, reset) {
  if (typeof used !== "number" || !Number.isFinite(used)) return null;
  const resetsAt = typeof reset === "number" ? reset : Date.parse(reset) / 1000;
  return {
    label,
    used_percent: Math.min(100, Math.max(0, used)),
    resets_at: Number.isFinite(resetsAt) && resetsAt > 0 ? Math.floor(resetsAt) : null,
  };
}

export function claudeAllowance(usage) {
  if (!usage?.rate_limits_available || !usage?.rate_limits) {
    return { status: usage?.subscription_type ? "unavailable" : "no_subscription", windows: [] };
  }
  const rate = usage.rate_limits;
  let windows;
  // Newer CLIs expose the display-ready list, including model-specific limits.
  // Use it instead of the legacy fields to avoid duplicate weekly windows.
  if (Array.isArray(rate.limits) && rate.limits.length) {
    windows = rate.limits.map((limit) => {
      const model = limit.scope?.model?.display_name;
      const label = limit.kind === "session" ? "Session · 5 hours"
        : limit.kind === "weekly_all" ? "Weekly · all models"
        : limit.kind === "weekly_scoped" && model ? `Weekly · ${model}`
        : limit.kind === "weekly_scoped" ? "Weekly · scoped allowance"
        : String(limit.kind || "Allowance").replaceAll("_", " ");
      return windowValue(label, limit.percent, limit.resets_at);
    }).filter(Boolean);
  } else {
    windows = Object.entries(rate).flatMap(([key, value]) => {
      if (!value || typeof value !== "object" || key === "extra_usage") return [];
      const label = key === "five_hour" ? "Session · 5 hours"
        : key === "seven_day" ? "Weekly · all models"
        : key.startsWith("seven_day_") ? `Weekly · ${key.slice(10).replaceAll("_", " ")}`
        : key.replaceAll("_", " ");
      const window = windowValue(label, value.utilization, value.resets_at);
      return window ? [window] : [];
    });
  }
  return { status: windows.length ? "ready" : "unavailable", plan: usage.subscription_type, windows };
}

function durationLabel(minutes) {
  if (minutes === 10080) return "Weekly";
  if (typeof minutes !== "number" || !Number.isFinite(minutes) || minutes <= 0) return "Allowance";
  if (minutes % 1440 === 0) return `${minutes / 1440} days`;
  if (minutes % 60 === 0) return `${minutes / 60} hours`;
  return `${minutes} minutes`;
}

export function codexAllowance(usage) {
  const buckets = usage?.rateLimitsByLimitId
    ? Object.values(usage.rateLimitsByLimitId)
    : usage?.rateLimits ? [usage.rateLimits] : [];
  const windows = buckets.flatMap((bucket) => [bucket.primary, bucket.secondary].flatMap((limit) => {
    if (!limit) return [];
    const prefix = buckets.length > 1 ? `${bucket.limitName || bucket.limitId || "Codex"} · ` : "";
    const window = windowValue(`${prefix}${durationLabel(limit.windowDurationMins)}`, limit.usedPercent, limit.resetsAt);
    return window ? [window] : [];
  }));
  const count = usage?.rateLimitResetCredits?.availableCount;
  return {
    status: windows.length ? "ready" : "unavailable",
    plan: buckets.find((bucket) => bucket.planType)?.planType ?? null,
    windows,
    reset_credits: Number.isInteger(count) && count >= 0 ? count : null,
  };
}

async function readClaude(cli) {
  const { query } = await import("@anthropic-ai/claude-agent-sdk");
  let finish;
  const pending = new Promise((resolve) => { finish = resolve; });
  const runtime = query({
    prompt: (async function* () { await pending; })(),
    options: {
      cwd: homedir(), pathToClaudeCodeExecutable: cli,
      tools: [], mcpServers: {}, strictMcpConfig: true, settingSources: [],
    },
  });
  const timeout = setTimeout(() => { runtime.close(); finish(); }, 20000);
  let account_fingerprint = null;
  try {
    await runtime.initializationResult();
    if (typeof runtime.accountInfo === "function") {
      account_fingerprint = accountFingerprint(await runtime.accountInfo());
    }
    const usage = runtime.usage_EXPERIMENTAL_MAY_CHANGE_DO_NOT_RELY_ON_THIS_API_YET;
    if (typeof usage !== "function") return { status: "update_required", windows: [] };
    return { ...claudeAllowance(await usage.call(runtime)), account_fingerprint };
  } catch {
    return { status: "unavailable", windows: [], account_fingerprint };
  } finally {
    clearTimeout(timeout);
    runtime.close();
    finish();
  }
}

export async function readCodex(cli, spawnProvider = spawn) {
  const child = spawnProvider(cli, ["app-server", "--stdio"], { cwd: homedir(), stdio: ["pipe", "pipe", "ignore"] });
  const { createInterface } = await import("node:readline");
  const lines = createInterface({ input: child.stdout });
  let nextId = 0;
  const pending = new Map();
  const fail = () => {
    for (const { reject } of pending.values()) reject(new Error("Account query unavailable"));
    pending.clear();
  };
  child.on("error", fail);
  child.on("exit", fail);
  child.stdin.on("error", fail);
  lines.on("line", (line) => {
    let message;
    try { message = JSON.parse(line); } catch { return; }
    const request = pending.get(message.id);
    if (!request) return;
    pending.delete(message.id);
    if (message.error) request.reject(new Error("Account query unavailable"));
    else request.resolve(message.result);
  });
  const send = (message) => child.stdin.write(`${JSON.stringify(message)}\n`);
  const request = (method, params = {}) => new Promise((resolve, reject) => {
    const id = ++nextId;
    pending.set(id, { resolve, reject });
    send({ id, method, params });
  });
  const timeout = setTimeout(() => { fail(); child.kill(); }, 20000);
  let account_fingerprint = null;
  try {
    await request("initialize", { clientInfo: { name: "choro_usage", title: "Choro", version: "0.1.0" }, capabilities: { experimentalApi: true } });
    send({ method: "initialized", params: {} });
    const account = await request("account/read");
    if (!account?.account) return { status: "signed_out", windows: [] };
    if (account.account.type === "apiKey") return { status: "no_subscription", windows: [] };
    account_fingerprint = accountFingerprint(account.account);
    return { ...codexAllowance(await request("account/rateLimits/read")), account_fingerprint };
  } catch {
    return { status: "unavailable", windows: [], account_fingerprint };
  } finally {
    clearTimeout(timeout);
    lines.close();
    child.stdin.destroy();
    child.kill();
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const [provider, cli] = process.argv.slice(2);
    const result = provider === "claude" ? await readClaude(cli) : await readCodex(cli);
    process.stdout.write(`${JSON.stringify(result)}\n`);
    process.exit(0);
  } catch {
    process.stdout.write(`${JSON.stringify({ status: "unavailable", windows: [] })}\n`);
    process.exit(0);
  }
}
