// Pure, read-only dataset helpers. Missing values stay `null` end to end:
// they sort last, are excluded from aggregates, and are never shown as zero.
import type { Cell, ColumnKind, VisualColumn, VisualDataset } from "./types";

export type SortDirection = "ascending" | "descending";
export type Aggregate = "last" | "first" | "sum" | "mean" | "min" | "max";
export type Row = Readonly<Record<string, Cell>>;

const MISSING = "Missing";

/** The only data access available to OpenUI programs: a frozen local lookup. */
export function createDatasetReader(datasets: Record<string, VisualDataset>) {
  const frozen = new Map<string, VisualDataset>();
  for (const [name, dataset] of Object.entries(datasets ?? {})) {
    frozen.set(
      name,
      Object.freeze({
        columns: Object.freeze(dataset.columns.map((c) => Object.freeze({ ...c }))) as VisualColumn[],
        rows: Object.freeze(dataset.rows.map((r) => Object.freeze({ ...r }))) as Record<string, Cell>[],
        series: Object.freeze([...(dataset.series ?? [])]) as string[],
      }),
    );
  }
  return {
    names: () => [...frozen.keys()],
    get: (name: string): VisualDataset | undefined => frozen.get(name),
  };
}
export type DatasetReader = ReturnType<typeof createDatasetReader>;

export function columnHeading(column: VisualColumn): string {
  return column.unit ? `${column.label} (${column.unit})` : column.label;
}

const numberFormat = new Intl.NumberFormat(undefined, { maximumFractionDigits: 2 });

export function formatNumber(value: number): string {
  return numberFormat.format(value);
}

/** Display text for a cell; `missing` lets callers add an accessible marker. */
export function formatCell(value: Cell | undefined, kind: ColumnKind): { text: string; missing: boolean } {
  if (value === null || value === undefined) return { text: "—", missing: true };
  if (kind === "number" && typeof value === "number") return { text: formatNumber(value), missing: false };
  if (kind === "boolean" && typeof value === "boolean") return { text: value ? "Yes" : "No", missing: false };
  return { text: String(value), missing: false };
}

export function missingLabel(): string {
  return MISSING;
}

export function compareCells(a: Cell | undefined, b: Cell | undefined, kind: ColumnKind): number {
  const aMissing = a === null || a === undefined;
  const bMissing = b === null || b === undefined;
  if (aMissing || bMissing) return aMissing === bMissing ? 0 : aMissing ? 1 : -1;
  if (kind === "number") return Number(a) - Number(b);
  if (kind === "boolean") return Number(a) - Number(b);
  return String(a).localeCompare(String(b), undefined, { numeric: true, sensitivity: "base" });
}

/** Stable sort; missing values remain last in both directions. */
export function sortRows(rows: readonly Row[], column: VisualColumn | undefined, direction: SortDirection): Row[] {
  const indexed = rows.map((row, index) => ({ row, index }));
  if (!column) return indexed.map((e) => e.row);
  const sign = direction === "ascending" ? 1 : -1;
  indexed.sort((left, right) => {
    const a = left.row[column.key];
    const b = right.row[column.key];
    const aMissing = a === null || a === undefined;
    const bMissing = b === null || b === undefined;
    if (aMissing || bMissing) {
      if (aMissing === bMissing) return left.index - right.index;
      return aMissing ? 1 : -1;
    }
    return sign * compareCells(a, b, column.kind) || left.index - right.index;
  });
  return indexed.map((e) => e.row);
}

export function filterRows(rows: readonly Row[], columns: readonly VisualColumn[], query: string): Row[] {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return [...rows];
  return rows.filter((row) =>
    columns.some((column) => {
      const { text, missing } = formatCell(row[column.key], column.kind);
      return (missing ? MISSING : text).toLocaleLowerCase().includes(needle);
    }),
  );
}

function numericValues(rows: readonly Row[], key: string): number[] {
  return rows.map((row) => row[key]).filter((v): v is number => typeof v === "number" && Number.isFinite(v));
}

/** Aggregates ignore missing values; an aggregate of nothing is `null`. */
export function aggregate(rows: readonly Row[], key: string, how: Aggregate): number | null {
  if (how === "last" || how === "first") {
    const row = how === "last" ? rows[rows.length - 1] : rows[0];
    const value = row?.[key];
    return typeof value === "number" ? value : null;
  }
  const values = numericValues(rows, key);
  if (values.length === 0) return null;
  switch (how) {
    case "sum":
      return values.reduce((a, b) => a + b, 0);
    case "mean":
      return values.reduce((a, b) => a + b, 0) / values.length;
    case "min":
      return Math.min(...values);
    case "max":
      return Math.max(...values);
  }
}

/** Change from the previous row to the last row, when both exist. */
export function lastChange(rows: readonly Row[], key: string): { delta: number; percent: number | null } | null {
  if (rows.length < 2) return null;
  const last = rows[rows.length - 1][key];
  const previous = rows[rows.length - 2][key];
  if (typeof last !== "number" || typeof previous !== "number") return null;
  return { delta: last - previous, percent: previous === 0 ? null : ((last - previous) / Math.abs(previous)) * 100 };
}

export function hasMissing(rows: readonly Row[], keys: readonly string[]): boolean {
  return rows.some((row) => keys.some((key) => row[key] === null || row[key] === undefined));
}

export function missingPoints(rows: readonly Row[], categoryKey: string, keys: readonly string[]): string[] {
  const points: string[] = [];
  for (const row of rows) {
    for (const key of keys) {
      if (row[key] === null || row[key] === undefined) points.push(`${String(row[categoryKey] ?? "")}`);
    }
  }
  return [...new Set(points)];
}

/** Rows for react-ui charts, which only accept complete numeric data. */
export function completeChartRows(rows: readonly Row[], categoryKey: string, keys: readonly string[]) {
  return rows.map((row) => {
    const out: Record<string, string | number> = { [categoryKey]: String(row[categoryKey] ?? "") };
    for (const key of keys) out[key] = row[key] as number;
    return out;
  });
}
