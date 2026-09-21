export type Point = { x: number; y: number };
export type Camera = Point & { zoom: number };
export type Screen = {
  id: string;
  name: string;
  width: number;
  height: number;
  content_key: string;
  archived: boolean;
};
export type Layout = {
  schema_version: number;
  overview_mode: "canvas" | "grid";
  viewport: Camera;
  positions: Record<string, Point>;
  selected_screen_id: string | null;
};
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
        (b.screen.id === selected ? 1 : 0) - (a.screen.id === selected ? 1 : 0),
    )
    .map((c) => ({
      screen_id: c.screen.id,
      content_key: c.screen.content_key,
      tier: c.tier,
    }));
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
