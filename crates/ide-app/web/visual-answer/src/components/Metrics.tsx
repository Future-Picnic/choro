import type { ReactNode } from "react";
import { aggregate, formatNumber, lastChange, type Aggregate } from "../data";
import { useAnswer } from "../context";
import { Problem } from "./Problem";

const AGGREGATE_LABEL: Record<Aggregate, string> = {
  last: "Latest",
  first: "First",
  sum: "Total",
  mean: "Average",
  min: "Lowest",
  max: "Highest",
};

export function MetricGroupView({ children }: { children: ReactNode }) {
  return <div className="va-metrics">{children}</div>;
}

export interface MetricProps {
  label: string;
  dataset: string;
  column: string;
  aggregate?: Aggregate | null;
  compare?: boolean | null;
}

/** Values are computed from the answer's data, so a summary cannot drift from it. */
export function MetricView({ label, dataset: name, column, aggregate: how, compare }: MetricProps) {
  const answer = useAnswer();
  const dataset = answer.reader.get(name);
  const definition = dataset?.columns.find((c) => c.key === column);
  if (!dataset || !definition) return <Problem text={`“${label}” refers to data that is not in this answer.`} />;
  const method = how ?? "last";
  const value = aggregate(dataset.rows, column, method);
  const change = compare ? lastChange(dataset.rows, column) : null;
  const unit = definition.unit ? ` ${definition.unit}` : "";
  const direction = change ? (change.delta > 0 ? "up" : change.delta < 0 ? "down" : "flat") : null;
  return (
    <div className="va-metric">
      <span className="va-metric-label">{label}</span>
      <span className="va-metric-value">
        {value === null ? (
          <span className="va-missing">
            <span aria-hidden="true">—</span> <span className="va-metric-missing">Missing</span>
          </span>
        ) : (
          <>
            {formatNumber(value)}
            {unit && <span className="va-metric-unit">{unit}</span>}
          </>
        )}
      </span>
      {change && (
        <span className={`va-change va-change-${direction}`}>
          {direction === "up" ? "Up" : direction === "down" ? "Down" : "No change"}
          {direction !== "flat" && ` ${formatNumber(Math.abs(change.delta))}${unit}`}
          {change.percent !== null && direction !== "flat" && ` (${formatNumber(Math.abs(change.percent))}%)`} from the previous row
        </span>
      )}
      {compare && !change && <span className="va-change">No earlier value to compare</span>}
      <span className="va-metric-basis">
        {AGGREGATE_LABEL[method]} across {dataset.rows.length} {dataset.rows.length === 1 ? "row" : "rows"}
      </span>
    </div>
  );
}
