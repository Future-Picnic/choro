import { BarChart, LineChart } from "@openuidev/react-ui/Charts";
import { columnHeading, completeChartRows, hasMissing, missingPoints } from "../data";
import { useAnswer } from "../context";
import type { VisualColumn } from "../types";
import { DataTableView } from "./DataTable";
import { GapChart } from "./GapChart";
import { Problem } from "./Problem";

export interface SeriesChartProps {
  kind: "line" | "bar";
  dataset: string;
  category: string;
  series?: string[] | null;
  title?: string | null;
  horizontal?: boolean | null;
  viewKey: string;
}

/** Unique, readable series names; react-ui shows data keys in its tooltips. */
function seriesLabels(columns: VisualColumn[]): Map<string, string> {
  const seen = new Map<string, number>();
  const labels = new Map<string, string>();
  for (const column of columns) {
    const count = (seen.get(column.label) ?? 0) + 1;
    seen.set(column.label, count);
    labels.set(column.key, count === 1 ? column.label : `${column.label} ${count}`);
  }
  return labels;
}

export function SeriesChart({ kind, dataset: name, category, series, title, horizontal, viewKey }: SeriesChartProps) {
  const answer = useAnswer();
  const dataset = answer.reader.get(name);
  if (!dataset) return <Problem text={`This answer does not include the “${name}” data.`} />;
  const categoryColumn = dataset.columns.find((c) => c.key === category);
  const keys = (series?.length ? series : (dataset.series ?? [])).filter((key) =>
    dataset.columns.some((c) => c.key === key && c.kind === "number"),
  );
  if (!categoryColumn || keys.length === 0) return <Problem text="This chart is missing its category or numeric series." />;

  const seriesColumns = keys.map((key) => dataset.columns.find((c) => c.key === key)!) as VisualColumn[];
  const labels = seriesLabels(seriesColumns);
  const colorOf = (key: string) => answer.palette[keys.indexOf(key) % answer.palette.length];
  const view = answer.view(viewKey);
  const hidden = new Set(view.hiddenSeries ?? []);
  const visible = keys.filter((key) => !hidden.has(key));
  const rows = dataset.rows;
  const units = [...new Set(seriesColumns.filter((c) => visible.includes(c.key)).map((c) => c.unit ?? ""))];
  const unit = units.length === 1 && units[0] ? units[0] : null;
  const showData = view.showData ?? answer.expanded;
  const missing = missingPoints(rows, category, visible);
  const complete = !hasMissing(rows, visible);
  const longLabels = rows.some((row) => String(row[category] ?? "").length > 14);
  const sideways = kind === "bar" && (horizontal ?? (longLabels || rows.length > 8));
  const chartName = `${title ?? labels.get(keys[0])} by ${categoryColumn.label}`;

  const toggle = (key: string) => {
    const next = new Set(hidden);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    answer.setView(viewKey, { hiddenSeries: keys.filter((k) => next.has(k)) });
  };

  const renamed = (data: Record<string, string | number>[]) =>
    data.map((row) => {
      const out: Record<string, string | number> = { [categoryColumn.label]: row[category] };
      for (const key of visible) out[labels.get(key)!] = row[key];
      return out;
    });

  return (
    <figure className="va-chart">
      {(title || unit) && (
        <figcaption className="va-chart-head">
          {title && <span className="va-chart-title">{title}</span>}
          {unit && <span className="va-chart-unit">Values in {unit}</span>}
        </figcaption>
      )}
      {keys.length > 1 && (
        <div className="va-legend" role="group" aria-label="Series">
          {keys.map((key) => (
            <button
              key={key}
              type="button"
              className="va-series"
              aria-pressed={!hidden.has(key)}
              onClick={() => toggle(key)}
            >
              <span className="va-swatch" style={{ background: colorOf(key) }} aria-hidden="true" />
              {labels.get(key)}
            </button>
          ))}
        </div>
      )}
      <div className="va-plot" role="group" aria-label={`${kind === "line" ? "Line" : "Bar"} chart of ${chartName}. Show data lists exact values.`}>
        {rows.length === 0 ? (
          <p className="va-empty">There are no data points to chart.</p>
        ) : visible.length === 0 ? (
          <p className="va-empty">All series are hidden. Select a series above to show it.</p>
        ) : complete ? (
          kind === "line" ? (
            <LineChart
              data={renamed(completeChartRows(rows, category, visible))}
              categoryKey={categoryColumn.label}
              customPalette={visible.map(colorOf)}
              legend={false}
              condensed
              showDots={rows.length <= 40}
              dotRadius={2.5}
              crosshair
              height={answer.expanded ? 280 : 200}
              tickVariant="singleLine"
            />
          ) : (
            <BarChart
              data={renamed(completeChartRows(rows, category, visible))}
              categoryKey={categoryColumn.label}
              customPalette={visible.map(colorOf)}
              legend={false}
              condensed
              orientation={sideways ? "horizontal" : "vertical"}
              internalLine={false}
              barRadius={3}
              maxBarWidth={44}
              height={sideways ? Math.max(160, rows.length * 34) : answer.expanded ? 280 : 200}
            />
          )
        ) : (
          <GapChart
            kind={kind}
            rows={rows}
            category={category}
            series={visible.map((key) => ({ key, label: labels.get(key)!, color: colorOf(key) }))}
            height={answer.expanded ? 280 : 200}
          />
        )}
      </div>
      {missing.length > 0 && visible.length > 0 && (
        <p className="va-note">
          Missing for {missing.join(", ")}. {kind === "line" ? "The line breaks there" : "No bar is drawn"} instead of showing zero.
        </p>
      )}
      <button
        type="button"
        className="va-disclosure"
        aria-expanded={showData}
        onClick={() => answer.setView(viewKey, { showData: !showData })}
      >
        {showData ? "Hide data" : "Show data"}
      </button>
      {showData && (
        <DataTableView
          dataset={name}
          columns={[category, ...keys]}
          searchable={rows.length > 8}
          caption={`Data for ${chartName} (${columnHeading(categoryColumn)})`}
          viewKey={`${viewKey}:data`}
        />
      )}
    </figure>
  );
}
