import type { Bootstrap, HostMessage, SavedState, VisualTheme } from "./types";

declare global {
  interface Window {
    __CHORO_VISUAL__?: Bootstrap;
    ipc?: { postMessage: (message: string) => void };
    choroVisual?: {
      setMode: (mode: "docked" | "expanded") => void;
      setTheme: (theme: VisualTheme) => void;
      /** Send any pending state now (before the host hides or replaces the surface). */
      flush: () => void;
      /** Move keyboard focus into the content from the native panel chrome. */
      focusEdge: (edge: "first" | "last") => void;
      /** Reply immediately with the complete current state and this token. */
      snapshot: (token: number) => void;
    };
  }
}

/** Every message carries the answer identity and surface generation so the host
 *  can drop anything from a stale document or an earlier surface. */
export function post(boot: Pick<Bootstrap, "surface" | "answerId" | "revision">, message: HostMessage): void {
  try {
    window.ipc?.postMessage(
      JSON.stringify({ ...message, surface: boot.surface, answerId: boot.answerId, revision: boot.revision }),
    );
  } catch {
    // The host is gone (surface torn down); nothing to report to.
  }
}

/** Coalesces frequent state changes (typing, scrolling) into one message.
 *  Discrete choices (selection, sorting, series toggles) are sent at once so a
 *  surface replaced moments later cannot lose them. */
export function stateReporter(send: (state: SavedState) => void, delay = 250) {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let latest: SavedState | null = null;
  const flush = () => {
    if (timer) clearTimeout(timer);
    timer = null;
    if (latest) send(latest);
    latest = null;
  };
  return {
    report(state: SavedState, immediate = false) {
      latest = state;
      if (timer) clearTimeout(timer);
      if (immediate) flush();
      else timer = setTimeout(flush, delay);
    },
    flush,
    /** Drop a pending report; a snapshot supersedes it. */
    cancel() {
      if (timer) clearTimeout(timer);
      timer = null;
      latest = null;
    },
  };
}

export function applyTheme(theme: VisualTheme): void {
  const root = document.documentElement;
  const set = (name: string, value: string) => root.style.setProperty(name, value);
  root.dataset.theme = theme.dark ? "dark" : "light";
  root.style.colorScheme = theme.dark ? "dark" : "light";
  const tokens: Record<string, string> = {
    "--va-base": theme.base,
    "--va-surface": theme.surface,
    "--va-surface-2": theme.surface2,
    "--va-stage": theme.surface2,
    "--va-line": theme.line,
    "--va-line-2": theme.line2,
    "--va-t1": theme.t1,
    "--va-t2": theme.t2,
    "--va-t3": theme.t3,
    "--va-accent": theme.accent,
    "--va-accent-soft": theme.accentSoft,
    "--va-on-accent": theme.onAccent,
    "--va-focus": theme.focus,
    "--va-hover": theme.hover,
    "--va-sage": theme.sage,
    "--va-amber": theme.amber,
    "--va-rose": theme.rose,
    // OpenUI's components read these variables.
    "--openui-background": theme.base,
    "--openui-foreground": theme.surface,
    "--openui-popover-background": theme.surface,
    "--openui-sunk": theme.surface2,
    "--openui-sunk-light": theme.surface2,
    "--openui-highlight-subtle": theme.hover,
    "--openui-highlight": theme.hover,
    "--openui-text-neutral-primary": theme.t1,
    "--openui-text-neutral-secondary": theme.t2,
    "--openui-text-neutral-tertiary": theme.t3,
    "--openui-border-default": theme.line,
    "--openui-border-interactive": theme.line2,
    "--openui-border-interactive-emphasis": theme.focus,
    "--openui-border-accent-emphasis": theme.focus,
    "--openui-text-success-primary": theme.sage,
    "--openui-text-danger-primary": theme.rose,
    "--openui-text-alert-primary": theme.amber,
    "--openui-text-info-primary": theme.sky,
  };
  for (const [name, value] of Object.entries(tokens)) set(name, value);
}

export function seriesPalette(theme: VisualTheme): string[] {
  return [theme.accent, theme.sky, theme.sage, theme.amber, theme.rose, theme.teal];
}
