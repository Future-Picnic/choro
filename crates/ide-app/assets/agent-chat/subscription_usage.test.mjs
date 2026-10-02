import test from "node:test";
import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { PassThrough, Writable } from "node:stream";
import { claudeAllowance, codexAllowance, readCodex } from "./subscription_usage.mjs";

test("Claude uses percentages, preserves zero, and omits absent windows", () => {
  const result = claudeAllowance({ subscription_type: "max", rate_limits_available: true, rate_limits: {
    five_hour: { utilization: 16, resets_at: "2026-10-01T12:00:00Z" },
    seven_day: { utilization: 0, resets_at: null }, seven_day_opus: null,
  } });
  assert.equal(result.windows[0].used_percent, 16);
  assert.equal(result.windows[0].resets_at, 1790856000);
  assert.equal(result.windows[1].used_percent, 0);
  assert.equal(result.windows[1].resets_at, null);
  assert.equal(result.windows.length, 2);
});

test("Claude includes the provider's model-specific limits without duplicating legacy fields", () => {
  const result = claudeAllowance({ rate_limits_available: true, rate_limits: {
    seven_day: { utilization: 2 },
    limits: [{ kind: "weekly_all", percent: 2 }, { kind: "weekly_scoped", percent: 0, scope: { model: { display_name: "Fable" } } }],
  } });
  assert.deepEqual(result.windows.map((w) => w.label), ["Weekly · all models", "Weekly · Fable"]);
});

test("Codex handles multiple buckets, real durations, absent limits and reset credits", () => {
  const result = codexAllowance({ rateLimits: { primary: { usedPercent: 90 } }, rateLimitsByLimitId: {
    codex: { limitId: "codex", planType: "pro", primary: { usedPercent: 84, windowDurationMins: 10080, resetsAt: 1790856000 }, secondary: null },
    review: { limitName: "Review", primary: { usedPercent: 0, windowDurationMins: 300 } },
  }, rateLimitResetCredits: { availableCount: 2 } });
  assert.equal(result.windows.length, 2);
  assert.equal(result.windows[0].label, "codex · Weekly");
  assert.equal(result.windows[1].label, "Review · 5 hours");
  assert.equal(result.reset_credits, 2);
  assert.equal(result.plan, "pro");
});

test("missing or invalid data never becomes a full allowance", () => {
  assert.equal(claudeAllowance({ rate_limits_available: false }).status, "no_subscription");
  assert.equal(codexAllowance({}).status, "unavailable");
  assert.deepEqual(codexAllowance({ rateLimits: { primary: { usedPercent: null } } }).windows, []);
});

function codexProcess(email, failedMethod) {
  const sent = [];
  const child = new EventEmitter();
  child.stdout = new PassThrough();
  child.kill = () => { child.stdout.end(); child.emit("exit"); };
  child.stdin = new Writable({ write(chunk, _, done) {
    const message = JSON.parse(chunk.toString());
    sent.push(message.method);
    const result = message.method === "account/read" ? { account: { type: "chatgpt", email, token: "private-token" } }
      : message.method === "account/rateLimits/read" ? { rateLimits: { primary: { usedPercent: 25, windowDurationMins: 300 } } }
      : {};
    if (message.id) child.stdout.write(`${JSON.stringify(message.method === failedMethod
      ? { id: message.id, error: { message: "Unavailable" } } : { id: message.id, result })}\n`);
    done();
  } });
  return { child, sent };
}

test("Codex account reads never create a thread, send a prompt, or expose account credentials", async () => {
  const { child, sent } = codexProcess("private@example.com");
  const result = await readCodex("unused", () => child);
  assert.deepEqual(sent, ["initialize", "initialized", "account/read", "account/rateLimits/read"]);
  assert.equal(result.windows[0].used_percent, 25);
  assert.ok(!JSON.stringify(result).includes("private"));
  assert.match(result.account_fingerprint, /^[a-f0-9]{64}$/);
});

test("a failed quota read still identifies its account, while a failed auth read cannot reuse a cache", async () => {
  const first = await readCodex("unused", () => codexProcess("account-a@example.com").child);
  const same = await readCodex("unused", () => codexProcess("account-a@example.com", "account/rateLimits/read").child);
  const changed = await readCodex("unused", () => codexProcess("account-b@example.com", "account/rateLimits/read").child);
  const unknown = await readCodex("unused", () => codexProcess("account-a@example.com", "account/read").child);
  assert.equal(same.status, "unavailable");
  assert.equal(same.account_fingerprint, first.account_fingerprint);
  assert.notEqual(changed.account_fingerprint, first.account_fingerprint);
  assert.equal(unknown.account_fingerprint, null);
});
