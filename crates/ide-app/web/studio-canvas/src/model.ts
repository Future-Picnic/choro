export type Point = { x: number; y: number };
export type Camera = Point & { zoom: number };
export type Activity = "working" | "editing" | "queued";
export type Screen = {
  id: string;
  name: string;
  width: number;
  height: number;
  content_key: string;
  archived: boolean;
  /** Set while a live agent turn still owes this screen its design. */
  activity?: Activity | null;
};
export type Layout = {
  schema_version: number;
  overview_mode: "canvas" | "grid" | "focus";
  viewport: Camera;
  positions: Record<string, Point>;
  selected_screen_id: string | null;
  selected_section_id?: string | null;
};
/** Host-authored section metadata and authoritative geometry (canvas units). */
export type Section = {
  id: string;
  name: string;
  direction: "horizontal" | "vertical";
  gap: number;
  title_style: "left_title" | "full_width_header";
  header_alignment: "left" | "center";
  screen_ids: string[];
  active_screen_ids: string[];
  x: number;
  y: number;
  width: number;
  height: number;
  header_height: number;
};
export type Arrangement = "stacked" | "side_by_side";
export type Rect = { x: number; y: number; width: number; height: number };
export type Request = { screen_id: string; content_key: string; tier: number };
export const IMAGE_BUDGET = 64 * 1024 * 1024;
export const TIERS = [256, 512, 1024, 2048];
export function pixels(screen: Screen, tier: number) {
  const scale = tier / Math.max(screen.width, screen.height);
  return {
    width: Math.max(1, Math.round(screen.width * scale)),
    height: Math.max(1, Math.round(screen.height * scale)),
  };
}
export function previewPlan(
  screens: Screen[],
  positions: Record<string, Point>,
  camera: Camera,
  width: number,
  height: number,
  dpr: number,
  selected: string | null,
  moving = false,
): Request[] {
  const margin = 160,
    z = camera.zoom;
  const candidates = screens
    .filter((s) => !s.archived)
    .filter((s) => {
      const p = positions[s.id];
      if (!p) return false;
      const x = p.x * z + camera.x,
        y = p.y * z + camera.y;
      return (
        x + s.width * z >= -margin &&
        x <= width + margin &&
        y + s.height * z >= -margin &&
        y <= height + margin &&
        Math.max(s.width, s.height) * z >= 48
      );
    })
    .map((s) => ({
      screen: s,
      tier:
        TIERS.find(
          (t) => t >= Math.max(s.width, s.height) * z * Math.min(dpr, 2),
        ) ?? 2048,
    }));
  if (moving) for (const candidate of candidates) candidate.tier = Math.min(candidate.tier, 512);
  const total = () =>
    candidates.reduce((sum, c) => {
      const p = pixels(c.screen, c.tier);
      return sum + p.width * p.height * 4;
    }, 0);
  // Reserve headroom for two in-flight decodes alongside retained images.
  while (total() > IMAGE_BUDGET - 2 * 2048 * 2048 * 4) {
    const c = candidates
      .filter((c) => c.tier > 256)
      .sort(
        (a, b) =>
          (a.screen.id === selected ? 1 : 0) -
            (b.screen.id === selected ? 1 : 0) || b.tier - a.tier,
      )[0];
    if (!c) break;
    c.tier /= 2;
  }
  return candidates
    .sort(
      (a, b) =>
        (b.screen.id === selected ? 1 : 0) - (a.screen.id === selected ? 1 : 0) ||
        distance(a.screen) - distance(b.screen),
    )
    .map((c) => ({
      screen_id: c.screen.id,
      content_key: c.screen.content_key,
      tier: c.tier,
    }));
  function distance(s: Screen) {
    const p = positions[s.id];
    return Math.hypot((p.x + s.width / 2) * z + camera.x - width / 2,
      (p.y + s.height / 2) * z + camera.y - height / 2);
  }
}

/** Map insertion order is LRU. Keep recent offscreen decodes, but reserve space
 * for two decode jobs and release the least recently viewed images under pressure. */
export function trimPreviews<T extends { width: number; height: number }>(
  resident: Map<string, T>, visible: ReadonlySet<string>,
  limit = IMAGE_BUDGET / 2,
): T[] {
  let used = [...resident.values()].reduce((sum, p) => sum + p.width * p.height * 4, 0);
  const removed: T[] = [];
  for (const [id, p] of resident) {
    if (used <= limit) break;
    if (visible.has(id)) continue;
    resident.delete(id); removed.push(p); used -= p.width * p.height * 4;
  }
  return removed;
}
export function validCamera(v: Camera) {
  return (
    Number.isFinite(v.x) &&
    Number.isFinite(v.y) &&
    Math.abs(v.x) <= 1e7 &&
    Math.abs(v.y) <= 1e7 &&
    Number.isFinite(v.zoom) &&
    v.zoom >= 0.02 &&
    v.zoom <= 2
  );
}

/** Section titles stay readable independent of artboard scale: ~16–30 display px. */
export const LABEL_MIN_PX = 16;
export const LABEL_MAX_PX = 30;
const LINE = 1.35;
/** Mirrors the Rust board constants. */
const SECTION_SPACING = 160;
const CAPTION = 44;
export function labelPixels(zoom: number) {
  return Math.max(LABEL_MIN_PX, Math.min(LABEL_MAX_PX, 40 * Math.sqrt(Math.max(zoom, 0))));
}
export type LabelPlan = {
  /** Display pixels; divide by zoom for canvas units. */
  size: number;
  mode: "full" | "short" | "hidden";
  /** Character limit for a shortened title. */
  chars?: number;
  /** Room for the title in canvas units; longer titles truncate with an ellipsis. */
  maxWidth: number;
};
function overlaps(a: Rect, b: Rect) {
  return a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y;
}
/** Space between a title and its section surface, in display pixels. */
export const TITLE_GAP_PX = 6;
/** A title sits at the bottom of its reserved band, just above the surface,
 * and grows upward below the minimum label zoom. Stacked sections may use the
 * board's width for a title; side-by-side titles stop short of the next one.
 * At extreme zoom, collisions resolve in priority order: the selected
 * section keeps its full title; others shorten, then hide. */
export function sectionLabels(
  sections: Section[],
  zoom: number,
  selected: string | null | undefined,
  arrangement: Arrangement = "stacked",
): Record<string, LabelPlan> {
  const size = labelPixels(zoom);
  const board = Math.max(0, ...sections.map((s) => s.width));
  const room = (s: Section) => (arrangement === "stacked" ? Math.max(s.width, board) : s.width + SECTION_SPACING * 0.75);
  // Normal zoom: titles fit their reserved bands, so collision testing is unnecessary.
  if (sections.every((s) => (size * LINE + TITLE_GAP_PX) / zoom <= s.header_height))
    return Object.fromEntries(sections.map((s) => [s.id, { size, mode: "full", maxWidth: room(s) }]));
  const order = [...sections].sort((a, b) => (b.id === selected ? 1 : 0) - (a.id === selected ? 1 : 0));
  const placed: Rect[] = [];
  const plans: Record<string, LabelPlan> = {};
  const rect = (s: Section, px: number, chars: number) => {
    const height = (px * LINE) / zoom;
    const width = Math.min(room(s), (chars * px * 0.6 + 32) / zoom);
    const bottom = s.y + s.header_height - TITLE_GAP_PX / zoom;
    return { x: s.x, y: bottom - height, width, height };
  };
  for (const section of order) {
    const others = sections.filter((s) => s.id !== section.id);
    const free = (r: Rect) => !placed.some((p) => overlaps(p, r)) && !others.some((s) => overlaps(s, r));
    const maxWidth = room(section);
    const full = rect(section, size, section.name.length);
    if (section.id === selected || free(full)) {
      plans[section.id] = { size, mode: "full", maxWidth };
      placed.push(full);
      continue;
    }
    // Shorten: the largest size that fits the band and the spacing above it.
    const fit = Math.min(size, ((section.header_height + SECTION_SPACING) * zoom - TITLE_GAP_PX) / LINE - 1);
    const chars = Math.min(section.name.length, 14);
    const short = rect(section, fit, chars);
    if (fit >= 9 && free(short)) {
      plans[section.id] = { size: fit, mode: "short", chars, maxWidth };
      placed.push(short);
    } else plans[section.id] = { size, mode: "hidden", maxWidth };
  }
  return plans;
}
export function shortTitle(name: string, plan: LabelPlan) {
  return plan.mode === "short" && plan.chars && name.length > plan.chars ? name.slice(0, plan.chars - 1) + "…" : name;
}
/** Bounds of every section, including title bands. */
export function boardBounds(sections: Section[], zoom?: number): Rect | null {
  if (!sections.length) return null;
  const x = Math.min(...sections.map((s) => s.x));
  const y = Math.min(...sections.map((s) => zoom ? Math.min(s.y, s.y + s.header_height - (labelPixels(zoom) * LINE + TITLE_GAP_PX) / zoom) : s.y));
  return {
    x,
    y,
    width: Math.max(...sections.map((s) => s.x + s.width)) - x,
    height: Math.max(...sections.map((s) => s.y + s.height)) - y,
  };
}
export type DropTarget = {
  section_id: string | null;
  before_screen_id: string | null;
  /** Insertion marker in canvas units, or the whole content area when empty. */
  marker?: Rect;
};
/** Mirrors the host's insertion rule: the first other member whose midpoint
 * lies after the pointer along the section direction. */
export function dropTarget(
  sections: Section[],
  screens: Screen[],
  positions: Record<string, Point>,
  dragged: string,
  point: Point,
  size: ReadonlyMap<string, Screen> = new Map(screens.map((s) => [s.id, s])),
): DropTarget {
  const section = sections.find(
    (s) => point.x >= s.x && point.y >= s.y && point.x <= s.x + s.width && point.y <= s.y + s.height,
  );
  if (!section) return { section_id: null, before_screen_id: null };
  const horizontal = section.direction === "horizontal";
  const members = section.active_screen_ids.filter((id) => id !== dragged && positions[id] && size.get(id));
  const contentTop = section.y + section.header_height;
  if (!members.length)
    return {
      section_id: section.id,
      before_screen_id: null,
      marker: { x: section.x, y: contentTop, width: section.width, height: section.height - section.header_height },
    };
  const before =
    members.find((id) => {
      const p = positions[id], s = size.get(id)!;
      return horizontal ? point.x < p.x + s.width / 2 : point.y < p.y + s.height / 2;
    }) ?? null;
  const thickness = 8;
  let marker: Rect;
  if (horizontal) {
    const anchor = before ?? members[members.length - 1];
    const p = positions[anchor], s = size.get(anchor)!;
    const x = before ? p.x - section.gap / 2 : p.x + s.width + section.gap / 2;
    marker = { x: x - thickness / 2, y: contentTop, width: thickness, height: section.y + section.height - contentTop };
  } else {
    const anchor = before ?? members[members.length - 1];
    const p = positions[anchor], s = size.get(anchor)!;
    // Each screen has a caption band above it; mark the middle of the gap.
    const y = before ? p.y - CAPTION - section.gap / 2 : p.y + s.height + section.gap / 2;
    marker = { x: section.x, y: y - thickness / 2, width: section.width, height: thickness };
  }
  return { section_id: section.id, before_screen_id: before, marker };
}
/** True when dropping would leave the screen exactly where it already is. */
export function sameSlot(sections: Section[], dragged: string, target: DropTarget) {
  const current = sections.find((s) => s.screen_ids.includes(dragged));
  if (!current || current.id !== target.section_id) return false;
  const active = current.active_screen_ids;
  const index = active.indexOf(dragged);
  const next = index >= 0 && index + 1 < active.length ? active[index + 1] : null;
  return target.before_screen_id === next;
}

/** Choose a decode that fits now; retain blocked work and release large images first. */
export function takeNextDecode<
  T extends { screen_id: string; width: number; height: number },
>(
  queue: T[],
  resident: Map<string, { width: number; height: number }>,
  inFlightBytes: number,
): T | undefined {
  const bytes = (p: { width: number; height: number }) =>
    p.width * p.height * 4;
  const release = (p: T) =>
    bytes(resident.get(p.screen_id) ?? { width: 0, height: 0 }) - bytes(p);
  queue.sort((a, b) => release(b) - release(a));
  const used = [...resident.values()].reduce(
    (sum, p) => sum + bytes(p),
    inFlightBytes,
  );
  const index = queue.findIndex((p) => used + bytes(p) <= IMAGE_BUDGET);
  return index < 0 ? undefined : queue.splice(index, 1)[0];
}
