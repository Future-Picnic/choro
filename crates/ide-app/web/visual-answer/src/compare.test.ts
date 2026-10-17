import { describe, expect, it } from "vitest";
import { columnsFor, planComparison, sanitizeStageScroll } from "./compare";

describe("design comparison layout", () => {
  it("keeps compact controls side by side and sized to the design, not a fixed slot", () => {
    const plan = planComparison({ count: 4, containerWidth: 900, expanded: false });
    expect(plan.scale).toBe("control");
    expect(plan.columns).toBe(4);
    expect(plan.zoom).toBe(1);
    expect(plan.visibleHeight).toBe(132);
    expect(plan.overflow).toBe(false);
  });

  it("never leaves an orphan option on its own row", () => {
    expect(columnsFor(4, 500, 168)).toBe(2);
    expect(columnsFor(3, 500, 168)).toBe(1);
    expect(columnsFor(3, 560, 168)).toBe(3);
    expect(columnsFor(2, 300, 168)).toBe(1);
  });

  it("puts two hero sections side by side when readable and in rows when not", () => {
    const wide = planComparison({ count: 2, containerWidth: 900, scale: "section", height: 640, expanded: false });
    expect(wide.columns).toBe(2);
    expect(wide.frameWidth).toBe(1024);
    expect(wide.zoom).toBeLessThan(1);
    expect(wide.canvasHeight).toBe(Math.ceil(640 * wide.zoom));
    const narrow = planComparison({ count: 2, containerWidth: 560, scale: "section", expanded: false });
    expect(narrow.columns).toBe(1);
    const three = planComparison({ count: 3, containerWidth: 800, scale: "section", expanded: false });
    expect(three.columns).toBe(1);
  });

  it("scrolls a full page inside its preview and gives it more room expanded", () => {
    const docked = planComparison({ count: 2, containerWidth: 900, scale: "page", expanded: false });
    expect(docked.overflow).toBe(true);
    expect(docked.visibleHeight).toBe(320);
    const expanded = planComparison({
      count: 2,
      containerWidth: 1400,
      scale: "page",
      expanded: true,
      viewportHeight: 1000,
    });
    expect(expanded.visibleHeight).toBe(840);
    expect(expanded.overflow).toBe(true);
  });

  it("honours an explicit layout width such as a phone screen", () => {
    const phone = planComparison({ count: 2, containerWidth: 900, scale: "page", width: 390, height: 844, expanded: false });
    expect(phone.frameWidth).toBe(390);
    expect(phone.zoom).toBe(1);
    expect(phone.visibleHeight).toBe(320);
  });

  it("keeps saved preview scroll only for this revision's options and sane values", () => {
    const saved = { a: 120.4, b: -3, c: Number.NaN, gone: 50, d: "12", e: 1e9 };
    expect(sanitizeStageScroll(saved, ["a", "b", "c", "d", "e"])).toEqual({ a: 120, e: 6000 });
    expect(sanitizeStageScroll([1, 2], ["0"])).toEqual({});
    expect(sanitizeStageScroll(null, ["a"])).toEqual({});
  });

  it("lays out before the first measurement without collapsing", () => {
    const plan = planComparison({ count: 4, containerWidth: 0, expanded: false });
    expect(plan.columns).toBe(2);
    expect(plan.frameWidth).toBeGreaterThan(80);
  });
});
