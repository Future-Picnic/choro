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
  previewPlan,
  type Camera,
  type Layout,
  type Screen,
  type Request,
} from "./model";
import "./style.css";
import {createInlineEditor} from "./inline-editor";
type Boot = {
  session: string;
  revision: number;
  fingerprint: string;
  screens: Screen[];
  layout: Layout;
  theme: Record<string, string>;
  test?: boolean;
};
type Preview = { key: string; width: number; height: number; url: string; screenWidth:number;screenHeight:number };
type Data = {
  screen: Screen;
  preview?: Preview;
  error?: string;
  pending: boolean;
  editing: boolean;
  begin: () => void;
  end: (width: number, height: number, x: number, y: number) => void;
};
type ArtboardNode = Node<Data, "artboard">;
declare global {
  interface Window {
    __CHORO_CANVAS__: Boot;
    ipc: { postMessage: (raw: string) => void };
    choroCanvasReply: (value: any) => void;
    choroCanvasStats: () => unknown;
    choroStudioReply: (value:any) => void;
    choroCanvasTest?: { flow: ReactFlowInstance<ArtboardNode> };
  }
}
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
      aria-label={data.screen.name}
    >
      <div className="artboard-title">{data.screen.name}</div>
      <NodeResizer
        isVisible={!!selected && !data.pending && !data.editing}
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
const nodeTypes = { artboard: Artboard };
function Canvas() {
  const [nodes, setNodes] = useState<ArtboardNode[]>([]);
  const inline = useRef<ReturnType<typeof createInlineEditor> | null>(null);
  const screens = useRef(boot.screens),
    layout = useRef(boot.layout),
    revision = useRef({
      revision: boot.revision,
      fingerprint: boot.fingerprint,
    });
  const flow = useRef<ReactFlowInstance<ArtboardNode> | null>(null),
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
  const syncNodes = useCallback(() => {
    setNodes((old) =>
      screens.current
        .filter((s) => !s.archived)
        .map((screen) => {
          const previous = old.find((n) => n.id === screen.id);
          if (
            previous &&
            resize.current?.id === screen.id &&
            !resize.current.cancelled
          )
            return previous;
          const saving = pending.current.get(screen.id);
          const n: ArtboardNode = {
            id: screen.id,
            type: "artboard",
            position: previous?.dragging
              ? previous.position
              : saving
                ? { x: saving.x, y: saving.y }
                : (layout.current.positions[screen.id] ?? { x: 0, y: 0 }),
            width: saving?.width ?? screen.width,
            height: saving?.height ?? screen.height,
            dragging: previous?.dragging,
            style: {
              width: saving?.width ?? screen.width,
              height: saving?.height ?? screen.height,
            },
            dragHandle: ".artboard-title",
            selected: layout.current.selected_screen_id === screen.id,
            draggable: !pending.current.has(screen.id),
            data: {
              screen,
              preview: previews.current.get(screen.id),
              error: failures.current.get(screen.id),
              pending: pending.current.has(screen.id),
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
                  all.map((n) =>
                    n.id === screen.id
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
            previous.selected === n.selected &&
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
        }),
    );
  }, []);
  const planPreviews = useCallback(() => {
    if (!flow.current || !alive.current) return;
    const v = flow.current.getViewport();
    layout.current.viewport = v;
    const requests = previewPlan(
      screens.current.filter(s=>s.id!==inline.current?.screen),
      layout.current.positions,
      v,
      innerWidth,
      innerHeight,
      devicePixelRatio,
      layout.current.selected_screen_id,
    );
    desired.current = new Map(requests.map((r) => [r.screen_id, r]));
    let changed = false;
    for (const id of previews.current.keys())
      if (!desired.current.has(id)) {
        previews.current.delete(id);
        changed = true;
      }
    decodeQueue.current = decodeQueue.current.filter((r) => {
      const d = desired.current.get(r.screen_id);
      return d && d.content_key === r.content_key && d.tier === r.tier;
    });
    if (changed) syncNodes();
    send("previews", { requests });
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
          wanted.tier !== item.tier
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
              current.tier !== item.tier
            )
              return;
            if (
              image.naturalWidth !== item.width ||
              image.naturalHeight !== item.height
            )
              throw Error("Preview dimensions changed");
            previews.current.set(item.screen_id, {
              key: item.key,
              width: item.width,
              height: item.height,
              url: image.src,
              screenWidth:sourceScreen.width,screenHeight:sourceScreen.height,
            });
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
            image.src = "";
            next();
          });
      }
    },
    [syncNodes],
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
        layout.current = {
          ...message.layout,
          viewport: layout.current.viewport,
        };
        syncNodes();
        schedulePreviews();
      } else if (message.type === "editor") {
        inline.current?.open(message.screen_id?message:null);
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
        layout.current.positions = message.positions;
        syncNodes();
        schedulePreviews();
      } else if (message.type === "command" && flow.current) {
        const f = flow.current;
        if (message.command === "focus-screen" && message.screen_id === inline.current?.screen)
          inline.current?.focus();
        if (message.command === "zoom-in") void f.zoomIn({ duration: 0 });
        if (message.command === "zoom-out") void f.zoomOut({ duration: 0 });
        if (message.command === "fit-all")
          void f.fitView({
            padding: 0.12,
            minZoom: 0.02,
            maxZoom: 1,
            duration: 0,
          });
        if (
          message.command === "fit-selected" &&
          layout.current.selected_screen_id
        )
          void f.fitView({
            nodes: [{ id: layout.current.selected_screen_id }],
            padding: 0.15,
            minZoom: 0.02,
            maxZoom: 1,
            duration: 0,
          });
        if (message.command === "flush") {
          layout.current.viewport = f.getViewport();
          flushCamera();
          inline.current?.flush(message.request_id);
        }
        if (message.command === "refresh") {
          failures.current.clear();
          planPreviews();
        }
      }
    };
    window.choroCanvasStats = () => ({
      mounted: document.querySelectorAll(".react-flow__node").length,
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
      if (
        event.key === "Enter" &&
        layout.current.selected_screen_id &&
        !resize.current
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
      previews.current.clear();
      decodeQueue.current = [];
      removeEventListener("keydown", onKey);
      removeEventListener("resize", onResize);
    };
  }, [syncNodes, schedulePreviews, decodeNext, flushCamera, planPreviews]);
  const onChanges = useCallback((changes: NodeChange<ArtboardNode>[]) => {
    const accepted = changes.filter(
      (c) =>
        !(
          resize.current?.cancelled &&
          "id" in c &&
          c.id === resize.current.id &&
          (c.type === "dimensions" || c.type === "position")
        ),
    );
    for (const c of accepted) {
      if (c.type === "select") {
        const next = c.selected
          ? c.id
          : layout.current.selected_screen_id === c.id
            ? null
            : layout.current.selected_screen_id;
        if (next !== layout.current.selected_screen_id) {
          layout.current.selected_screen_id = next;
          send("select", { screen_id: next });
        }
      }
      if (
        c.type === "position" &&
        c.position &&
        !resize.current &&
        !pending.current.has(c.id)
      ) {
        layout.current.positions[c.id] = c.position;
        if (!c.dragging)
          send("position", { screen_id: c.id, position: c.position });
      }
    }
    setNodes((n) => applyNodeChanges(accepted, n));
  }, []);
  const onInit = useCallback(
    (f: ReactFlowInstance<ArtboardNode>) => {
      flow.current = f;
      if (boot.test) window.choroCanvasTest = { flow: f };
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
      <ReactFlow<ArtboardNode>
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
        panOnDrag={[1]}
        panActivationKeyCode="Space"
        zoomOnScroll={false}
        zoomOnPinch
        zoomActivationKeyCode={["Meta", "Control"]}
        zoomOnDoubleClick={false}
        onMove={(_, v) => {
          layout.current.viewport = v;
          clearTimeout(timers.current.camera);
          timers.current.camera = setTimeout(flushCamera, 300);
          positionEditor();
          schedulePreviews();
        }}
        onMoveEnd={() => {
          flushCamera();
          positionEditor();
          schedulePreviews();
        }}
        onNodeClick={(_, n) => {
          if(inline.current?.screen && inline.current.screen!==n.id){send("open",{screen_id:n.id});return;}
          layout.current.selected_screen_id = n.id;
          send("select", { screen_id: n.id });
          syncNodes();
          schedulePreviews();
        }}
        onNodeDoubleClick={(_, n) => {
          flushCamera();
          send("open", { screen_id: n.id });
        }}
        onPaneClick={() => {
          if(inline.current?.screen){send("close-editor");return;}
          layout.current.selected_screen_id = null;
          send("select", { screen_id: null });
          syncNodes();
        }}
        onNodeDrag={positionEditor}
        onNodeDragStop={(_, n) => {
          layout.current.positions[n.id] = n.position;
          positionEditor();
          send("position", { screen_id: n.id, position: n.position });
          schedulePreviews();
        }}
      />
      {!screens.current.some((s) => !s.archived) && (
        <div className="empty">Add a screen to start designing</div>
      )}
    </>
  );
}
for (const [key, value] of Object.entries(boot.theme ?? {}))
  document.documentElement.style.setProperty(`--${key}`, value);
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
