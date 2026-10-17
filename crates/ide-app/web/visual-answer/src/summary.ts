// A short, readable description of the user's current local view. The host
// stores it with the saved state and includes it in follow-up messages, so the
// agent knows what the user was looking at without reading opaque view state.
import { filterRows, type Row } from "./data";
import type { DatasetReader } from "./data";
import type { ElementNode } from "@openuidev/react-lang";
import type { ComponentView, VisualDocument } from "./types";
import { viewKey } from "./context";

const MAX_SUMMARY = 1800;

export interface ViewTarget {
  key: string;
  typeName: string;
  dataset: string;
  columns?: string[];
}

/** Components with local view state, in program order, with their view keys. */
export function viewTargets(nodes: ElementNode[]): ViewTarget[] {
  const targets: ViewTarget[] = [];
  for (const node of nodes) {
    const props = node.props as Record<string, unknown>;
    if (!["DataTable", "TrendChart", "CategoryChart"].includes(node.typeName)) continue;
    const dataset = String(props.dataset ?? "");
    const columns = Array.isArray(props.columns) ? props.columns.map(String) : undefined;
    targets.push({ key: viewKey(node.statementId, node.typeName, dataset), typeName: node.typeName, dataset, columns });
  }
  return targets;
}

function quote(text: string): string {
  return `“${text.replace(/\s+/g, " ").trim().slice(0, 80)}”`;
}

function describeTable(view: ComponentView, target: ViewTarget, reader: DatasetReader, label: string): string | null {
  const dataset = reader.get(target.dataset);
  if (!dataset) return null;
  const columns = target.columns?.length
    ? dataset.columns.filter((c) => target.columns!.includes(c.key))
    : dataset.columns;
  const parts: string[] = [];
  const search = view.search?.trim();
  if (search) {
    const shown = filterRows(dataset.rows as Row[], columns, search).length;
    parts.push(`search ${quote(search)} (${shown} of ${dataset.rows.length} rows shown)`);
  }
  if (view.sortKey) {
    const column = dataset.columns.find((c) => c.key === view.sortKey);
    if (column) parts.push(`sorted by ${column.label} ${view.sortDirection ?? "ascending"}`);
  }
  return parts.length ? `${label} “${target.dataset}”: ${parts.join("; ")}` : null;
}

function describeChart(view: ComponentView, target: ViewTarget, reader: DatasetReader): string | null {
  const dataset = reader.get(target.dataset);
  if (!dataset) return null;
  const parts: string[] = [];
  const hidden = (view.hiddenSeries ?? [])
    .map((key) => dataset.columns.find((c) => c.key === key)?.label)
    .filter((label): label is string => !!label);
  if (hidden.length) parts.push(`hidden series ${hidden.join(", ")}`);
  return parts.length ? `${target.typeName === "TrendChart" ? "Line chart" : "Bar chart"} “${target.dataset}”: ${parts.join("; ")}` : null;
}

export function summarizeView(
  document: VisualDocument,
  reader: DatasetReader,
  targets: ViewTarget[],
  views: Record<string, ComponentView>,
  selectedOptionId: string | null,
): string {
  const lines: string[] = [];
  const selected = document.options.find((o) => o.id === selectedOptionId);
  if (selected) lines.push(`Selected design: ${selected.title}`);
  for (const target of targets) {
    const view = views[target.key];
    if (target.typeName === "DataTable") {
      if (view) {
        const line = describeTable(view, target, reader, "Table");
        if (line) lines.push(line);
      }
    } else {
      if (view) {
        const line = describeChart(view, target, reader);
        if (line) lines.push(line);
      }
      const data = views[`${target.key}:data`];
      if (data) {
        const line = describeTable(data, { ...target, columns: undefined }, reader, "Chart data");
        if (line) lines.push(line);
      }
    }
  }
  const summary = lines.join(". ");
  return summary.length > MAX_SUMMARY ? `${summary.slice(0, MAX_SUMMARY - 1)}…` : summary;
}

/** Drops restored view state that no longer applies to this document. */
export function sanitizeViews(
  views: Record<string, ComponentView> | undefined,
  reader: DatasetReader,
  targets: ViewTarget[],
): Record<string, ComponentView> {
  const out: Record<string, ComponentView> = {};
  if (!views || typeof views !== "object") return out;
  for (const target of targets) {
    const dataset = reader.get(target.dataset);
    if (!dataset) continue;
    const keys = new Set(dataset.columns.map((c) => c.key));
    for (const key of [target.key, `${target.key}:data`]) {
      const view = views[key];
      if (!view || typeof view !== "object") continue;
      const clean: ComponentView = {};
      if (typeof view.search === "string") clean.search = view.search.slice(0, 200);
      if (view.sortKey === null || (typeof view.sortKey === "string" && keys.has(view.sortKey))) clean.sortKey = view.sortKey;
      if (view.sortDirection === "ascending" || view.sortDirection === "descending") clean.sortDirection = view.sortDirection;
      if (Array.isArray(view.hiddenSeries)) clean.hiddenSeries = view.hiddenSeries.filter((k) => keys.has(k));
      if (typeof view.showData === "boolean") clean.showData = view.showData;
      out[key] = clean;
    }
  }
  return out;
}
