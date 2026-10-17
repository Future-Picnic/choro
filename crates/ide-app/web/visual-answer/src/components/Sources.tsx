import type { VisualSource } from "../types";

function retrieved(seconds: number): string {
  return new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** Provenance is always visible, outside the agent's program. References are
 *  plain text: the renderer never navigates or loads them. */
export function Sources({ sources }: { sources: VisualSource[] }) {
  if (!sources.length) return null;
  return (
    <footer className="va-sources" aria-label="Sources">
      {sources.map((source, index) => (
        <p key={index} className="va-source">
          <span className="va-source-label">{source.label}</span>
          {source.time_range && <span>Time range: {source.time_range}</span>}
          {source.retrieved_at != null && <span>Retrieved {retrieved(source.retrieved_at)}</span>}
          {source.reference && <span className="va-source-ref">{source.reference}</span>}
        </p>
      ))}
    </footer>
  );
}
