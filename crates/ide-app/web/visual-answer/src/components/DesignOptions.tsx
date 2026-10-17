import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { planComparison, type ComparisonPlan, type DesignScale } from "../compare";
import { useAnswer } from "../context";
import type { VisualOption } from "../types";
import { Problem } from "./Problem";

// Each design renders in a sandboxed frame: no scripts, no same-origin access,
// no network, no forms, and no navigation. It can never reach the host page.
const FRAME_POLICY =
  "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; form-action 'none'; base-uri 'none'";

// The frame itself never scrolls (it can't receive the pointer, so its own
// scrollbar would be unusable). It is sized to the design; the preview around
// it scrolls in this document instead.
export function designDocument(html: string, background: string, foreground: string): string {
  const head = `<meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${FRAME_POLICY}"><style>html,body{margin:0;min-height:100%;overflow:hidden}body{display:grid;place-items:center;min-height:100vh;background:${background};color:${foreground};font:13px/1.4 -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}</style>`;
  const lower = html.toLowerCase();
  const headAt = lower.indexOf("<head");
  if (headAt >= 0) {
    const close = html.indexOf(">", headAt);
    if (close >= 0) return html.slice(0, close + 1) + head + html.slice(close + 1);
  }
  return `<!doctype html><html><head>${head}</head><body>${html}</body></html>`;
}

/** Laid-out width of an element, following resizes where the platform can. */
function useWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    setWidth(element.clientWidth);
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

function useWindowHeight(active: boolean): number {
  const [height, setHeight] = useState(() => window.innerHeight);
  useEffect(() => {
    if (!active) return;
    const update = () => setHeight(window.innerHeight);
    update();
    window.addEventListener("resize", update);
    return () => window.removeEventListener("resize", update);
  }, [active]);
  return height;
}

interface StageProps {
  option: VisualOption;
  plan: ComparisonPlan;
  stage: string;
  ink: string;
  register: (element: HTMLDivElement | null) => void;
}

/** One design preview. Taller than its slot, it scrolls here with a visible
 *  scrollbar and a quiet cue; the frame stays inert so a click still selects.
 *  Its position is saved in design pixels and restored after every layout
 *  (first mount, expand/dock), clamped to what the current preview can show. */
function OptionStage({ option, plan, stage, ink, register }: StageProps) {
  const answer = useAnswer();
  const viewport = useRef<HTMLDivElement | null>(null);
  // The position this component last set; the scroll event it causes (and
  // any clamp from a smaller layout) is not a user scroll and is not saved.
  const applied = useRef<number | null>(null);
  const [atEnd, setAtEnd] = useState(false);
  const updateCue = (el: HTMLDivElement) => setAtEnd(el.scrollTop + el.clientHeight >= el.scrollHeight - 2);

  useLayoutEffect(() => {
    const el = viewport.current;
    if (!el) return;
    const max = Math.max(0, plan.canvasHeight - plan.visibleHeight);
    const top = Math.min(max, Math.round(answer.stageScroll(option.id) * plan.zoom));
    el.scrollTop = top;
    applied.current = el.scrollTop;
    // From the plan: the DOM may not have laid out the new geometry yet.
    setAtEnd(top >= max - 2);
    // Restore only when the preview's geometry changes, never on a re-render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [option.id, plan.canvasHeight, plan.visibleHeight, plan.zoom]);

  const onScroll = () => {
    const el = viewport.current;
    if (!el) return;
    updateCue(el);
    if (applied.current !== null && Math.abs(el.scrollTop - applied.current) < 1) return;
    applied.current = null;
    answer.setStageScroll(option.id, el.scrollTop / (plan.zoom || 1));
  };
  return (
    <div className={`va-stage${plan.overflow ? " va-stage-scrolls" : ""}`}>
      <div
        ref={(el) => {
          viewport.current = el;
          register(el);
        }}
        className="va-stage-viewport"
        style={{ height: plan.visibleHeight }}
        onScroll={onScroll}
      >
        <div className="va-stage-canvas" style={{ height: plan.canvasHeight }}>
          <iframe
            className="va-option-stage"
            title={`${option.title} preview`}
            sandbox=""
            referrerPolicy="no-referrer"
            loading="lazy"
            tabIndex={-1}
            aria-hidden="true"
            style={{
              width: plan.frameWidth,
              height: plan.frameHeight,
              transform: plan.zoom < 1 ? `scale(${plan.zoom})` : undefined,
            }}
            srcDoc={designDocument(option.html, stage, ink)}
          />
        </div>
      </div>
      {plan.overflow && !atEnd && (
        <span className="va-stage-more" aria-hidden="true">
          Scroll for more
        </span>
      )}
    </div>
  );
}

export interface DesignOptionsProps {
  scale?: DesignScale;
  height?: number;
  width?: number;
}

export function DesignOptionsView({ scale, height, width }: DesignOptionsProps) {
  const answer = useAnswer();
  const [group, containerWidth] = useWidth<HTMLDivElement>();
  const windowHeight = useWindowHeight(answer.expanded);
  const stages = useRef(new Map<string, HTMLDivElement>());
  const options = answer.document.options;
  if (options.length < 2) return <Problem text="Design choices need two to four alternatives." />;
  const plan = planComparison({
    count: options.length,
    containerWidth,
    scale,
    height,
    width,
    expanded: answer.expanded,
    viewportHeight: windowHeight,
  });
  const selected = answer.selectedOptionId;
  const focusIndex = Math.max(0, options.findIndex((o) => o.id === selected));
  const styles = getComputedStyle(document.documentElement);
  const stage = styles.getPropertyValue("--va-stage").trim() || "#fff";
  const ink = styles.getPropertyValue("--va-t1").trim() || "#000";

  const choose = (option: VisualOption) => {
    if (!answer.readOnly) answer.select(option.id);
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>, index: number) => {
    const step = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[event.key];
    const page = { PageDown: 1, PageUp: -1 }[event.key];
    if (event.key === " " || event.key === "Enter") {
      event.preventDefault();
      choose(options[index]);
    } else if (step) {
      event.preventDefault();
      const next = (index + step + options.length) % options.length;
      group.current?.querySelectorAll<HTMLElement>("[role=radio]")[next]?.focus();
      choose(options[next]);
    } else if (page) {
      // Read a tall design from the keyboard without changing the selection.
      const viewport = stages.current.get(options[index].id);
      if (viewport) {
        event.preventDefault();
        viewport.scrollBy({ top: page * Math.max(40, viewport.clientHeight * 0.8) });
      }
    }
  };

  return (
    <div className="va-options-wrap">
      <div
        ref={group}
        className={`va-options va-options-${plan.scale}`}
        style={{ gridTemplateColumns: `repeat(${plan.columns}, minmax(0, 1fr))` }}
        role="radiogroup"
        aria-label="Design alternatives"
        aria-disabled={answer.readOnly || undefined}
        data-columns={plan.columns}
      >
        {options.map((option, index) => {
          const checked = option.id === selected;
          return (
            <div
              key={option.id}
              role="radio"
              aria-checked={checked}
              aria-label={`${option.title}. ${option.description}`}
              aria-keyshortcuts={plan.overflow ? "PageDown PageUp" : undefined}
              tabIndex={index === focusIndex ? 0 : -1}
              className={`va-option${checked ? " va-option-selected" : ""}`}
              data-option={option.id}
              onClick={() => choose(option)}
              onKeyDown={(event) => onKeyDown(event, index)}
            >
              <div className="va-option-head">
                <span className="va-option-title">{option.title}</span>
                <span className="va-radio" aria-hidden="true" />
              </div>
              <OptionStage
                option={option}
                plan={plan}
                stage={stage}
                ink={ink}
                register={(element) => {
                  if (element) stages.current.set(option.id, element);
                  else stages.current.delete(option.id);
                }}
              />
              {option.description && <p className="va-option-description">{option.description}</p>}
            </div>
          );
        })}
      </div>
      {plan.overflow && !answer.expanded && (
        <p className="va-options-hint">Scroll a preview to see all of it, or expand the answer for a larger view.</p>
      )}
    </div>
  );
}
