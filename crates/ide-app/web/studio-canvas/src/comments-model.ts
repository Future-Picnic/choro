export type CommentPin = {
  id: string; screen_id: string; x: number; y: number;
  body: string; resolved: boolean; created_at: number;
};
export type CommentState = { schema_version: number; revision: number; pins: CommentPin[] };
export type CommentDraft = { id: string; screen_id: string; x: number; y: number };
export const COMMENT_EVENT = "choro-canvas-comment";
export const MAX_COMMENT_BYTES = 8000;
export function commentBodyValid(body: string) {
  return body.trim().length > 0 && new TextEncoder().encode(body).byteLength <= MAX_COMMENT_BYTES;
}
/** A damaged timestamp must not prevent reading or resolving the saved note. */
export function commentDate(seconds: number): Date | null {
  if (!Number.isFinite(seconds) || seconds < 0) return null;
  const date = new Date(seconds * 1000);
  return Number.isFinite(date.getTime()) ? date : null;
}
/** Screen-local fractions keep pins independent of layout, zoom, and resizing. */
export function commentAnchor(point: { x: number; y: number }, screen: { x: number; y: number; width: number; height: number }) {
  if (![point.x, point.y, screen.x, screen.y, screen.width, screen.height].every(Number.isFinite)
      || screen.width <= 0 || screen.height <= 0) return null;
  const x = (point.x - screen.x) / screen.width, y = (point.y - screen.y) / screen.height;
  return x >= 0 && x <= 1 && y >= 0 && y <= 1 ? { x, y } : null;
}
export function pinPosition(pin: Pick<CommentPin, "x" | "y">, screen: { x: number; y: number; width: number; height: number }, camera: { x: number; y: number; zoom: number }) {
  return { x: camera.x + (screen.x + pin.x * screen.width) * camera.zoom,
    y: camera.y + (screen.y + pin.y * screen.height) * camera.zoom };
}

/** Keep the note beside its pin, flipping left and clamping at the stage edges. */
export function commentPopoverPosition(anchor: { x: number; y: number }, size: { width: number; height: number }, area: { width: number; height: number }) {
  const right = anchor.x + 44;
  const fitsRight = right + size.width <= area.width - 12;
  const left = anchor.x - size.width - 12;
  const fitsBeside = fitsRight || left >= 12;
  const x = fitsRight ? right : left;
  const y = fitsBeside ? anchor.y - 32 : anchor.y + 12 + size.height <= area.height - 12
    ? anchor.y + 12 : anchor.y - 44 - size.height;
  return {
    x: Math.max(12, Math.min(x, area.width - size.width - 12)),
    y: Math.max(12, Math.min(y, area.height - size.height - 12)),
  };
}
