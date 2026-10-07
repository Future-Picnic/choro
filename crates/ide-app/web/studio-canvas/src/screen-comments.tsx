/** Screen/player adapter. It shares comment UI and storage with the canvas;
 * the authored iframe keeps running underneath a temporary click surface. */
import { useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { createPortal } from "react-dom";
import { CommentsLayer, CommentPins, type CommentBoard, type PinProps } from "./comments";
import { COMMENT_EVENT, commentAnchor, type CommentPin } from "./comments-model";

type Geometry = { screen_id: string; name: string; x: number; y: number; width: number; height: number; visibleWidth: number; visibleHeight: number; zoom: number; scrollX: number; scrollY: number };
declare global {
  interface Window {
    __CHORO_STUDIO__: { session: string; screen_id: string; thumbnail?: boolean; inline?: boolean; system_specimen?: boolean; screens?: { id: string; name: string; width: number; height: number; archived?: boolean }[] };
    choroCommentGeometry: () => Geometry;
    choroCommentWheel: (event: WheelEvent) => void;
    choroCommentsEnabled?: boolean;
    choroCommentSelection?: string | null;
    choroCommentFocus: (pin: CommentPin) => void;
  }
}
const boot = window.__CHORO_STUDIO__;
const requestId = () => typeof crypto.randomUUID === "function" ? crypto.randomUUID() : "10000000-1000-4000-8000-100000000000".replace(/[018]/g, c => (Number(c) ^ crypto.getRandomValues(new Uint8Array(1))[0] & 15 >> Number(c) / 4).toString(16));
const send = (type: string, data: Record<string, unknown> = {}) => window.ipc.postMessage(JSON.stringify({ session: boot.session, request_id: requestId(), type, ...data }));

function ScreenLayer() {
  const canvas = useRef(document.getElementById("canvas") as HTMLDivElement);
  const [geometry, setGeometry] = useState(window.choroCommentGeometry);
  useEffect(() => {
    const update = () => setGeometry(window.choroCommentGeometry());
    addEventListener("studio-comment-geometry", update);
    const observer = new ResizeObserver(update);
    observer.observe(canvas.current);
    update();
    return () => { removeEventListener("studio-comment-geometry", update); observer.disconnect(); };
  }, []);
  const boards = useMemo(() => new Map<string, CommentBoard>([
    ...(boot.screens ?? []).filter(screen => !screen.archived).map(screen => [screen.id, {
      id: screen.id, position: { x: 0, y: 0 }, width: screen.width, height: screen.height, hidden: screen.id !== geometry.screen_id,
      data: { screen: { name: screen.name } },
    }] as [string, CommentBoard]),
    [geometry.screen_id, {
    id: geometry.screen_id, position: { x: 0, y: 0 }, width: geometry.width, height: geometry.height,
    data: { screen: { name: geometry.name } },
    clip: { x: geometry.x, y: geometry.y, width: geometry.visibleWidth, height: geometry.visibleHeight },
  }]]), [geometry]);
  const camera = { x: geometry.x - geometry.scrollX * geometry.zoom, y: geometry.y - geometry.scrollY * geometry.zoom, zoom: geometry.zoom };
  const focus = (pin: CommentPin) => {
    if (pin.screen_id === geometry.screen_id) window.choroCommentFocus(pin);
    return true;
  };
  const capture = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const node = capture.current;
    const wheel = (event: WheelEvent) => { event.preventDefault(); event.stopPropagation(); window.choroCommentWheel(event); };
    node?.addEventListener("wheel", wheel, { passive: false });
    return () => node?.removeEventListener("wheel", wheel);
  }, []);
  return <>
    {createPortal(<div ref={capture} className="screen-comment-capture" aria-label="Place a comment on the screen"
      style={{ left: geometry.x, top: geometry.y, width: geometry.visibleWidth, height: geometry.visibleHeight }}
      onClick={event => {
        event.preventDefault(); event.stopPropagation();
        const area = canvas.current.getBoundingClientRect();
        const point = { x: (event.clientX - area.left - geometry.x) / geometry.zoom + geometry.scrollX,
          y: (event.clientY - area.top - geometry.y) / geometry.zoom + geometry.scrollY };
        const anchor = commentAnchor(point, { x: 0, y: 0, width: geometry.width, height: geometry.height });
        if (anchor) dispatchEvent(new CustomEvent(COMMENT_EVENT, { detail: { type: "place", draft: { id: requestId(), screen_id: geometry.screen_id, ...anchor } } }));
      }} onContextMenu={event => event.preventDefault()} />, canvas.current)}
    <CommentsLayer send={send} requestId={requestId} canvas={canvas} boards={boards}
      initialSelected={window.choroCommentSelection} focus={focus} crossScreen
      renderPins={(pins: PinProps) => <CommentPins {...pins} camera={camera} />} />
  </>;
}
function ScreenComments() {
  const [state, setState] = useState({ enabled: window.choroCommentsEnabled ?? false, screen: boot.screen_id });
  useEffect(() => {
    const mode = (event: Event) => setState({ enabled: (event as CustomEvent).detail.enabled, screen: boot.screen_id });
    addEventListener("studio-comment-mode", mode);
    setState({ enabled: window.choroCommentsEnabled ?? false, screen: boot.screen_id });
    return () => removeEventListener("studio-comment-mode", mode);
  }, []);
  return state.enabled ? <ScreenLayer key={state.screen} /> : null;
}
if (boot && !boot.inline && !boot.thumbnail && !boot.system_specimen) {
  const root = document.createElement("div"); root.className = "screen-comments-root"; document.body.append(root);
  createRoot(root).render(<ScreenComments />);
}
