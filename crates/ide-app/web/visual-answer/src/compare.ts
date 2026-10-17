// Layout for design comparisons. The agent states what is being compared
// (a control, a page section or a whole page) and, optionally, the design's
// natural size; the renderer never measures inside the sandboxed frames.

export type DesignScale = "control" | "section" | "page";

export const DESIGN_SCALES: readonly DesignScale[] = ["control", "section", "page"];
export const DESIGN_HEIGHT_RANGE = { min: 60, max: 6000 } as const;
export const DESIGN_WIDTH_RANGE = { min: 320, max: 2560 } as const;

interface ScaleRule {
  /** Natural viewport width the design is laid out at; null fits the column. */
  viewport: number | null;
  /** Default natural height when the agent gives none. */
  height: number;
  /** Narrowest readable column before options move to rows. */
  minColumn: number;
  /** Tallest visible preview before it scrolls, docked and expanded. */
  docked: number;
  expanded: number;
}

const RULES: Record<DesignScale, ScaleRule> = {
  control: { viewport: null, height: 132, minColumn: 168, docked: 240, expanded: 360 },
  section: { viewport: 1024, height: 560, minColumn: 300, docked: 300, expanded: 560 },
  page: { viewport: 1280, height: 2000, minColumn: 300, docked: 320, expanded: 720 },
};

/** Column counts that never leave an orphan option on its own row. */
const COLUMN_CHOICES: Record<number, number[]> = { 2: [2, 1], 3: [3, 1], 4: [4, 2, 1] };

export const OPTION_GAP = 10;
/** Horizontal inset of a preview inside its option card (margins and border). */
export const STAGE_INSET = 14;

export interface ComparisonInput {
  count: number;
  /** Laid-out width of the options container; 0 before the first layout. */
  containerWidth: number;
  scale?: DesignScale;
  height?: number;
  width?: number;
  expanded: boolean;
  /** The answer's own visible height; only used expanded (docked height follows content). */
  viewportHeight?: number;
}

export interface ComparisonPlan {
  scale: DesignScale;
  columns: number;
  /** Width and height the design frame is laid out at. */
  frameWidth: number;
  frameHeight: number;
  /** CSS scale applied to the frame so its natural width fits the column. */
  zoom: number;
  /** Height of the scaled design. */
  canvasHeight: number;
  /** Height shown before the preview scrolls. */
  visibleHeight: number;
  overflow: boolean;
}

const FALLBACK_WIDTH = 640;

/** Saved preview scroll per option, in the design's own (unscaled) pixels so
 *  a position survives expand/dock and other zoom changes. */
export type StageScroll = Record<string, number>;

/** Keep only finite, non-negative positions for options in this revision. */
export function sanitizeStageScroll(value: unknown, optionIds: readonly string[]): StageScroll {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const known = new Set(optionIds);
  const clean: StageScroll = {};
  for (const [id, top] of Object.entries(value as Record<string, unknown>)) {
    if (known.has(id) && typeof top === "number" && Number.isFinite(top) && top > 0) {
      clean[id] = Math.min(Math.round(top), DESIGN_HEIGHT_RANGE.max);
    }
  }
  return clean;
}

export function columnsFor(count: number, containerWidth: number, minColumn: number): number {
  const width = containerWidth > 0 ? containerWidth : FALLBACK_WIDTH;
  const choices = COLUMN_CHOICES[count] ?? [1];
  return choices.find((c) => (width - OPTION_GAP * (c - 1)) / c >= minColumn) ?? 1;
}

export function planComparison(input: ComparisonInput): ComparisonPlan {
  const scale = input.scale ?? "control";
  const rule = RULES[scale];
  const columns = columnsFor(input.count, input.containerWidth, rule.minColumn);
  const width = input.containerWidth > 0 ? input.containerWidth : FALLBACK_WIDTH;
  const column = (width - OPTION_GAP * (columns - 1)) / columns;
  const inner = Math.max(80, Math.floor(column - STAGE_INSET));
  const viewport = input.width ?? rule.viewport;
  const frameWidth = viewport ?? inner;
  const frameHeight = input.height ?? rule.height;
  const zoom = viewport ? Math.min(1, inner / viewport) : 1;
  const canvasHeight = Math.ceil(frameHeight * zoom);
  let cap = input.expanded ? rule.expanded : rule.docked;
  if (input.expanded && input.viewportHeight && input.viewportHeight > 0) {
    // Expanded, a page may use most of the answer's height, never less than the rule.
    cap = Math.max(cap, scale === "page" ? Math.floor(input.viewportHeight - 160) : cap);
  }
  const visibleHeight = Math.min(canvasHeight, cap);
  return {
    scale,
    columns,
    frameWidth,
    frameHeight,
    zoom,
    canvasHeight,
    visibleHeight,
    overflow: canvasHeight > visibleHeight + 1,
  };
}
