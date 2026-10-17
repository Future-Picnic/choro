import { Renderer, type OpenUIError } from "@openuidev/react-lang";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { applyTheme, post, seriesPalette, stateReporter } from "./bridge";
import { AnswerContext, type AnswerContextValue } from "./context";
import { createDatasetReader } from "./data";
import { library } from "./library";
import { sanitizeViews, summarizeView, viewTargets } from "./summary";
import { sanitizeStageScroll, type StageScroll } from "./compare";
import type { Bootstrap, ComponentView, SavedState, SurfaceMode, VisualTheme } from "./types";
import { elements, validateProgram } from "./validate";
import { Sources } from "./components/Sources";

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), [tabindex="0"], a[href]';

// jsdom performs no layout, so every element there has a null offsetParent.
const NO_LAYOUT = typeof navigator !== "undefined" && navigator.userAgent.includes("jsdom");

function focusable(): HTMLElement[] {
  return [...document.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => NO_LAYOUT || el.offsetParent !== null || el === document.activeElement,
  );
}

export function App({ boot }: { boot: Bootstrap }) {
  const { document: answer } = boot;
  const [mode, setMode] = useState<SurfaceMode>(boot.mode);
  const [theme, setTheme] = useState<VisualTheme>(boot.theme);
  const reader = useMemo(() => createDatasetReader(answer.datasets), [answer]);
  const validation = useMemo(() => validateProgram(answer.openui_lang, reader, answer), [answer, reader]);
  const targets = useMemo(() => viewTargets(elements(validation.result?.root)), [validation]);
  const [selected, setSelected] = useState<string | null>(
    answer.options.some((o) => o.id === boot.state.selectedOptionId) ? boot.state.selectedOptionId : null,
  );
  const [views, setViews] = useState<Record<string, ComponentView>>(() =>
    sanitizeViews(boot.state.fields?.view, reader, targets),
  );
  const openuiState = useRef<Record<string, unknown>>(boot.state.fields?.openui ?? {});
  // Preview scroll is read synchronously by snapshots (a final snapshot can
  // arrive in the same tick as the scroll), so it lives in a ref, not state.
  const stageScroll = useRef<StageScroll>(
    sanitizeStageScroll(
      boot.state.fields?.stages,
      answer.options.map((o) => o.id),
    ),
  );
  const [renderErrors, setRenderErrors] = useState<string[]>([]);
  const scroller = useRef<HTMLElement | null>(null);
  const reporter = useMemo(() => stateReporter((state) => post(boot, { type: "state", state })), [boot]);

  const snapshot = useCallback(
    (next: { selectedOptionId?: string | null; views?: Record<string, ComponentView> } = {}): SavedState => {
      const selectedOptionId = next.selectedOptionId !== undefined ? next.selectedOptionId : selected;
      const view = next.views ?? views;
      return {
        selectedOptionId,
        fields: {
          openui: openuiState.current,
          view,
          summary: summarizeView(answer, reader, targets, view, selectedOptionId),
          stages: { ...stageScroll.current },
        },
        scrollTop: Math.max(0, Math.round(scroller.current?.scrollTop ?? 0)),
      };
    },
    [selected, views, answer, reader, targets],
  );

  // Always the newest state, for host snapshot requests between renders.
  const latestSnapshot = useRef(snapshot);
  latestSnapshot.current = snapshot;

  useLayoutEffect(() => applyTheme(theme), [theme]);

  // Restore scroll after the first layout, then tell the host the surface is live.
  useLayoutEffect(() => {
    scroller.current = (document.scrollingElement ?? document.documentElement) as HTMLElement | null;
    if (scroller.current) scroller.current.scrollTop = boot.state.scrollTop;
    post(boot, { type: "ready" });
    if (!validation.ok) post(boot, { type: "invalid", errors: validation.errors });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    window.choroVisual = {
      setMode,
      setTheme,
      flush: () => reporter.flush(),
      // The host asks before hiding/replacing this surface or sending: reply
      // at once with the complete state (no debounce), echoing the token.
      snapshot: (token) => {
        reporter.cancel();
        post(boot, { type: "snapshot", token, state: latestSnapshot.current() });
      },
      focusEdge: (edge) => {
        const items = focusable();
        (edge === "first" ? items[0] : items[items.length - 1])?.focus();
      },
    };
    return () => {
      delete window.choroVisual;
    };
  }, [reporter, boot]);

  useEffect(() => {
    const onScroll = () => reporter.report(snapshot());
    const onHide = () => reporter.flush();
    window.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("pagehide", onHide);
    return () => {
      window.removeEventListener("scroll", onScroll);
      window.removeEventListener("pagehide", onHide);
    };
  }, [reporter, snapshot]);

  // Report the content's height so a docked panel can fit it. Only this
  // document is measured; design frames are sandboxed and never inspected.
  const root = useRef<HTMLElement | null>(null);
  useLayoutEffect(() => {
    const element = root.current;
    if (!element) return;
    let last = 0;
    const report = () => {
      const height = Math.ceil(element.getBoundingClientRect().height);
      if (height > 0 && Math.abs(height - last) >= 1) {
        last = height;
        post(boot, { type: "size", height });
      }
    };
    report();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(report);
    observer.observe(element);
    return () => observer.disconnect();
  }, [boot]);

  // The content is one region of a native panel. Tab past either end hands
  // focus to the panel's native header or composer instead of trapping it.
  // Escape is decided natively, exactly as in the composer: stop running work
  // first, otherwise dock an expanded answer, otherwise leave the answer.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        // A held key repeats; one press is one step.
        if (event.repeat) return;
        reporter.flush();
        post(boot, { type: "escape" });
      } else if (event.key === "Tab") {
        const items = focusable();
        const active = document.activeElement;
        const atEnd = items.length === 0 || active === items[items.length - 1];
        const atStart = items.length === 0 || active === items[0];
        if (event.shiftKey ? atStart : atEnd) {
          event.preventDefault();
          reporter.flush();
          post(boot, { type: "focusExit", direction: event.shiftKey ? "backward" : "forward" });
        }
      }
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [boot, reporter]);

  const context: AnswerContextValue = useMemo(
    () => ({
      reader,
      document: answer,
      palette: seriesPalette(theme),
      readOnly: boot.readOnly,
      expanded: mode === "expanded",
      selectedOptionId: selected,
      select: (optionId) => {
        if (boot.readOnly || !answer.options.some((o) => o.id === optionId)) return;
        setSelected(optionId);
        reporter.report(snapshot({ selectedOptionId: optionId }), true);
      },
      view: (key) => views[key] ?? {},
      setView: (key, patch) => {
        const next = { ...views, [key]: { ...views[key], ...patch } };
        setViews(next);
        // Typing coalesces; sorting and toggles are saved at once.
        reporter.report(snapshot({ views: next }), !("search" in patch));
      },
      stageScroll: (optionId) => stageScroll.current[optionId] ?? 0,
      setStageScroll: (optionId, top) => {
        if (!answer.options.some((o) => o.id === optionId)) return;
        const next = sanitizeStageScroll({ ...stageScroll.current, [optionId]: top }, answer.options.map((o) => o.id));
        if (next[optionId] === stageScroll.current[optionId]) return;
        stageScroll.current = next;
        // Scrolling coalesces like page scroll; a snapshot always has the latest.
        reporter.report(latestSnapshot.current());
      },
    }),
    [reader, answer, theme, boot.readOnly, mode, selected, views, reporter, snapshot],
  );

  const onError = useCallback(
    (errors: OpenUIError[]) => {
      const messages = errors.filter((e) => e.severity !== "warning").map((e) => e.message);
      setRenderErrors(messages);
      if (messages.length) post(boot, { type: "invalid", errors: messages });
    },
    [boot],
  );

  const problems = validation.ok ? renderErrors : validation.errors;
  return (
    <main ref={root} className={`va-root va-${mode}`} aria-label={answer.title}>
      {problems.length > 0 ? (
        <div className="va-invalid" role="alert">
          <p className="va-invalid-title">This visual answer can’t be shown.</p>
          <p className="va-invalid-body">Its program didn’t pass Choro’s checks, so nothing from it was rendered. You can keep chatting or ask the agent to correct it.</p>
          <ul className="va-invalid-list">
            {problems.slice(0, 8).map((problem) => (
              <li key={problem}>{problem}</li>
            ))}
          </ul>
        </div>
      ) : (
        <AnswerContext.Provider value={context}>
          <Renderer
            response={answer.openui_lang}
            library={library}
            initialState={openuiState.current}
            onStateUpdate={(state) => {
              openuiState.current = state;
              reporter.report(snapshot());
            }}
            onError={onError}
            toolProvider={null}
          />
        </AnswerContext.Provider>
      )}
      <Sources sources={answer.sources} />
    </main>
  );
}
