import { memo, useCallback, useEffect, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  ReactFlow,
  ReactFlowProvider,
  NodeResizer,
  applyNodeChanges,
  getViewportForBounds,
  type Node,
  type NodeProps,
  type ReactFlowInstance,
  type NodeChange,
} from "@xyflow/react";
import {
  takeNextDecode,
  trimPreviews,
  IMAGE_BUDGET,
  previewPlan,
  sectionLabels,
  dropTarget,
  sameSlot,
  boardBounds,
  type Camera,
  type Activity,
  type Layout,
  type Screen,
  type Request,
  type Section,
  type Arrangement,
  type DropTarget,
  type Rect,
} from "./model";
import "./style.css";
import {createInlineEditor} from "./inline-editor";
import { SectionBoard, CanvasMenu, type SectionNode, type MenuState } from "./sections";
type Boot = {
  session: string;
  revision: number;
  fingerprint: string;
  screens: Screen[];
  sections?: Section[];
  arrangement?: Arrangement;
  layout: Layout;
  theme: Record<string, string>;
  test?: boolean;
};
type Preview = { key: string; content_key: string; tier: number; width: number; height: number; url: string; image: HTMLImageElement; screenWidth:number;screenHeight:number };
type Data = {
  screen: Screen;
  preview?: Preview;
  error?: string;
  pending: boolean;
  editing: boolean;
  /** False once the artboard is too small to read, as for its preview image. */
  legible: boolean;
  begin: () => void;
  end: (width: number, height: number, x: number, y: number) => void;
};
type ArtboardNode = Node<Data, "artboard">;
type CanvasNode = ArtboardNode | SectionNode;
/** An in-flight artboard drag, with the revision it started from. */
type DragState = { id: string; revision: number; fingerprint: string; origin: { x: number; y: number }; cancelled: boolean; sizes: Map<string, Screen> };
declare global {
  interface Window {
    __CHORO_CANVAS__: Boot;
    ipc: { postMessage: (raw: string) => void };
    choroCanvasReply: (value: any) => void;
    choroCanvasStats: () => unknown;
    choroStudioReply: (value:any) => void;
    choroCanvasTest?: {
      flow: ReactFlowInstance<CanvasNode>;
      begin: (id: string) => void;
      hover: (id: string, x: number, y: number) => void;
      finish: (id: string, x: number, y: number) => void;
      drop: (id: string, x: number, y: number) => void;
      escape: () => void;
    };
  }
}
const SECTION_PREFIX = "section:";
const pointer = (event: MouseEvent | TouchEvent) => {
  if ("clientX" in event) return { x: event.clientX, y: event.clientY };
  const touch = event.changedTouches[0] ?? event.touches[0];
  return { x: touch?.clientX ?? 0, y: touch?.clientY ?? 0 };
};
const boot = window.__CHORO_CANVAS__;
const requestId = () =>
  crypto.randomUUID?.() ??
  "10000000-1000-4000-8000-100000000000".replace(/[018]/g, (c) =>
    (
      Number(c) ^
      (crypto.getRandomValues(new Uint8Array(1))[0] & (15 >> (Number(c) / 4)))
    ).toString(16),
  );
const send = (type: string, data: Record<string, unknown> = {}) =>
  window.ipc.postMessage(
    JSON.stringify({
      session: boot.session,
      type,
      request_id: requestId(),
      ...data,
    }),
  );
const CAPTIONS = {
  working: "Designing this screen…",
  editing: "Editing this screen…",
  queued: "Next in this turn",
} as const;
/** Marks work in flight. A blank screen gets a placeholder skeleton; an
 *  authored one keeps its real design visible under a lighter treatment. */
const DesigningOverlay = memo(function DesigningOverlay({
  activity,
}: {
  activity: Activity;
}) {
  return (
    <div className={`artboard-work ${activity}`} aria-hidden="true">
      {activity !== "queued" && <div className="artboard-sweep" />}
      {activity !== "editing" && (
        <div className="artboard-skeleton">
          <div className="bone head" />
          <div className="bone line" />
          <div className="bone line short" />
          <div className="bone card" />
          <div className="bone row" />
          <div className="bone cta" />
        </div>
      )}
      <div className="artboard-caption">
        <span className="bars">
          <i />
          <i />
          <i />
        </span>
        {CAPTIONS[activity]}
      </div>
    </div>
  );
});
const Artboard = memo(function Artboard({
  data,
  selected,
}: NodeProps<ArtboardNode>) {
  const [size, setSize] = useState<{ width: number; height: number } | null>(
    null,
  );
  useEffect(
    () => setSize(null),
    [data.screen.width, data.screen.height, data.pending],
  );
  return (
    <div
      className={`artboard ${selected ? "selected" : ""}`}
      aria-label={
        data.screen.activity === "working"
          ? `${data.screen.name} — being designed`
          : data.screen.activity === "editing"
            ? `${data.screen.name} — being edited`
            : data.screen.activity === "queued"
              ? `${data.screen.name} — queued for design`
              : data.screen.name
      }
    >
      <div className="artboard-title">
        <span className="artboard-name">{data.screen.name}</span>
        {data.screen.activity && (
          <span className={`artboard-chip ${data.screen.activity}`}>
            <span className="dot" />
            {data.screen.activity === "working"
              ? "Designing"
              : data.screen.activity === "editing"
                ? "Editing"
                : "Queued"}
          </span>
        )}
      </div>
      <NodeResizer
        isVisible={
          !!selected && !data.pending && !data.editing && !data.screen.activity
        }
        minWidth={240}
        maxWidth={3840}
        minHeight={240}
        maxHeight={4096}
        onResizeStart={() => {
          data.begin();
          setSize(null);
        }}
        onResize={(_, p) => setSize(p)}
        onResizeEnd={(_, p) => {
          data.end(Math.round(p.width), Math.round(p.height), p.x, p.y);
          setSize(null);
        }}
      />
      {data.preview ? (
        <div className="artboard-image">
          <img
            draggable={false}
            alt=""
            src={data.preview.url}
            style={{ width: data.preview.screenWidth, height: data.preview.screenHeight }}
          />
        </div>
      ) : (
        <div className="artboard-placeholder">
          {data.error
            ? "Preview unavailable"
            : data.pending
              ? "Saving…"
              : "Preview"}
        </div>
      )}
      {data.screen.activity && data.legible && (
        <DesigningOverlay activity={data.screen.activity} />
      )}
      {selected && (
        <div className="artboard-size">
          {size?.width ?? data.screen.width} ×{" "}
          {size?.height ?? data.screen.height}
          {data.pending
            ? " · Saving…"
            : data.error
              ? " · Refresh to retry"
              : ""}
        </div>
      )}
    </div>
  );
});
const nodeTypes = { artboard: Artboard, section: SectionBoard };
function Canvas() {
  const [nodes, setNodes] = useState<CanvasNode[]>([]);
  const inline = useRef<ReturnType<typeof createInlineEditor> | null>(null);
  const screens = useRef(boot.screens),
    sections = useRef<Section[]>(boot.sections ?? []),
    arrangement = useRef<Arrangement>(boot.arrangement ?? "stacked"),
    layout = useRef(boot.layout),
    revision = useRef({
      revision: boot.revision,
      fingerprint: boot.fingerprint,
    });
  const drag = useRef<DragState | null>(null),
    hint = useRef<DropTarget | null>(null),
    /** Screens dropped into or out of a section, held until the host answers. */
    moving = useRef(new Map<string, { request_id: string; x: number; y: number; timer: ReturnType<typeof setTimeout> }>()),
    labelZoom = useRef(boot.layout.viewport.zoom);
  const [menu, setMenu] = useState<MenuState>(null);
  const flow = useRef<ReactFlowInstance<CanvasNode> | null>(null),
    desired = useRef(new Map<string, Request>()),
    previews = useRef(new Map<string, Preview>());
  const [ready, setReady] = useState(false);
  const pending = useRef(
      new Map<
        string,
        {
          request_id: string;
          width: number;
          height: number;
          x: number;
          y: number;
        }
      >(),
    ),
    failures = useRef(new Map<string, string>());
  const resize = useRef<{
    id: string;
    revision: number;
    fingerprint: string;
    cancelled: boolean;
    position: { x: number; y: number };
  } | null>(null);
  const timers = useRef<{
    preview?: ReturnType<typeof setTimeout>;
    camera?: ReturnType<typeof setTimeout>;
  }>({});
  const decodeQueue = useRef<any[]>([]),
    decoding = useRef(0),
    decodingBytes = useRef(0),
    alive = useRef(true),
    decodingKeys = useRef(new Set<string>());
  const lastDemand = useRef("");
  const lastImageVisibility = useRef("");
  const lastMotion = useRef(-Infinity);
  const canvasCamera = useRef<Camera | null>(null);
  const activation = useRef<{screen:string;x:number;y:number} | null>(null);
  const fitRects = (rects: Rect[], boards: Section[] = []) => {
    const f=flow.current;if(!f||(!rects.length&&!boards.length))return;
    const target=inline.current?.area??{x:0,y:0,width:innerWidth,height:innerHeight};
    let v = { x: 0, y: 0, zoom: 1 };
    // Recompute the display-sized title extents at the resulting fit zoom.
    for (let pass = 0; pass < 4; pass++) {
      const board = boardBounds(boards, v.zoom);
      const all = board ? [...rects, board] : rects;
      const x=Math.min(...all.map(r=>r.x)),y=Math.min(...all.map(r=>r.y));
      const width=Math.max(...all.map(r=>r.x+r.width))-x,height=Math.max(...all.map(r=>r.y+r.height))-y;
      v=getViewportForBounds({x,y,width,height},target.width,target.height,.02,1,.15);
    }
    void f.setViewport({...v,x:v.x+target.x,y:v.y+target.y});
  };
  /** Fit all includes every section with its title band. */
  const fitScreens = (selected?: string) => {
    const focus=layout.current.overview_mode==="focus";
    const visible=screens.current.filter(s=>!s.archived&&(!selected||s.id===selected)
      &&(!focus||s.id===layout.current.selected_screen_id));
    const rects:Rect[]=visible.map(s=>({s,p:layout.current.positions[s.id]})).filter(v=>v.p).map(v=>({x:v.p.x,y:v.p.y,width:v.s.width,height:v.s.height}));
    fitRects(rects,!selected&&!focus?sections.current:[]);
  };
  /** Keep section object identity when unchanged so boards do not re-render. */
  const adoptSections = (next?: Section[], nextArrangement?: Arrangement) => {
    if (nextArrangement) arrangement.current = nextArrangement;
    if (!next) return;
    const old = new Map(sections.current.map((s) => [s.id, s]));
    sections.current = next.map((s) => (JSON.stringify(old.get(s.id)) === JSON.stringify(s) ? old.get(s.id)! : s));
  };
  const sectionOf = (screen: string) => sections.current.find((s) => s.screen_ids.includes(screen)) ?? null;
  const fitSection = (id: string) => {
    const s=sections.current.find(s=>s.id===id);
    if(s)fitRects([], [s]);
  };
  const syncNodes = useCallback(() => {
    setNodes((old) => {
      const previousNodes = new Map<string, CanvasNode>(old.map(n => [n.id, n]));
      const focus = layout.current.overview_mode === "focus";
      const zoom = labelZoom.current;
      const selectedSection = layout.current.selected_section_id ?? null;
      const plans = sectionLabels(sections.current, zoom, selectedSection, arrangement.current);
      // Section boards render behind artboards and never move on their own.
      const boards: SectionNode[] = sections.current.map((section) => {
        const id = SECTION_PREFIX + section.id;
        const previous = previousNodes.get(id) as SectionNode | undefined;
        const target = hint.current?.section_id === section.id;
        const marker = target ? hint.current?.marker : undefined;
        const label = plans[section.id];
        const selected = selectedSection === section.id;
        if (previous && previous.data.section === section && previous.data.selected === selected &&
          previous.data.target === target && previous.data.marker === marker && previous.data.zoom === zoom &&
          previous.data.label.mode === label.mode && previous.data.label.size === label.size &&
          previous.data.label.maxWidth === label.maxWidth && previous.data.label.chars === label.chars && previous.hidden === focus)
          return previous;
        // Exact host geometry doubles as the measured size, so a re-planned
        // title never makes a board briefly unmeasured (and thus unculled).
        return {
          id, type: "section", position: { x: section.x, y: section.y },
          width: section.width, height: section.height,
          measured: { width: section.width, height: section.height },
          style: { width: section.width, height: section.height },
          zIndex: -1, selectable: false, draggable: false, focusable: false, hidden: focus,
          data: { section, selected, target, marker, label, zoom },
        };
      });
      const artboards = screens.current
        .filter((s) => !s.archived)
        .map((screen) => {
          const previous = previousNodes.get(screen.id) as ArtboardNode | undefined;
          if (
            previous &&
            resize.current?.id === screen.id &&
            !resize.current.cancelled
          )
            return previous;
          const saving = pending.current.get(screen.id);
          const moved = moving.current.get(screen.id);
          const legible =
            Math.max(screen.width, screen.height) * layout.current.viewport.zoom >= 48 ||
            inline.current?.screen === screen.id;
          const n: ArtboardNode = {
            id: screen.id,
            type: "artboard",
            position: previous?.dragging
              ? previous.position
              : saving
                ? { x: saving.x, y: saving.y }
                : moved
                  ? { x: moved.x, y: moved.y }
                  : (layout.current.positions[screen.id] ?? { x: 0, y: 0 }),
            width: saving?.width ?? screen.width,
            height: saving?.height ?? screen.height,
            dragging: previous?.dragging,
            style: {
              width: saving?.width ?? screen.width,
              height: saving?.height ?? screen.height,
            },
            dragHandle: ".artboard-title",
            hidden: layout.current.overview_mode === "focus" && layout.current.selected_screen_id !== screen.id,
            selected: layout.current.selected_screen_id === screen.id,
            draggable: layout.current.overview_mode !== "focus" && !pending.current.has(screen.id) && !moved,
            data: {
              screen,
              legible,
              preview: legible ? previews.current.get(screen.id) : undefined,
              error: failures.current.get(screen.id),
              pending: pending.current.has(screen.id) || !!moved,
              editing: inline.current?.screen===screen.id,
              begin: () => {
                resize.current = {
                  id: screen.id,
                  ...revision.current,
                  cancelled: false,
                  position: layout.current.positions[screen.id],
                };
              },
              end: (width, height, x, y) => {
                const start = resize.current;
                resize.current = null;
                if (
                  !start ||
                  start.cancelled ||
                  pending.current.has(screen.id)
                ) {
                  syncNodes();
                  return;
                }
                if (width === screen.width && height === screen.height) {
                  syncNodes();
                  return;
                }
                const request_id = requestId();
                pending.current.set(screen.id, {
                  request_id,
                  width,
                  height,
                  x,
                  y,
                });
                send("resize", {
                  request_id,
                  screen_id: screen.id,
                  width,
                  height,
                  x,
                  y,
                  revision: start.revision,
                  fingerprint: start.fingerprint,
                });
                setNodes((all) =>
                  all.map((n): CanvasNode =>
                    n.id === screen.id && n.type === "artboard"
                      ? {
                          ...n,
                          draggable: false,
                          data: { ...n.data, pending: true },
                        }
                      : n,
                  ),
                );
              },
            },
          };
          if (
            previous &&
            previous.data.screen === screen &&
            previous.data.preview === n.data.preview &&
            previous.data.legible === n.data.legible &&
            previous.selected === n.selected &&
            previous.hidden === n.hidden &&
            previous.draggable === n.draggable &&
            previous.data.pending === n.data.pending &&
            previous.data.editing === n.data.editing &&
            previous.data.error === n.data.error &&
            previous.position.x === n.position.x &&
            previous.position.y === n.position.y &&
            previous.width === n.width &&
            previous.height === n.height
          )
            return previous;
          return n;
        });
      const next: CanvasNode[] = [...boards, ...artboards];
      return old.length === next.length && next.every((n, i) => n === old[i]) ? old : next;
    });
  }, []);
  /** Camera and insertion feedback leave every artboard object untouched. */
  const syncSectionNodes = () => {
    const zoom = labelZoom.current;
    const plans = sectionLabels(sections.current, zoom, layout.current.selected_section_id, arrangement.current);
    setNodes((all) => all.map((node) => {
      if (node.type !== "section") return node;
      const target = hint.current?.section_id === node.data.section.id;
      const marker = target ? hint.current?.marker : undefined;
      const label = plans[node.data.section.id];
      const before = node.data.label;
      if (node.data.zoom === zoom && node.data.target === target && node.data.marker === marker &&
          before.mode === label.mode && before.size === label.size && before.chars === label.chars && before.maxWidth === label.maxWidth)
        return node;
      return { ...node, data: { ...node.data, zoom, target, marker, label } };
    }));
  };
  const trackMove = (id: string, request_id: string, position: { x: number; y: number }, origin = position) => {
    const timer = setTimeout(() => {
      if (!alive.current || moving.current.get(id)?.request_id !== request_id) return;
      moving.current.delete(id);
      layout.current.positions[id] = origin;
      failures.current.set(id, "The grouping request did not respond. Restoring saved positions.");
      syncNodes();
      // Request authoritative state, including any edit completed after the timeout.
      send("ready");
    }, 35000);
    moving.current.set(id, { request_id, ...position, timer });
  };
  const planPreviews = useCallback(() => {
    if (!flow.current || !alive.current) return;
    const v = flow.current.getViewport();
    layout.current.viewport = v;
    const requests = previewPlan(
      screens.current.filter(s=>s.id!==inline.current?.screen && (layout.current.overview_mode!=="focus"||s.id===layout.current.selected_screen_id)),
      layout.current.positions,
      v,
      innerWidth,
      innerHeight,
      devicePixelRatio,
      layout.current.selected_screen_id,
      performance.now() - lastMotion.current < 150,
    );
    desired.current = new Map(requests.map((r) => [r.screen_id, r]));
    // Touch visible images without dropping the rest. Returning to an artboard
    // can reuse its decoded image even if the host's encoded LRU has evicted it.
    for (const r of requests) {
      const p = previews.current.get(r.screen_id);
      if (p) { previews.current.delete(r.screen_id); previews.current.set(r.screen_id, p); }
    }
    const retained = new Set(requests.map(r => r.screen_id));
    if (inline.current?.screen) retained.add(inline.current.screen);
    const evicted = trimPreviews(previews.current, retained);
    for (const p of evicted) p.image.src = "";
    decodeQueue.current = decodeQueue.current.filter((r) => {
      const d = desired.current.get(r.screen_id);
      return d && d.content_key === r.content_key && r.tier <= d.tier;
    });
    const visibility = screens.current.filter(s => Math.max(s.width, s.height) * v.zoom >= 48).map(s => s.id).join(',');
    if (evicted.length || visibility !== lastImageVisibility.current) syncNodes();
    lastImageVisibility.current = visibility;
    const pressure = [...previews.current.values()].reduce((n, p) => n + p.width * p.height * 4, 0) > IMAGE_BUDGET / 2;
    const missing = requests.filter(r => {
      const p = previews.current.get(r.screen_id);
      return !p || p.content_key !== r.content_key || p.tier < r.tier || (pressure && p.tier > r.tier);
    });
    const signature = JSON.stringify(missing);
    if (signature !== lastDemand.current) {
      lastDemand.current = signature;
      send("previews", { requests: missing });
    }
  }, [syncNodes]);
  const positionEditor = useCallback(() => {
    const id=inline.current?.screen,f=flow.current;if(!id||!f)return;
    const p=f.getNode(id)?.position??layout.current.positions[id];if(!p)return;
    const v=f.getViewport();inline.current?.position(v.x+p.x*v.zoom,v.y+p.y*v.zoom,v.zoom);
  },[]);
  const lastPlan = useRef(0);
  const schedulePreviews = useCallback(() => {
    // Keep demand current while moving; refine once the camera settles.
    if (performance.now() - lastPlan.current >= 50) {
      lastPlan.current = performance.now();
      planPreviews();
    }
    clearTimeout(timers.current.preview);
    timers.current.preview = setTimeout(planPreviews, 150);
  }, [planPreviews]);
  const flushCamera = useCallback(() => {
    clearTimeout(timers.current.camera);
    // Focus shows one screen, opened fitted and centred every time, so its
    // camera is transient: remembering it would carry a zoom across screens
    // and overwrite the saved Canvas arrangement view with it.
    if (layout.current.overview_mode === "focus") return;
    send("camera", { viewport: layout.current.viewport });
  }, []);
  const decodeNext = useCallback(
    function next() {
      while (
        decoding.current < 2 &&
        decodeQueue.current.length &&
        alive.current
      ) {
        const item = takeNextDecode(
          decodeQueue.current,
          previews.current,
          decodingBytes.current,
        );
        if (!item) break;
        const wanted = desired.current.get(item.screen_id);
        if (
          !wanted ||
          wanted.content_key !== item.content_key ||
          item.tier > wanted.tier
        )
          continue;
        if (
          previews.current.get(item.screen_id)?.key === item.key ||
          decodingKeys.current.has(item.key)
        )
          continue;
        const sourceScreen=screens.current.find(s=>s.id===item.screen_id&&s.content_key===item.content_key);
        if(!sourceScreen)continue;
        const bytes = item.width * item.height * 4;
        if (
          !Number.isInteger(item.width) ||
          !Number.isInteger(item.height) ||
          item.width < 1 ||
          item.height < 1 ||
          item.width > 2048 ||
          item.height > 2048
        )
          continue;
        decoding.current++;
        decodingBytes.current += bytes;
        decodingKeys.current.add(item.key);
        const image = new Image();
        let retained = false;
        // The isolated fixture uses data images; production only receives opaque keys.
        image.src =
          boot.test && item.test_url
            ? item.test_url
            : `choro-canvas-image://localhost/${boot.session}/${item.key}.png`;
        image
          .decode()
          .then(() => {
            const current = desired.current.get(item.screen_id);
            if (
              !alive.current ||
              !current ||
              current.content_key !== item.content_key ||
              item.tier > current.tier
            )
              return;
            const previous = previews.current.get(item.screen_id);
            if (previous && previous.content_key === item.content_key &&
              (previous.tier === item.tier || (previous.tier > item.tier && item.tier !== current.tier))) return;
            if (
              image.naturalWidth !== item.width ||
              image.naturalHeight !== item.height
            )
              throw Error("Preview dimensions changed");
            previews.current.set(item.screen_id, {
              key: item.key,
              content_key: item.content_key,
              tier: item.tier,
              width: item.width,
              height: item.height,
              url: image.src,
              image,
              screenWidth:sourceScreen.width,screenHeight:sourceScreen.height,
            });
            retained = true;
            if (previous) previous.image.src = "";
            failures.current.delete(item.screen_id);
            syncNodes();
          })
          .catch(() => {
            const current=desired.current.get(item.screen_id);
            if (alive.current && current?.content_key===item.content_key && current?.tier===item.tier) {
              failures.current.set(item.screen_id, "Preview unavailable");
              syncNodes();
            }
          })
          .finally(() => {
            decoding.current--;
            decodingBytes.current -= bytes;
            decodingKeys.current.delete(item.key);
            if (!retained) image.src = "";
            planPreviews();
            next();
          });
      }
    },
    [syncNodes, planPreviews],
  );
  useEffect(() => {
    alive.current = true;
    syncNodes();
    inline.current=createInlineEditor(send,(input,id,area)=>{
      const f=flow.current;if(!f||!input)return;
      const v=f.getViewport(),screen=screens.current.find(s=>s.id===id),p=f.getNode(id)?.position??layout.current.positions[id];
      if(!screen||!p)return;
      if(input.kind==='sync'){positionEditor();return;}
      if(input.kind==='end'){flushCamera();return;}
      const target=area??{x:0,y:0,width:innerWidth,height:innerHeight};
      if(input.kind==='fit-screen'||input.kind==='fit-selection'){
        const r=input.kind==='fit-selection'?input.rect:{x:0,y:0,width:screen.width,height:screen.height};
        if(!r||![r.x,r.y,r.width,r.height].every(Number.isFinite)||r.width<=0||r.height<=0)return;
        const next=getViewportForBounds({x:p.x+r.x,y:p.y+r.y,width:r.width,height:r.height},target.width,target.height,.02,input.kind==='fit-screen'?1:2,.15);
        void f.setViewport({...next,x:next.x+target.x,y:next.y+target.y});return;
      }
      if(input.kind==='preset'){
        if(!Number.isFinite(input.zoom))return;
        input={kind:'gesture',x:target.x+target.width/2,y:target.y+target.height/2,dx:0,dy:Math.max(.02,Math.min(2,input.zoom))/v.zoom};
      }
      if(![input.x,input.y,input.dx,input.dy].every(n=>Number.isFinite(n)&&Math.abs(n)<1e6))return;
      if(input.kind==='pan')void f.setViewport({...v,x:v.x+input.dx,y:v.y+input.dy});
      else if(input.kind==='zoom'||input.kind==='gesture'){
        const factor=input.kind==='gesture'?input.dy:Math.exp(-Math.max(-200,Math.min(200,input.dy))*.01);
        if(factor<=0||factor>100)return;
        const zoom=Math.max(.02,Math.min(2,v.zoom*factor)),ratio=zoom/v.zoom;
        void f.setViewport({zoom,x:input.x-(input.x-v.x)*ratio,y:input.y-(input.y-v.y)*ratio});
      }
    },()=>{syncNodes();schedulePreviews();});
    window.choroStudioReply=value=>inline.current?.reply(value);
    window.choroCanvasReply = (message) => {
      if (message.session !== boot.session) return;
      if (message.type === "state") {
        if(message.revision<revision.current.revision)return;
        if(message.theme){for(const [key,value] of Object.entries(message.theme))document.documentElement.style.setProperty(`--${key}`,String(value));inline.current?.theme(message.theme);}
        revision.current = {
          revision: message.revision,
          fingerprint: message.fingerprint,
        };
        const old = new Map(screens.current.map((s) => [s.id, s]));
        screens.current = message.screens.map((s: Screen) =>
          JSON.stringify(old.get(s.id)) === JSON.stringify(s)
            ? old.get(s.id)
            : s,
        );
        adoptSections(message.sections, message.arrangement);
        const previousView = layout.current.overview_mode;
        if (previousView === "canvas" && message.layout.overview_mode === "focus")
          canvasCamera.current = flow.current?.getViewport() ?? layout.current.viewport;
        layout.current = {
          ...message.layout,
          viewport: layout.current.viewport,
        };
        syncNodes();
        schedulePreviews();
        if (previousView !== layout.current.overview_mode) {
          const view = layout.current.overview_mode;
          requestAnimationFrame(() => {
            if (layout.current.overview_mode !== view) return;
            if (view === "focus") inline.current?.focus();
            else if (view === "canvas" && canvasCamera.current) void flow.current?.setViewport(canvasCamera.current);
          });
        }
      } else if (message.type === "selection") {
        layout.current.selected_screen_id = message.screen_id;
        layout.current.selected_section_id = message.section_id;
        syncNodes();
        schedulePreviews();
      } else if (message.type === "editor") {
        inline.current?.open(message.screen_id?message:null);
        const point=activation.current;
        if (point && point.screen === message.screen_id) {
          inline.current?.activate(point.x,point.y);
          activation.current = null;
        }
        if (message.screen_id && layout.current.overview_mode === "focus") inline.current?.focus();
        positionEditor();
      } else if (message.type === "preview") {
        if (
          decodeQueue.current.length < 200 &&
          !decodeQueue.current.some((item) => item.key === message.key)
        )
          decodeQueue.current.push(message);
        decodeNext();
      } else if (message.type === "preview-failed") {
        if (
          desired.current.get(message.screen_id)?.content_key ===
          message.content_key
        ) {
          failures.current.set(message.screen_id, message.error);
          syncNodes();
        }
      } else if (message.type === "resize-result") {
        const active = pending.current.get(message.screen_id);
        if (!active || active.request_id !== message.request_id) return;
        pending.current.delete(message.screen_id);
        if (message.revision < revision.current.revision) {
          syncNodes();
          schedulePreviews();
          return;
        }
        if (message.error)
          failures.current.set(message.screen_id, message.error);
        revision.current = {
          revision: message.revision,
          fingerprint: message.fingerprint,
        };
        screens.current = message.screens;
        adoptSections(message.sections, message.arrangement);
        layout.current.positions = message.positions;
        syncNodes();
        schedulePreviews();
      } else if (message.type === "move-result") {
        // Correlated by request: stale or foreign answers are ignored.
        const entry = [...moving.current].find(([, m]) => m.request_id === message.request_id);
        if (!entry) return;
        clearTimeout(entry[1].timer);
        moving.current.delete(entry[0]);
        if (message.error) failures.current.set(entry[0], message.error);
        else failures.current.delete(entry[0]);
        if (message.revision >= revision.current.revision) {
          // Success or conflict, the host's answer is the authority to show.
          revision.current = { revision: message.revision, fingerprint: message.fingerprint };
          screens.current = message.screens;
          adoptSections(message.sections, message.arrangement);
          layout.current.positions = message.positions;
        }
        syncNodes();
        schedulePreviews();
      } else if (message.type === "command" && flow.current) {
        const f = flow.current;
        if (message.command === "focus-screen" && message.screen_id === inline.current?.screen)
          inline.current?.focus();
        if (message.command === "zoom-in") void f.zoomIn({ duration: 0 });
        if (message.command === "zoom-out") void f.zoomOut({ duration: 0 });
        if (message.command === "fit-all") fitScreens();
        // Section geometry is host-authored, so no layout pass is needed first.
        if (message.command === "fit-section" && typeof message.section_id === "string")
          fitSection(message.section_id);
        if (message.command === "fit-selected" && inline.current?.screen) inline.current.focus();
        else if (
          message.command === "fit-selected" &&
          layout.current.selected_screen_id
        )
          fitScreens(layout.current.selected_screen_id);
        if (message.command === "flush") {
          layout.current.viewport = f.getViewport();
          flushCamera();
          inline.current?.flush(message.request_id);
        }
        if (message.command === "refresh") {
          failures.current.clear();
          lastDemand.current = "";
          planPreviews();
        }
      }
    };
    window.choroCanvasStats = () => ({
      mounted: document.querySelectorAll(".react-flow__node-artboard").length,
      sections_mounted: document.querySelectorAll(".react-flow__node-section").length,
      images: previews.current.size,
      decoded_bytes: [...previews.current.values()].reduce(
        (n, p) => n + p.width * p.height * 4,
        0,
      ),
      decoding: decoding.current,
      decoding_bytes: decodingBytes.current,
      iframes: document.querySelectorAll("iframe").length,
    });
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape" && resize.current) {
        resize.current.cancelled = true;
        syncNodes();
        event.preventDefault();
      }
      if (event.key === "Escape" && drag.current && !drag.current.cancelled) {
        cancelDrag();
        event.preventDefault();
      }
      if (event.key === "Escape") setMenu(null);
      if (
        event.key === "Enter" &&
        layout.current.selected_screen_id &&
        !resize.current &&
        !(event.target as HTMLElement | null)?.closest?.(".canvas-menu")
      ) {
        flushCamera();
        send("open", { screen_id: layout.current.selected_screen_id });
        event.preventDefault();
      }
    };
    const onResize = () => schedulePreviews();
    addEventListener("keydown", onKey);
    addEventListener("resize", onResize);
    return () => {
      flushCamera();
      alive.current = false;
      inline.current?.dispose();inline.current=null;
      clearTimeout(timers.current.preview);
      clearTimeout(timers.current.camera);
      for (const move of moving.current.values()) clearTimeout(move.timer);
      for (const p of previews.current.values()) p.image.src = "";
      previews.current.clear();
      decodeQueue.current = [];
      removeEventListener("keydown", onKey);
      removeEventListener("resize", onResize);
    };
  }, [syncNodes, schedulePreviews, decodeNext, flushCamera, planPreviews]);
  const onChanges = useCallback((changes: NodeChange<CanvasNode>[]) => {
    const accepted = changes.filter(
      (c) =>
        !(
          resize.current?.cancelled &&
          "id" in c &&
          c.id === resize.current.id &&
          (c.type === "dimensions" || c.type === "position")
        ) &&
        // A drag cancelled with Escape stays at its authoritative position.
        !(drag.current?.cancelled && "id" in c && c.id === drag.current.id && c.type === "position") &&
        // Grouped screens move only by drag-and-drop; keyboard nudges cannot detach them.
        !(c.type === "position" && !c.dragging && !drag.current && !!sectionOf(c.id)),
    );
    for (const c of accepted) {
      if (c.type === "select" && !c.id.startsWith(SECTION_PREFIX)) {
        const next = c.selected
          ? c.id
          : layout.current.selected_screen_id === c.id
            ? null
            : layout.current.selected_screen_id;
        if (next !== layout.current.selected_screen_id) {
          layout.current.selected_screen_id = next;
          if (next) layout.current.selected_section_id = null;
          send("select", { screen_id: next });
        }
      }
      if (
        c.type === "position" &&
        c.position &&
        !resize.current &&
        !pending.current.has(c.id) &&
        !moving.current.has(c.id) &&
        !c.id.startsWith(SECTION_PREFIX)
      ) {
        // Grouped screens keep their section's authoritative position;
        // drops are decided once in finishDrag.
        const grouped = !!sectionOf(c.id);
        if (!grouped) layout.current.positions[c.id] = c.position;
        if (!c.dragging && !drag.current && !grouped)
          send("position", { screen_id: c.id, position: c.position });
      }
    }
    setNodes((n) => applyNodeChanges(accepted, n));
  }, []);
  /** Escape: restore the authoritative position; nothing is saved. */
  const cancelDrag = () => {
    const d = drag.current;
    if (!d) return;
    d.cancelled = true;
    hint.current = null;
    if (!sectionOf(d.id)) layout.current.positions[d.id] = d.origin;
    setNodes((all) => all.map((n) => (n.id === d.id ? { ...n, position: { ...d.origin }, dragging: false } : n)));
    syncNodes();
  };
  /** One accepted drop is one saved edit; same-slot drops snap back. */
  const finishDrag = useCallback((id: string, position: { x: number; y: number }, pointer: { x: number; y: number }) => {
    const start = drag.current;
    drag.current = null;
    hint.current = null;
    if (!start || start.id !== id || start.cancelled) { syncNodes(); return; }
    const grouped = sectionOf(id);
    const target: DropTarget = sections.current.length
      ? dropTarget(sections.current, screens.current, layout.current.positions, id, pointer, start.sizes)
      : { section_id: null, before_screen_id: null };
    if (!target.section_id && !grouped) {
      layout.current.positions[id] = position;
      send("position", { screen_id: id, position });
      syncNodes();
      return;
    }
    if (target.section_id && sameSlot(sections.current, id, target)) { syncNodes(); return; }
    const request_id = requestId();
    trackMove(id, request_id, position, start.origin);
    send("move-screen", {
      request_id, screen_id: id, section_id: target.section_id, before_screen_id: target.before_screen_id,
      position: target.section_id ? null : position, revision: start.revision, fingerprint: start.fingerprint,
    });
    syncNodes();
  }, [syncNodes]);
  const beginDrag = (id: string, position: { x: number; y: number }) => {
    drag.current = { id, ...revision.current, origin: { ...(layout.current.positions[id] ?? position) }, cancelled: false, sizes: new Map(screens.current.map((s) => [s.id, s])) };
    setMenu(null);
  };
  /** Show the target section and insertion point without reflowing anything. */
  const hoverDrag = (id: string, pointer: { x: number; y: number }) => {
    const d = drag.current;
    if (!d || d.cancelled || d.id !== id || !sections.current.length) return;
    const target = dropTarget(sections.current, screens.current, layout.current.positions, id, pointer, d.sizes);
    const next = target.section_id ? target : null;
    if (next?.section_id === hint.current?.section_id && next?.before_screen_id === hint.current?.before_screen_id) return;
    hint.current = next;
    syncSectionNodes();
  };
  const closeMenu = useCallback(() => setMenu(null), []);
  const onInit = useCallback(
    (f: ReactFlowInstance<CanvasNode>) => {
      flow.current = f;
      // Isolated fixtures drive the same drag lifecycle without OS pointer events.
      if (boot.test) window.choroCanvasTest = {
        flow: f,
        begin: (id) => beginDrag(id, layout.current.positions[id] ?? { x: 0, y: 0 }),
        hover: (id, x, y) => hoverDrag(id, { x, y }),
        finish: (id, x, y) => finishDrag(id, { x, y }, { x, y }),
        drop: (id, x, y) => { beginDrag(id, { x, y }); hoverDrag(id, { x, y }); finishDrag(id, { x, y }, { x, y }); },
        escape: () => dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
      };
      setReady(true);
      send("ready");
      schedulePreviews();
    },
    [schedulePreviews],
  );
  useEffect(() => {
    if (
      ready &&
      screens.current.length &&
      (boot as Boot & { fit_initial?: boolean }).fit_initial
    )
      void flow.current?.fitView({ padding: 0.12, minZoom: 0.02, maxZoom: 1 });
  }, [ready]);
  return (
    <>
      <ReactFlow<CanvasNode>
        nodes={nodes}
        edges={[]}
        nodeTypes={nodeTypes}
        onNodesChange={onChanges}
        onInit={onInit}
        defaultViewport={boot.layout.viewport}
        minZoom={0.02}
        maxZoom={2}
        onlyRenderVisibleElements
        nodesConnectable={false}
        elementsSelectable
        nodesDraggable
        selectionOnDrag={false}
        selectionKeyCode={null}
        multiSelectionKeyCode={null}
        deleteKeyCode={null}
        panOnScroll
        panOnScrollSpeed={1}
        panOnDrag={[1]}
        panActivationKeyCode="Space"
        zoomOnScroll={false}
        zoomOnPinch
        zoomActivationKeyCode={["Meta", "Control"]}
        zoomOnDoubleClick={false}
        onMove={(_, v) => {
          lastMotion.current = performance.now();
          layout.current.viewport = v;
          if (document.documentElement.style.getPropertyValue("--zoom") !== String(v.zoom))
            document.documentElement.style.setProperty("--zoom", String(v.zoom));
          // Re-plan section titles only when the zoom has moved noticeably.
          if (sections.current.length && Math.abs(Math.log(v.zoom / labelZoom.current)) > 0.03) {
            labelZoom.current = v.zoom;
            syncSectionNodes();
          }
          clearTimeout(timers.current.camera);
          timers.current.camera = setTimeout(flushCamera, 300);
          positionEditor();
          schedulePreviews();
        }}
        onMoveEnd={() => {
          // Programmatic movement (including each wheel event from the live
          // editor) also emits move-end. Persist only after the gesture settles.
          clearTimeout(timers.current.camera);
          timers.current.camera = setTimeout(flushCamera, 300);
          positionEditor();
          schedulePreviews();
        }}
        onNodeClick={(_, n) => {
          if (n.type === "section") {
            const id = n.id.slice(SECTION_PREFIX.length);
            layout.current.selected_section_id = id;
            layout.current.selected_screen_id = null;
            send("select-section", { section_id: id });
            syncNodes();
            return;
          }
          if(inline.current?.screen && inline.current.screen!==n.id){send("open",{screen_id:n.id});return;}
          layout.current.selected_screen_id = n.id;
          layout.current.selected_section_id = null;
          send("select", { screen_id: n.id });
          syncNodes();
          schedulePreviews();
        }}
        onNodeContextMenu={(event, n) => {
          event.preventDefault();
          const section = n.type === "section";
          setMenu({ x: event.clientX, y: event.clientY, kind: section ? "section" : "screen", id: section ? n.id.slice(SECTION_PREFIX.length) : n.id, ...revision.current });
        }}
        onNodeDoubleClick={(event, n) => {
          if (n.type === "section") { fitSection(n.id.slice(SECTION_PREFIX.length)); return; }
          const v=flow.current!.getViewport();
          activation.current={screen:n.id,x:(event.clientX-v.x)/v.zoom-n.position.x,y:(event.clientY-v.y)/v.zoom-n.position.y};
          flushCamera();
          send("open", { screen_id: n.id });
        }}
        onPaneClick={() => {
          if(inline.current?.screen){inline.current.reply({session:inline.current.session,type:"deselect"});return;}
          if(layout.current.overview_mode === "focus")return;
          layout.current.selected_screen_id = null;
          layout.current.selected_section_id = null;
          send("select", { screen_id: null });
          syncNodes();
        }}
        onNodeDragStart={(_, n) => { if (n.type === "artboard") beginDrag(n.id, n.position); }}
        onNodeDrag={(event, n) => {
          positionEditor();
          if (n.type === "artboard" && flow.current)
            hoverDrag(n.id, flow.current.screenToFlowPosition(pointer(event)));
        }}
        onNodeDragStop={(event, n) => {
          if (n.type === "artboard" && flow.current)
            finishDrag(n.id, n.position, flow.current.screenToFlowPosition(pointer(event)));
          positionEditor();
          schedulePreviews();
        }}
      />
      {!screens.current.some((s) => !s.archived) && !sections.current.length && (
        <div className="empty">Add a screen to start designing</div>
      )}
      {menu && (
        <CanvasMenu
          menu={menu}
          sections={sections.current}
          current={menu.kind === "screen" ? (sectionOf(menu.id)?.id ?? null) : null}
          onClose={closeMenu}
          onAction={(action) => {
            if (menu.kind === "section" && action === "fit") { fitSection(menu.id); return; }
            send("context-action", menu.kind === "screen" ? { screen_id: menu.id, section_id: null, action } : { screen_id: null, section_id: menu.id, action });
          }}
          onMove={(section) => {
            const request_id = requestId();
            const position = layout.current.positions[menu.id] ?? { x: 0, y: 0 };
            trackMove(menu.id, request_id, position);
            send("move-screen", {
              request_id, screen_id: menu.id, section_id: section, before_screen_id: null, position: null,
              revision: menu.revision, fingerprint: menu.fingerprint,
            });
            syncNodes();
          }}
        />
      )}
    </>
  );
}
for (const [key, value] of Object.entries(boot.theme ?? {}))
  document.documentElement.style.setProperty(`--${key}`, value);
document.documentElement.style.setProperty("--zoom", String(boot.layout.viewport.zoom));
try {
  createRoot(document.getElementById("root")!, {
    onUncaughtError: (error) =>
      send("failed", {
        error: error instanceof Error ? error.stack : String(error),
      }),
  }).render(
    <ReactFlowProvider>
      <Canvas />
    </ReactFlowProvider>,
  );
} catch (error) {
  send("failed", { error: String(error) });
}
// WebKit defers ResizeObserver notifications when newly visible nodes mount.
// That browser notification is recoverable; actual script exceptions still fail closed.
addEventListener("error", (event) => {
  if (
    event.message !==
    "ResizeObserver loop completed with undelivered notifications."
  )
    send("failed", { error: event.message });
});
