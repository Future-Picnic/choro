// Strict program validation. A program renders only when the OpenUI parser,
// built from the same library, reports no problems and every dataset and
// column reference resolves against this document's data.
import { createParser, type ElementNode, type ParseResult } from "@openuidev/react-lang";
import { library, ROOT_COMPONENT } from "./library";
import { DESIGN_HEIGHT_RANGE, DESIGN_WIDTH_RANGE } from "./compare";
import type { DatasetReader } from "./data";
import type { VisualDocument } from "./types";

const parser = createParser(library.toJSONSchema(), ROOT_COMPONENT);
const MAX_STATEMENTS = 200;
// Same limits as `ide_core::visual_answers` for declared series.
export const MAX_CHART_SERIES = 8;
export const MAX_CHART_POINTS = 500;

export interface Validation {
  ok: boolean;
  errors: string[];
  result: ParseResult | null;
}

function isElement(value: unknown): value is ElementNode {
  return !!value && typeof value === "object" && (value as ElementNode).type === "element";
}

/** Every component node in program order. */
export function elements(value: unknown, found: ElementNode[] = []): ElementNode[] {
  if (Array.isArray(value)) value.forEach((item) => elements(item, found));
  else if (isElement(value)) {
    found.push(value);
    Object.values(value.props ?? {}).forEach((prop) => elements(prop, found));
  }
  return found;
}

function checkReferences(node: ElementNode, reader: DatasetReader, document: VisualDocument, errors: string[]) {
  const props = node.props as Record<string, unknown>;
  const where = node.statementId ? `${node.typeName} “${node.statementId}”` : node.typeName;
  if (node.typeName === "DesignOptions") {
    if (document.options.length < 2) errors.push(`${where} needs two to four alternatives in document.options.`);
    const inRange = (value: unknown, range: { min: number; max: number }) =>
      value == null || (typeof value === "number" && Number.isFinite(value) && value >= range.min && value <= range.max);
    if (!inRange(props.height, DESIGN_HEIGHT_RANGE))
      errors.push(`${where} height must be ${DESIGN_HEIGHT_RANGE.min}–${DESIGN_HEIGHT_RANGE.max} CSS pixels.`);
    if (!inRange(props.width, DESIGN_WIDTH_RANGE))
      errors.push(`${where} width must be ${DESIGN_WIDTH_RANGE.min}–${DESIGN_WIDTH_RANGE.max} CSS pixels.`);
    return;
  }
  if (!("dataset" in props)) return;
  const name = String(props.dataset ?? "");
  const dataset = reader.get(name);
  if (!dataset) {
    errors.push(`${where} uses dataset “${name}”, which is not in document.datasets.`);
    return;
  }
  const columns = new Map(dataset.columns.map((c) => [c.key, c]));
  const requireColumn = (key: unknown, label: string, numeric = false) => {
    const column = columns.get(String(key));
    if (!column) errors.push(`${where} ${label} “${String(key)}” is not a column of “${name}”.`);
    else if (numeric && column.kind !== "number") errors.push(`${where} ${label} “${String(key)}” must be a number column.`);
  };
  const list = (value: unknown) => (Array.isArray(value) ? value : []);
  switch (node.typeName) {
    case "DataTable":
      list(props.columns).forEach((key) => requireColumn(key, "column"));
      if (props.sortBy != null) requireColumn(props.sortBy, "sortBy");
      break;
    case "TrendChart":
    case "CategoryChart": {
      requireColumn(node.typeName === "TrendChart" ? props.x : props.category, node.typeName === "TrendChart" ? "x" : "category");
      // Limits apply to the series the chart actually draws, whether named in
      // the program or declared on the dataset (tables and summaries can
      // chart datasets that declare no series).
      const series = list(props.series).length ? list(props.series) : (dataset.series ?? []);
      if (series.length === 0) errors.push(`${where} needs numeric series, either in the program or in the dataset.`);
      const repeated = series.filter((key, index) => series.indexOf(key) !== index);
      if (repeated.length)
        errors.push(`${where} lists series “${String(repeated[0])}” more than once; name each series once.`);
      if (series.length > MAX_CHART_SERIES)
        errors.push(`${where} draws ${series.length} series; use at most ${MAX_CHART_SERIES}, or split it into several charts.`);
      if (dataset.rows.length > MAX_CHART_POINTS)
        errors.push(
          `${where} would plot ${dataset.rows.length} points per series; use at most ${MAX_CHART_POINTS} by aggregating the data (for example by week) or charting a smaller range.`,
        );
      series.forEach((key) => requireColumn(key, "series", true));
      break;
    }
    case "Metric":
      requireColumn(props.column, "column", true);
      break;
  }
}

export function validateProgram(source: string, reader: DatasetReader, document: VisualDocument): Validation {
  const errors: string[] = [];
  let result: ParseResult | null = null;
  try {
    result = parser.parse(source);
  } catch (error) {
    return { ok: false, errors: [`The program could not be parsed: ${String(error)}`], result: null };
  }
  const { meta } = result;
  if (meta.incomplete) errors.push("The program is incomplete.");
  if (meta.statementCount > MAX_STATEMENTS) errors.push(`Use at most ${MAX_STATEMENTS} statements.`);
  for (const error of meta.errors) if (error.severity !== "warning") errors.push(error.message);
  if (meta.orphaned.length) errors.push(`Statements not used by root: ${meta.orphaned.join(", ")}.`);
  if (meta.unresolved.length) errors.push(`Undefined references: ${meta.unresolved.join(", ")}.`);
  if (result.queryStatements.length || result.mutationStatements.length)
    errors.push("Query and Mutation are not available in Choro visual answers.");
  if (Object.keys(result.stateDeclarations ?? {}).length) errors.push("$variables are not available in Choro visual answers.");
  if (!result.root || result.root.typeName !== ROOT_COMPONENT) errors.push(`The root must be ${ROOT_COMPONENT}(...).`);
  for (const node of elements(result.root)) {
    if (!library.components[node.typeName]) errors.push(`Unknown component ${node.typeName}.`);
    else checkReferences(node, reader, document, errors);
  }
  return { ok: errors.length === 0, errors: [...new Set(errors)], result };
}
