// Mirrors `ide_core::visual_answers`. The host has already validated the
// transport contract; the renderer still treats every field as untrusted.

export type ColumnKind = "text" | "number" | "boolean" | "date";
export type Cell = string | number | boolean | null;

export interface VisualColumn {
  key: string;
  label: string;
  kind: ColumnKind;
  unit?: string | null;
}

export interface VisualDataset {
  columns: VisualColumn[];
  rows: Record<string, Cell>[];
  series?: string[];
}

export interface VisualSource {
  label: string;
  reference?: string | null;
  retrieved_at?: number | null;
  time_range?: string | null;
}

export interface VisualOption {
  id: string;
  title: string;
  description: string;
  html: string;
}

export type VisualActionKind = "select" | "refine" | "follow_up";

export interface VisualAction {
  id: string;
  label: string;
  kind: VisualActionKind;
}

export type VisualKind = "table" | "line_chart" | "bar_chart" | "summary" | "design_options";

export interface VisualDocument {
  title: string;
  summary: string;
  kind: VisualKind;
  openui_lang: string;
  datasets: Record<string, VisualDataset>;
  sources: VisualSource[];
  options: VisualOption[];
  actions: VisualAction[];
}

/** Choro-owned view state per component, keyed by a stable component key. */
export interface ComponentView {
  search?: string;
  sortKey?: string | null;
  sortDirection?: "ascending" | "descending";
  hiddenSeries?: string[];
  showData?: boolean;
}

export interface SavedFields {
  openui?: Record<string, unknown>;
  view?: Record<string, ComponentView>;
  /** Readable description of the current view, sent with follow-ups. */
  summary?: string;
  /** Scroll inside each design preview, by option id, in design pixels. */
  stages?: Record<string, number>;
}

export interface SavedState {
  selectedOptionId: string | null;
  fields: SavedFields;
  scrollTop: number;
}

export interface VisualTheme {
  dark: boolean;
  base: string;
  surface: string;
  surface2: string;
  line: string;
  line2: string;
  t1: string;
  t2: string;
  t3: string;
  accent: string;
  accentSoft: string;
  onAccent: string;
  focus: string;
  hover: string;
  sage: string;
  amber: string;
  rose: string;
  sky: string;
  teal: string;
}

export type SurfaceMode = "docked" | "expanded";

export interface Bootstrap {
  /** Host-issued generation; every message echoes it with the answer identity. */
  surface: number;
  answerId: string;
  revision: number;
  document: VisualDocument;
  state: SavedState;
  theme: VisualTheme;
  mode: SurfaceMode;
  readOnly: boolean;
}

export type HostMessage =
  | { type: "ready" }
  | { type: "state"; state: SavedState }
  /** Immediate reply to a host snapshot request (handoff or send). */
  | { type: "snapshot"; token: number; state: SavedState }
  | { type: "invalid"; errors: string[] }
  /** Escape pressed in the content. Choro decides what it does (stop work,
   *  dock, leave the answer) with the same rules as the native composer. */
  | { type: "escape" }
  /** Laid-out height of the content, so a docked panel fits it without blank space. */
  | { type: "size"; height: number }
  /** Keyboard focus leaves the content toward the native panel chrome. */
  | { type: "focusExit"; direction: "forward" | "backward" };
