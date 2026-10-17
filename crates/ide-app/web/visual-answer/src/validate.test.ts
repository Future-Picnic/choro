import { describe, expect, it } from "vitest";
import { createDatasetReader } from "./data";
import { designDocument, missingDocument, projectsDocument } from "./test-fixtures";
import type { VisualDocument } from "./types";
import { validateProgram } from "./validate";

function check(document: VisualDocument, program = document.openui_lang) {
  return validateProgram(program, createDatasetReader(document.datasets), document);
}

describe("program validation", () => {
  it("accepts every first-release example shape", () => {
    expect(check(projectsDocument).errors).toEqual([]);
    expect(check(missingDocument).errors).toEqual([]);
    expect(check(designDocument).errors).toEqual([]);
    const summary = `root = Answer([metrics, table])
metrics = MetricGroup([total])
total = Metric("Open issues", "projects", "open_issues", "sum")
table = DataTable("projects")`;
    expect(check(projectsDocument, summary).ok).toBe(true);
    const bars = `root = Answer([section])
section = Section("Issues", [bars])
bars = CategoryChart("projects", "name", ["open_issues"], "Open issues", true)`;
    expect(check(projectsDocument, bars).ok).toBe(true);
  });

  it("rejects components outside the library", () => {
    const result = check(projectsDocument, `root = Answer([x])\nx = Script("alert(1)")`);
    expect(result.ok).toBe(false);
  });

  it("rejects Query, Mutation and reactive state", () => {
    expect(check(projectsDocument, `root = Answer([t])\nt = DataTable("projects")\nq = Query("fetch_rows", {})`).ok).toBe(false);
    expect(check(projectsDocument, `$open = true\nroot = Answer([t])\nt = DataTable("projects")`).ok).toBe(false);
  });

  it("rejects references to absent data and wrong column types", () => {
    expect(check(projectsDocument, `root = Answer([t])\nt = DataTable("secrets")`).errors.join(" ")).toMatch(/not in document.datasets/);
    expect(check(projectsDocument, `root = Answer([t])\nt = DataTable("projects", ["salary"])`).errors.join(" ")).toMatch(/salary/);
    expect(check(projectsDocument, `root = Answer([c])\nc = CategoryChart("projects", "name", ["owner"])`).errors.join(" ")).toMatch(/number column/);
    expect(check(projectsDocument, `root = Answer([c])\nc = TrendChart("projects", "name")`).errors.join(" ")).toMatch(/numeric series/);
  });

  it("enforces chart limits on the series a chart actually draws", () => {
    // A table-kind document whose dataset declares no series can still chart:
    // explicit series must respect the same limits.
    const wide = structuredClone(projectsDocument);
    const keys = Array.from({ length: 9 }, (_, i) => `m${i}`);
    wide.datasets.projects.columns.push(...keys.map((key) => ({ key, label: key.toUpperCase(), kind: "number" as const })));
    wide.datasets.projects.rows = wide.datasets.projects.rows.map((row) => ({
      ...row,
      ...Object.fromEntries(keys.map((key, i) => [key, i])),
    }));
    const list = (n: number) => keys.slice(0, n).map((k) => `"${k}"`).join(", ");
    expect(check(wide, `root = Answer([c])\nc = TrendChart("projects", "name", [${list(8)}])`).ok).toBe(true);
    const nine = check(wide, `root = Answer([c])\nc = TrendChart("projects", "name", [${list(9)}])`);
    expect(nine.ok).toBe(false);
    expect(nine.errors.join(" ")).toMatch(/9 series; use at most 8/);

    const duplicate = check(projectsDocument, `root = Answer([c])\nc = CategoryChart("projects", "name", ["open_issues", "open_issues"])`);
    expect(duplicate.ok).toBe(false);
    expect(duplicate.errors.join(" ")).toMatch(/more than once/);

    const long = structuredClone(projectsDocument);
    long.datasets.projects.rows = Array.from({ length: 501 }, (_, i) => ({ name: `P${i}`, owner: "Maya", status: "Active", open_issues: i }));
    const points = check(long, `root = Answer([c])\nc = TrendChart("projects", "name", ["open_issues"])`);
    expect(points.ok).toBe(false);
    expect(points.errors.join(" ")).toMatch(/501 points per series; use at most 500/);
    // The same rows remain valid as a table.
    expect(check(long, `root = Answer([t])\nt = DataTable("projects")`).ok).toBe(true);
  });

  it("rejects malformed, rootless, unreachable and design-less programs", () => {
    expect(check(projectsDocument, `root = Answer([t`).ok).toBe(false);
    expect(check(projectsDocument, `t = DataTable("projects")`).ok).toBe(false);
    expect(check(projectsDocument, `root = DataTable("projects")`).ok).toBe(false);
    expect(check(projectsDocument, `root = Answer([t])\nt = DataTable("projects")\nextra = Note("unused")`).ok).toBe(false);
    expect(check(projectsDocument, `root = Answer([d])\nd = DesignOptions()`).ok).toBe(false);
  });
});
