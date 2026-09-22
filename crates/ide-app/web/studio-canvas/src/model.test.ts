import test from "node:test";
import assert from "node:assert/strict";
import {
  previewPlan,
  takeNextDecode,
  pixels,
  validCamera,
  IMAGE_BUDGET,
  trimPreviews,
  type Screen,
} from "./model.ts";
const screens: Screen[] = Array.from({ length: 200 }, (_, i) => ({
  id: String(i),
  name: `Screen ${i}`,
  width: i % 2 ? 390 : 1440,
  height: 960,
  archived: false,
  content_key: String(i),
}));
const positions = Object.fromEntries(
  screens.map((s, i) => [
    s.id,
    { x: (i % 4) * 1560, y: Math.floor(i / 4) * 1080 },
  ]),
);
test("culls offscreen screens and omits unreadably small images", () => {
  const r = previewPlan(
    screens,
    positions,
    { x: 0, y: 0, zoom: 1 },
    1200,
    800,
    2,
    null,
  );
  assert.deepEqual(
    r.map((r) => r.screen_id),
    ["0"],
  );
  assert.equal(
    previewPlan(
      screens,
      positions,
      { x: 0, y: 0, zoom: 0.02 },
      1200,
      800,
      2,
      null,
    ).length,
    0,
  );
});

test("movement asks for coarse images and idle restores sharp demand", () => {
  const args = [screens, positions, { x: 0, y: 0, zoom: 1 }, 1200, 800, 2, null] as const;
  assert.equal(previewPlan(...args, true)[0].tier, 512);
  assert.equal(previewPlan(...args, false)[0].tier, 2048);
});

test("recent offscreen images survive until bounded LRU pressure evicts them", () => {
  const resident = new Map(Array.from({length: 3}, (_, i) =>
    [String(i), { width: 2048, height: 2048 }] as const));
  const original = resident.get('0');
  // With available budget, leaving and returning does not discard the image.
  assert.equal(trimPreviews(resident, new Set(), IMAGE_BUDGET).length, 0);
  assert.equal(resident.get('0'), original);
  // Screen 0 is visible, so the oldest offscreen entry (1) is released first.
  assert.equal(trimPreviews(resident, new Set(['0'])).length, 1);
  assert.ok(resident.has('0')); assert.ok(!resident.has('1')); assert.ok(resident.has('2'));
  assert.ok([...resident.values()].reduce((n, p) => n + p.width * p.height * 4, 0) <= IMAGE_BUDGET / 2);
});
test("fit-all requests remain bounded, preserving mobile aspect ratios", () => {
  const r = previewPlan(
    screens,
    positions,
    { x: 0, y: 0, zoom: 0.08 },
    1800,
    5000,
    2,
    "1",
  );
  const bytes = r.reduce((n, r) => {
    const p = pixels(screens[Number(r.screen_id)], r.tier);
    return n + p.width * p.height * 4;
  }, 0);
  assert.ok(bytes <= IMAGE_BUDGET);
  assert.equal(r[0].screen_id, "1");
  assert.deepEqual(pixels(screens[1], 1024), { width: 416, height: 1024 });
});
test("hostile camera values are rejected", () => {
  for (const v of [
    { x: Infinity, y: 0, zoom: 1 },
    { x: 0, y: 0, zoom: NaN },
    { x: 0, y: 0, zoom: 0 },
    { x: 0, y: 0, zoom: 3 },
  ])
    assert.equal(validCamera(v), false);
  assert.ok(validCamera({ x: -500, y: 800, zoom: 0.02 }));
});

test("temporary memory pressure retains work and decodes downgrades first", () => {
  const resident = new Map<string, { width: number; height: number }>([
    ["198", { width: 2048, height: 2048 }],
    ["199", { width: 2048, height: 2048 }],
  ]);
  const queue = Array.from({ length: 200 }, (_, i) => ({
    screen_id: String(i),
    width: 256,
    height: 256,
  }));
  let peak = 0;
  while (queue.length) {
    const next = takeNextDecode(queue, resident, 0);
    assert.ok(next, "Every lower-resolution preview must eventually fit");
    peak = Math.max(
      peak,
      [...resident.values()].reduce((n, p) => n + p.width * p.height * 4, 0) +
        next.width * next.height * 4,
    );
    resident.set(next.screen_id, next);
  }
  assert.equal(resident.size, 200);
  assert.ok(peak <= IMAGE_BUDGET);
  const blocked = [{ screen_id: "new", width: 2048, height: 2048 }];
  assert.equal(takeNextDecode(blocked, resident, 16 * 1024 * 1024), undefined);
  assert.equal(
    blocked.length,
    1,
    "Temporary pressure must not discard the job",
  );
});
