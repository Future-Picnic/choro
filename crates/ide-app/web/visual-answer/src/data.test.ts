import { describe, expect, it } from "vitest";
import { aggregate, createDatasetReader, filterRows, formatCell, lastChange, missingPoints, sortRows } from "./data";
import { projectsDocument } from "./test-fixtures";

const dataset = projectsDocument.datasets.projects;
const issues = dataset.columns[3];

describe("dataset helpers", () => {
  it("never turns a missing value into zero", () => {
    expect(formatCell(null, "number")).toEqual({ text: "—", missing: true });
    expect(formatCell(0, "number")).toEqual({ text: "0", missing: false });
    expect(aggregate(dataset.rows, "open_issues", "sum")).toBe(50);
    expect(aggregate(dataset.rows, "open_issues", "min")).toBe(0);
    expect(aggregate([{ v: null }], "v", "sum")).toBeNull();
    expect(aggregate([{ v: 1 }, { v: null }], "v", "last")).toBeNull();
  });

  it("sorts missing values last in both directions", () => {
    const down = sortRows(dataset.rows, issues, "descending").map((r) => r.name);
    const up = sortRows(dataset.rows, issues, "ascending").map((r) => r.name);
    expect(down).toEqual(["Cinder", "Atlas", "Ember", "Beacon", "Fjord", "Delta"]);
    expect(up).toEqual(["Fjord", "Beacon", "Ember", "Atlas", "Cinder", "Delta"]);
  });

  it("searches displayed values, including the missing marker", () => {
    expect(filterRows(dataset.rows, dataset.columns, "maya").map((r) => r.name)).toEqual(["Atlas", "Cinder"]);
    expect(filterRows(dataset.rows, dataset.columns, "missing").map((r) => r.name)).toEqual(["Delta"]);
    expect(filterRows(dataset.rows, dataset.columns, "  ")).toHaveLength(6);
  });

  it("only reports a change when both rows have values", () => {
    expect(lastChange([{ v: 10 }, { v: 15 }], "v")).toEqual({ delta: 5, percent: 50 });
    expect(lastChange([{ v: null }, { v: 15 }], "v")).toBeNull();
    expect(lastChange([{ v: 0 }, { v: 4 }], "v")).toEqual({ delta: 4, percent: null });
    expect(missingPoints([{ d: "Tue", v: null }], "d", ["v"])).toEqual(["Tue"]);
  });

  it("exposes data read-only", () => {
    const reader = createDatasetReader(projectsDocument.datasets);
    const rows = reader.get("projects")!.rows;
    expect(() => {
      (rows as unknown as unknown[]).push({});
    }).toThrow();
    expect(() => {
      (rows[0] as Record<string, unknown>).name = "Changed";
    }).toThrow();
    expect(reader.get("absent")).toBeUndefined();
  });
});
