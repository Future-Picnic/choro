import { useEffect, useLayoutEffect, useRef, useState, type RefObject, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { COMMENT_EVENT, commentBodyValid, commentDate, commentPopoverPosition, pinPosition, type CommentDraft, type CommentPin, type CommentState } from "./comments-model";
import { StageButton, StageIconButton, PinButton, CommentRowButton } from "./ui/design/buttons";
import "./comments.css";

export type CommentBoard = { id: string; position: { x: number; y: number }; width?: number; height?: number; hidden?: boolean; clip?: { x: number; y: number; width: number; height: number }; data: { screen?: { name: string } } };
export type PinProps = { pins: CommentPin[]; draft: CommentDraft | null; selected: string | null; boards: Map<string, CommentBoard>; select: (pin: CommentPin) => void; detail: ReactNode };
type Props = { send: (type: string, data?: Record<string, unknown>) => void; requestId: () => string; canvas: RefObject<HTMLDivElement | null>;
  boards: Map<string, CommentBoard>; renderPins: (props: PinProps) => ReactNode; focus: (pin: CommentPin) => boolean; initialSelected?: string | null; crossScreen?: boolean };
type Reply = { type: "comments-result"; request_id: string; comments?: CommentState; error?: string; sent?: boolean };
type Event = Reply | { type: "place"; draft: CommentDraft };

/** The viewport subscription exists only while comments are visible. Panning
 * updates this small layer, never artboard data or the preview render queue. */
export function CommentPins({ pins, draft, selected, boards, select, camera, detail }: PinProps & { camera: { x: number; y: number; zoom: number } }) {
  const marker = (pin: CommentDraft, number: number | null) => {
    const board = boards.get(pin.screen_id);
    if (!board || board.hidden) return null;
    const p = pinPosition(pin, { ...board.position, width: board.width ?? 0, height: board.height ?? 0 }, camera);
    if (board.clip && (p.x < board.clip.x || p.y < board.clip.y || p.x > board.clip.x + board.clip.width || p.y > board.clip.y + board.clip.height)) return null;
    if (p.x < -32 || p.y < -32 || p.x > innerWidth + 32 || p.y > innerHeight + 32) return null;
    return <PinButton key={pin.id} id={`studio-comment-pin-${pin.id}`} style={{ left: p.x, top: p.y }}
      className={`${number === null ? "draft" : ""} ${selected === pin.id ? "selected" : ""}`}
      aria-label={number === null ? "New comment location" : `Comment ${number} on ${board.data.screen?.name ?? "screen"}`}
      aria-pressed={selected === pin.id} disabled={number === null}
      onClick={event => { event.stopPropagation(); if (number !== null) select(pin as CommentPin); }}>
      {number ?? "+"}
    </PinButton>;
  };
  const active = draft ?? pins.find(pin => pin.id === selected);
  const board = active && boards.get(active.screen_id);
  const anchor = active && board && !board.hidden ? pinPosition(active, { ...board.position, width: board.width ?? 0, height: board.height ?? 0 }, camera) : null;
  return <div className="comment-pins" aria-label="Screen comment pins">
    {pins.map((pin, i) => marker(pin, i + 1))}
    {draft && marker(draft, null)}
    {anchor && detail && <CommentPopover anchor={anchor}>{detail}</CommentPopover>}
  </div>;
}

function CommentPopover({ anchor, children }: { anchor: { x: number; y: number }; children: ReactNode }) {
  const element = useRef<HTMLDivElement>(null);
  const [bounds, setBounds] = useState({ width: 320, height: 200, areaWidth: 0, areaHeight: 0 });
  useLayoutEffect(() => {
    const node = element.current, stage = node?.parentElement;
    if (!node || !stage) return;
    const panel = document.querySelector(".comments-panel");
    const measure = () => {
      const rect = stage.getBoundingClientRect(), sidebar = panel?.getBoundingClientRect();
      const areaWidth = sidebar && sidebar.left > rect.left && sidebar.left < rect.right ? sidebar.left - rect.left : rect.width;
      const next = { width: node.offsetWidth, height: node.offsetHeight, areaWidth, areaHeight: rect.height };
      setBounds(previous => Object.keys(next).every(key => previous[key as keyof typeof next] === next[key as keyof typeof next]) ? previous : next);
    };
    const observer = new ResizeObserver(measure);
    observer.observe(node); observer.observe(stage); if (panel) observer.observe(panel);
    measure();
    return () => observer.disconnect();
  }, []);
  const position = commentPopoverPosition(anchor, bounds, { width: bounds.areaWidth, height: bounds.areaHeight });
  const availableWidth = Math.max(120, bounds.areaWidth - 24);
  const besideWidth = Math.max(anchor.x - 24, bounds.areaWidth - anchor.x - 56);
  const maxWidth = Math.min(availableWidth, besideWidth >= 220 ? besideWidth : availableWidth);
  return <div ref={element} className="comment-popover nodrag nopan nowheel" role="dialog" aria-label="Comment"
    style={{ left: position.x, top: position.y, maxWidth, maxHeight: Math.max(120, bounds.areaHeight - 24), visibility: bounds.areaWidth ? "visible" : "hidden" }}
    onClick={event => event.stopPropagation()} onPointerDown={event => event.stopPropagation()}>
    {children}
  </div>;
}

/** Fully unmounted outside Comments mode: no storage reads, timers, or comment
 * subscriptions in Design/Prototype. The host owns durable storage. */
export function CommentsLayer({ send, requestId, canvas, boards, renderPins, focus, initialSelected, crossScreen = false }: Props) {
  const [state, setState] = useState<CommentState | null>(null);
  const [draft, setDraft] = useState<CommentDraft | null>(null);
  const [text, setText] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [hidden, setHidden] = useState(false);
  const [jumpTo, setJumpTo] = useState<CommentPin | null>(null);
  const [busy, setBusy] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [needsReload, setNeedsReload] = useState(false);
  const [notice, setNotice] = useState("");
  const input = useRef<HTMLTextAreaElement>(null);
  const closeButton = useRef<HTMLButtonElement>(null);
  const selectedRow = useRef<HTMLDivElement>(null);
  const expandButton = useRef<HTMLButtonElement>(null);
  const pending = useRef<{ id: string; kind: "read" | "create" | "resolve" | "send" | "focus"; pin?: string; timer: ReturnType<typeof setTimeout> } | null>(null);
  const [sent, setSent] = useState<Set<string>>(() => new Set());
  const unconfirmed = useRef<string | null>(null);
  const initialSelection = useRef(initialSelected);
  const current = useRef({ draft, text, busy, hidden });
  current.current = { draft, text, busy, hidden };
  const pins = state?.pins.filter(pin => !pin.resolved && boards.has(pin.screen_id)) ?? [];
  const dirty = !!draft && text.length > 0;
  const begin = (kind: "read" | "create" | "resolve" | "send" | "focus", data: Record<string, unknown> = {}, pin?: string) => {
    const id = requestId();
    if (pending.current) clearTimeout(pending.current.timer);
    pending.current = { id, kind, pin, timer: setTimeout(() => {
      setBusy(false); setNeedsReload(true);
      setError(kind === "send" ? "Handoff was not confirmed. Check the design assistant before sending again." : kind === "focus" ? "The comment location did not open. Reload comments and try again." : "The comment request was not confirmed. Reload comments to check what was saved.");
    }, 15000) };
    setBusy(true); setError(null); setNotice("");
    send(kind === "read" ? "comments-read" : kind === "send" ? "comment-send" : kind === "focus" ? "comment-focus" : "comment-edit", { request_id: id, ...data });
  };
  useEffect(() => {
    const listener = (event: globalThis.Event) => {
      const value = (event as CustomEvent<Event>).detail;
      if (value.type === "place") {
        const c = current.current;
        if (c.busy) return;
        if (c.draft && c.text.length) { setError("Post or cancel your comment before placing another pin."); return; }
        setDraft(value.draft); setSelected(null); setText(""); setError(null); setNotice("");
        return;
      }
      const request = pending.current;
      if (!request || value.request_id !== request.id) return;
      clearTimeout(request.timer); pending.current = null;
      setBusy(false); setError(value.error ?? null);
      if (value.error && !value.comments) setNeedsReload(true);
      if (value.comments) {
        setState(value.comments); setNeedsReload(false);
        if (request.kind === "read" && initialSelection.current) {
          const target = value.comments.pins.find(pin => pin.id === initialSelection.current && !pin.resolved);
          initialSelection.current = null;
          if (target) { setSelected(target.id); setJumpTo(target); }
        }
      }
      if (value.error) {
        if (request.kind === "send" && request.pin) setSent(previous => { const next = new Set(previous); next.delete(request.pin!); return next; });
        return;
      }
      if (request.kind === "send" && value.sent) setNotice("Sent to the design assistant. Resolve this comment when the fix is complete.");
      const saved = request.kind === "create" ? request.pin : unconfirmed.current;
      if (saved && value.comments?.pins.some(pin => pin.id === saved)) {
        setDraft(null); setText(""); setSelected(saved); setNotice("Comment saved.");
        unconfirmed.current = null;
        send("comment-draft", { dirty: false });
      }
      if (request.kind === "resolve") { setSelected(null); setNotice("Comment resolved."); expandButton.current?.focus(); }
    };
    addEventListener(COMMENT_EVENT, listener);
    begin("read");
    return () => { removeEventListener(COMMENT_EVENT, listener); if (pending.current) clearTimeout(pending.current.timer); };
  }, []);
  useEffect(() => { send("comment-draft", { dirty }); }, [dirty, send]);
  useEffect(() => { if (draft) input.current?.focus(); }, [draft]);
  useEffect(() => { if (selected) closeButton.current?.focus(); }, [selected]);
  useEffect(() => { if (!hidden) selectedRow.current?.scrollIntoView({ block: "nearest" }); }, [selected, hidden]);
  useLayoutEffect(() => {
    if (!jumpTo || !canvas.current) return;
    if (focus(jumpTo)) setJumpTo(null);
  }, [jumpTo, canvas, focus, boards]);
  const cancel = () => { if (draft) expandButton.current?.focus(); setDraft(null); setText(""); setError(null); send("comment-draft", { dirty: false }); };
  const dismiss = () => {
    cancel();
    if (selected) document.getElementById(`studio-comment-pin-${selected}`)?.focus();
    setSelected(null);
  };
  const select = (pin: CommentPin) => {
    if (busy) return false;
    if (dirty) { setError("Post or cancel your comment before opening another pin."); return false; }
    setDraft(null); setSelected(pin.id); setError(null); setNotice("");
    return true;
  };
  const focusPin = (pin: CommentPin) => {
    if (!select(pin)) return;
    if (crossScreen && boards.get(pin.screen_id)?.hidden) begin("focus", { id: pin.id }, pin.id);
    else setJumpTo({ ...pin });
  };
  const selectedPin = pins.find(pin => pin.id === selected);
  const active = draft ?? selectedPin;
  const visibleNote = active && !boards.get(active.screen_id)?.hidden;
  const feedback = <>
    {busy && <p role="status" className="comments-help">{pending.current?.kind === "read" ? "Loading comments…" : pending.current?.kind === "send" ? "Sending…" : pending.current?.kind === "focus" ? "Opening comment…" : "Saving…"}</p>}
    {error && <div role="alert" className="comments-error">{error}
      {(needsReload || !state) && <StageButton disabled={busy} onClick={() => begin("read")}>Reload comments</StageButton>}
    </div>}
    <span role="status" className="comments-status">{notice}</span>
  </>;
  const detail = active && <div className="comment-detail" onKeyDown={event => {
    if (event.key === "Escape") { event.stopPropagation(); if (!busy && !dirty) dismiss(); }
  }}>
    <div className="comment-popover-heading">
      <h3>{draft ? "New comment" : "Comment"}</h3>
      <StageIconButton ref={closeButton} aria-label={draft ? "Cancel comment" : "Close comment"} title={draft ? "Cancel comment" : "Close comment"}
        disabled={busy} onClick={dismiss}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true"><path d="m6 6 12 12M18 6 6 18" /></svg>
      </StageIconButton>
    </div>
    <p className="comment-location">{boards.get(active.screen_id)?.data.screen?.name ?? "Screen"}</p>
    {draft ? <>
      <label className="comment-input-label" htmlFor="studio-comment-body">Your comment</label>
      <textarea id="studio-comment-body" ref={input} value={text} disabled={busy} maxLength={8000}
        placeholder="What needs attention?" onChange={event => setText(event.target.value)} />
      {text.length > 0 && !commentBodyValid(text) && <p className="comments-help">Enter a comment of up to 8000 bytes.</p>}
      <div className="comment-actions">
        <StageButton disabled={busy} onClick={cancel}>Cancel</StageButton>
        <StageButton primary disabled={busy || needsReload || !state || !boards.has(draft.screen_id) || !commentBodyValid(text)} onClick={() => {
          unconfirmed.current = draft.id;
          begin("create", { revision: state!.revision, operation: { operation: "create", ...draft, body: text } }, draft.id);
        }}>Post comment</StageButton>
      </div>
    </> : selectedPin && <>
      <p className="comment-body">{selectedPin.body}</p>
      <div className="comment-actions">
        <StageButton disabled={busy || needsReload || sent.has(selectedPin.id)} onClick={() => {
          setSent(previous => new Set(previous).add(selectedPin.id));
          begin("send", { revision: state!.revision, id: selectedPin.id }, selectedPin.id);
        }}>{sent.has(selectedPin.id) ? "Sent to agent" : "Send to agent"}</StageButton>
        <StageButton primary disabled={busy || needsReload} onClick={() => begin("resolve", {
          revision: state!.revision, operation: { operation: "resolve", id: selectedPin.id },
        }, selectedPin.id)}>Resolve</StageButton>
      </div>
    </>}
    {feedback}
  </div>;
  return <>
    {canvas.current && createPortal(renderPins({ pins, draft, selected, boards, select: pin => { select(pin); }, detail }), canvas.current)}
    <aside className={`comments-panel nodrag nopan nowheel ${hidden ? "collapsed" : ""}`} aria-label="Comments"
      onKeyDown={event => { event.stopPropagation(); if (event.key === "Escape" && draft && !dirty && !busy) cancel(); }}>
      <div className="comments-heading">
        <h2 hidden={hidden}>Comments{state && <span>{pins.length}</span>}</h2>
        <StageIconButton ref={expandButton} aria-label={hidden ? "Expand comments" : "Collapse comments"}
          title={hidden ? "Expand comments" : "Collapse comments"} aria-expanded={!hidden} aria-controls="studio-comment-content"
          onClick={() => setHidden(!hidden)}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
            <rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" />
            <path d={hidden ? "m11 9-3 3 3 3" : "m8 9 3 3-3 3"} />
          </svg>
        </StageIconButton>
        {hidden && state && <span className="comments-collapsed-count" aria-label={`${pins.length} open comments`}>{pins.length}</span>}
        {hidden && dirty && <span className="comments-draft-indicator" title="Unsent comment" aria-label="Unsent comment" />}
      </div>
      {!hidden && <div id="studio-comment-content" className="comments-content">
        <p className="comments-help">Click a screen to comment.</p>
        {!visibleNote && feedback}
        <div className="comment-list">
          {!busy && !pins.length && state && <p className="comments-help">No open comments.</p>}
          {pins.map((pin, i) => {
            const selectedPin = pin.id === selected;
            const createdDate = commentDate(pin.created_at);
            return <div key={pin.id} className={`comment-list-item ${selectedPin ? "selected" : ""}`} ref={selectedPin ? selectedRow : undefined}>
              <CommentRowButton onClick={() => focusPin(pin)} disabled={busy} aria-pressed={selectedPin}>
                <span className="comment-row-title"><span className="comment-row-number">{i + 1}</span>{boards.get(pin.screen_id)?.data.screen?.name ?? "Screen"}</span>
                <span className="comment-row-body">{pin.body}</span>
                {createdDate ? <time dateTime={createdDate.toISOString()} title={createdDate.toLocaleString()}>{createdDate.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })}</time>
                  : <span className="comment-date">Date unavailable</span>}
              </CommentRowButton>
            </div>;
          })}
        </div>
      </div>}
    </aside>
  </>;
}
