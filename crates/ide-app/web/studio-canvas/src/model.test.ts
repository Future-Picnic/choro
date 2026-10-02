import test from "node:test";
import assert from "node:assert/strict";
import {
  previewPlan,
  takeNextDecode,
  pixels,
  validCamera,
  IMAGE_BUDGET,
  trimPreviews,
  labelPixels,
  sectionLabels,
  shortTitle,
  dropTarget,
  sameSlot,
  boardBounds,
  LABEL_MIN_PX,
  LABEL_MAX_PX,
  TITLE_GAP_PX,
  type Screen,
  type Section,
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

const section = (id: string, y: number, members: string[], extra: Partial<Section> = {}): Section => ({
  id,
  name: `Flow ${id}`,
  direction: "horizontal",
  gap: 96,
  title_style: "left_title",
  header_alignment: "left",
  screen_ids: members,
  active_screen_ids: members,
  x: 0,
  y,
  width: 3000,
  height: 1200,
  header_height: 128,
  ...extra,
});
test("fit bounds contain display-sized titles at the minimum and extreme zoom", () => {
  const s = section("flow", 0, [], { header_height: 192 });
  const min = (16 * 1.35 + TITLE_GAP_PX) / s.header_height;
  assert.ok(s.header_height - (labelPixels(min) * 1.35 + TITLE_GAP_PX) / min >= -1e-9);
  for (const zoom of [min, .1, .02]) {
    const bounds = boardBounds([s], zoom)!;
    const titleTop = s.y + s.header_height - (labelPixels(zoom) * 1.35 + TITLE_GAP_PX) / zoom;
    assert.ok(bounds.y <= titleTop);
    assert.equal(bounds.y + bounds.height, s.y + s.height);
  }
});
test("section titles stay 16–30 display px independent of artboard zoom", () => {
  assert.equal(labelPixels(0.02), LABEL_MIN_PX);
  assert.equal(labelPixels(2), LABEL_MAX_PX);
  assert.ok(labelPixels(0.25) > LABEL_MIN_PX && labelPixels(0.25) < LABEL_MAX_PX);
  const sections = [section("a", 0, []), section("b", 1360, [])];
  const normal = sectionLabels(sections, 0.25, null);
  assert.deepEqual(Object.values(normal).map((p) => p.mode), ["full", "full"]);
});
test("extreme zoom keeps the selected title and shortens or hides others", () => {
  const sections = [section("a", 0, []), section("b", 1360, []), section("c", 2720, [])];
  const plans = sectionLabels(sections, 0.03, "b");
  assert.equal(plans.b.mode, "full");
  // The first section has open canvas above it; the last would collide.
  assert.equal(plans.a.mode, "full");
  assert.notEqual(plans.c.mode, "full");
  const unselected = sectionLabels(sections, 0.03, null);
  assert.equal(unselected.a.mode, "full");
  assert.notEqual(unselected.b.mode, "full");
  for (const plan of Object.values(plans)) if (plan.mode === "short") assert.ok(plan.size < labelPixels(0.03));
  const long = { ...sections[0], name: "A very long flow title that does not fit" };
  assert.equal(shortTitle(long.name, { size: 10, mode: "short", chars: 14, maxWidth: 3000 }).length, 14);
  assert.equal(shortTitle(long.name, { size: 10, mode: "full", maxWidth: 3000 }), long.name);
  // Stacked titles may use the board width; side-by-side ones stop before the next section.
  const narrow = [section("a", 0, [], { width: 480 }), section("b", 1360, [])];
  assert.equal(sectionLabels(narrow, 0.25, null).a.maxWidth, 3000);
  const beside = [section("a", 0, [], { width: 480 }), section("b", 0, [], { x: 640 })];
  assert.ok(sectionLabels(beside, 0.25, null, "side_by_side").a.maxWidth < 640);
});
test("drop targets mirror host insertion and detect unchanged slots", () => {
  const screens: Screen[] = ["1", "2", "3"].map((id) => ({ id, name: id, width: 390, height: 844, archived: false, content_key: id }));
  const positions = { "1": { x: 48, y: 220 }, "2": { x: 534, y: 220 }, "3": { x: 1020, y: 220 } };
  const flows = [section("a", 0, ["1", "2", "3"]), section("empty", 1400, [], { width: 480, height: 408 })];
  assert.deepEqual(dropTarget(flows, screens, positions, "3", { x: 600, y: 300 }).before_screen_id, "2");
  assert.equal(dropTarget(flows, screens, positions, "1", { x: 2000, y: 300 }).before_screen_id, null);
  const empty = dropTarget(flows, screens, positions, "1", { x: 100, y: 1500 });
  assert.equal(empty.section_id, "empty");
  assert.equal(empty.marker?.width, 480);
  assert.equal(dropTarget(flows, screens, positions, "1", { x: -500, y: -500 }).section_id, null);
  assert.ok(sameSlot(flows, "2", { section_id: "a", before_screen_id: "3" }));
  assert.ok(sameSlot(flows, "3", { section_id: "a", before_screen_id: null }));
  assert.ok(!sameSlot(flows, "3", { section_id: "a", before_screen_id: "1" }));
  assert.ok(!sameSlot(flows, "1", { section_id: "empty", before_screen_id: null }));
  const vertical = [section("v", 0, ["1", "2"], { direction: "vertical" })];
  const stacked = { "1": { x: 48, y: 220 }, "2": { x: 48, y: 1204 } };
  const target = dropTarget(vertical, screens, stacked, "3", { x: 100, y: 1100 });
  assert.equal(target.before_screen_id, "2");
  assert.ok(target.marker && target.marker.width === 3000);
  assert.deepEqual(boardBounds(flows), { x: 0, y: 0, width: 3000, height: 1808 });
  assert.equal(boardBounds([]), null);
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
