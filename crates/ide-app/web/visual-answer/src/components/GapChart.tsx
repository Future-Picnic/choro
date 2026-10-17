// Used only when a plotted value is missing. react-ui's charts accept numbers
// only and coerce null to zero, so missing points are drawn here as gaps.
import { useLayoutEffect, useRef, useState } from "react";
import { formatNumber, type Row } from "../data";

interface Series {
  key: string;
  label: string;
  color: string;
}

interface Props {
  kind: "line" | "bar";
  rows: readonly Row[];
  category: string;
  series: Series[];
  height: number;
}

const PAD = { top: 12, right: 12, bottom: 28, left: 44 };

/** Rounded linear ticks covering [min, max], always including zero for bars. */
export function niceTicks(min: number, max: number, count = 4): number[] {
  if (min === max) {
    min = Math.min(0, min);
    max = max === 0 ? 1 : Math.max(0, max);
  }
  const raw = (max - min) / count;
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * magnitude).find((s) => s >= raw) ?? raw;
  const start = Math.floor(min / step) * step;
  const ticks: number[] = [];
  for (let value = start; value <= max + step * 0.5; value += step) ticks.push(Number(value.toFixed(10)));
  return ticks;
}

export function GapChart({ kind, rows, category, series, height }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(560);
  useLayoutEffect(() => {
    const element = ref.current;
    if (!element) return;
    const measure = () => setWidth(Math.max(240, element.clientWidth));
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const values = rows.flatMap((row) => series.map((s) => row[s.key])).filter((v): v is number => typeof v === "number");
  const ticks = values.length
    ? niceTicks(Math.min(kind === "bar" ? 0 : Infinity, ...values), Math.max(kind === "bar" ? 0 : -Infinity, ...values))
    : [0, 1];
  const low = ticks[0];
  const high = ticks[ticks.length - 1];
  const plotW = width - PAD.left - PAD.right;
  const plotH = height - PAD.top - PAD.bottom;
  const band = plotW / Math.max(1, rows.length);
  const x = (index: number) => PAD.left + band * index + band / 2;
  const y = (value: number) => PAD.top + plotH - ((value - low) / (high - low || 1)) * plotH;
  const labelEvery = Math.max(1, Math.ceil(rows.length / Math.max(1, Math.floor(plotW / 64))));

  return (
    <div ref={ref} className="va-gap-chart">
      <svg width={width} height={height} role="presentation">
        {ticks.map((tick) => (
          <g key={tick}>
            <line className="va-grid" x1={PAD.left} x2={width - PAD.right} y1={y(tick)} y2={y(tick)} />
            <text className="va-axis" x={PAD.left - 6} y={y(tick)} textAnchor="end" dominantBaseline="middle">
              {formatNumber(tick)}
            </text>
          </g>
        ))}
        {rows.map((row, index) => {
          const missing = series.some((s) => typeof row[s.key] !== "number");
          return (
            <g key={index}>
              {missing && (
                <line className="va-missing-mark" x1={x(index)} x2={x(index)} y1={PAD.top} y2={PAD.top + plotH}>
                  <title>{`${String(row[category] ?? "")}: missing`}</title>
                </line>
              )}
              {index % labelEvery === 0 && (
                <text className={missing ? "va-axis va-axis-missing" : "va-axis"} x={x(index)} y={height - 8} textAnchor="middle">
                  {String(row[category] ?? "")}
                </text>
              )}
            </g>
          );
        })}
        {kind === "line"
          ? series.map((s) => {
              const segments: string[] = [];
              let current = "";
              rows.forEach((row, index) => {
                const value = row[s.key];
                if (typeof value === "number") current += `${current ? "L" : "M"}${x(index)},${y(value)}`;
                else if (current) {
                  segments.push(current);
                  current = "";
                }
              });
              if (current) segments.push(current);
              return (
                <g key={s.key}>
                  {segments.map((d, i) => (
                    <path key={i} d={d} fill="none" stroke={s.color} strokeWidth={2} strokeLinejoin="round" />
                  ))}
                  {rows.map((row, index) =>
                    typeof row[s.key] === "number" ? (
                      <circle key={index} cx={x(index)} cy={y(row[s.key] as number)} r={3} fill={s.color}>
                        <title>{`${s.label}, ${String(row[category] ?? "")}: ${formatNumber(row[s.key] as number)}`}</title>
                      </circle>
                    ) : null,
                  )}
                </g>
              );
            })
          : rows.map((row, index) => {
              const inner = (band * 0.7) / series.length;
              return series.map((s, i) => {
                const value = row[s.key];
                if (typeof value !== "number") return null;
                const left = x(index) - (band * 0.7) / 2 + inner * i;
                const top = Math.min(y(value), y(0));
                return (
                  <rect key={`${index}-${s.key}`} x={left} y={top} width={Math.max(1, inner - 2)} height={Math.abs(y(value) - y(0))} rx={2} fill={s.color}>
                    <title>{`${s.label}, ${String(row[category] ?? "")}: ${formatNumber(value)}`}</title>
                  </rect>
                );
              });
            })}
      </svg>
    </div>
  );
}
