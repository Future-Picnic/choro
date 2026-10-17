import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { boot, designDocument, missingDocument, projectsDocument } from "./test-fixtures";
import type { Bootstrap, SavedState } from "./types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;
// jsdom has no layout or canvas, so react-ui's charts are replaced by probes
// that record exactly what Choro passes them.
const chartCalls = vi.hoisted(() => [] as { kind: string; props: Record<string, unknown> }[]);
vi.mock("@openuidev/react-ui/Charts", () => ({
  LineChart: (props: Record<string, unknown>) => {
    chartCalls.push({ kind: "line", props });
    return <div data-react-ui-chart="line" />;
  },
  BarChart: (props: Record<string, unknown>) => {
    chartCalls.push({ kind: "bar", props });
    return <div data-react-ui-chart="bar" />;
  },
}));

let container: HTMLDivElement;
let root: Root;
let sent: Record<string, unknown>[];

beforeEach(() => {
  vi.useFakeTimers();
  sent = [];
  window.ipc = { postMessage: (message) => sent.push(JSON.parse(message)) };
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
});

function render(value: Bootstrap) {
  act(() => root.render(<App boot={value} />));
}
function flush() {
  act(() => {
    vi.advanceTimersByTime(1000);
  });
}
const rows = () => [...container.querySelectorAll("tbody tr")].map((tr) => tr.querySelector("td")?.textContent);
function type(input: HTMLInputElement, value: string) {
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    setter.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("visual answer renderer", () => {
  it("renders the real OpenUI program with the initial sort and visible provenance", () => {
    render(boot(projectsDocument));
    expect(rows()).toEqual(["Cinder", "Atlas", "Ember", "Beacon", "Fjord", "Delta"]);
    expect(container.querySelector('[aria-sort="descending"]')?.textContent).toContain("Open issues");
    expect(container.querySelector(".va-missing")?.textContent).toContain("Missing");
    expect(container.querySelector(".va-sources")?.textContent).toContain("Sample data");
    expect(sent.map((m) => m.type)).toEqual(["ready"]);
    expect(sent[0]).toMatchObject({ surface: 7, revision: 1, answerId: boot(projectsDocument).answerId });
  });

  it("searches and sorts locally, reporting only state, never a turn", () => {
    render(boot(projectsDocument));
    type(container.querySelector<HTMLInputElement>("input[type=search]")!, "jon");
    expect(rows()).toEqual(["Ember", "Beacon"]);
    const owner = [...container.querySelectorAll<HTMLButtonElement>(".va-sort")].find((b) => b.textContent?.includes("Owner"))!;
    act(() => owner.click());
    flush();
    expect(new Set(sent.map((m) => m.type))).toEqual(new Set(["ready", "state"]));
    const state = sent.filter((m) => m.type === "state").at(-1)!.state as { fields: { view: Record<string, unknown> } };
    expect(state.fields.view["s:projects"]).toMatchObject({ search: "jon", sortKey: "owner", sortDirection: "ascending" });
  });

  it("restores saved filters and selection from host state", () => {
    render(
      boot(projectsDocument, {
        state: { selectedOptionId: null, fields: { view: { "s:projects": { search: "maya" } } }, scrollTop: 0 },
      }),
    );
    expect(rows()).toEqual(["Cinder", "Atlas"]);
    expect(container.querySelector<HTMLInputElement>("input[type=search]")!.value).toBe("maya");
  });

  it("shows a useful empty result without inventing rows", () => {
    const empty = structuredClone(projectsDocument);
    empty.datasets.projects.rows = [];
    render(boot(empty));
    expect(rows()).toEqual([]);
    expect(container.textContent).toContain("This data has no rows yet.");
  });

  it("marks missing chart values instead of plotting zero", () => {
    render(boot(missingDocument, { mode: "expanded" }));
    expect(container.querySelector(".va-gap-chart")).not.toBeNull();
    expect(container.textContent).toContain("Missing for Tuesday");
    // Two segments around the gap; no point at Tuesday.
    expect(container.querySelectorAll(".va-gap-chart path")).toHaveLength(2);
    expect(container.querySelectorAll(".va-gap-chart circle")).toHaveLength(2);
    expect(rows()).toEqual(["Monday", "Tuesday", "Wednesday"]);
  });

  it("draws complete data with react-ui charts and toggles series locally", () => {
    const trend = structuredClone(missingDocument);
    trend.datasets.values.columns.push({ key: "other", label: "Other", kind: "number", unit: "items" });
    trend.datasets.values.rows = [
      { day: "Monday", value: 10, other: 4 },
      { day: "Tuesday", value: 12, other: 6 },
    ];
    trend.datasets.values.series = ["value", "other"];
    trend.openui_lang = `root = Answer([line, bars])\nline = TrendChart("values", "day")\nbars = CategoryChart("values", "day", ["value"])`;
    render(boot(trend));
    expect(container.querySelector(".va-gap-chart")).toBeNull();
    expect(container.querySelector('[data-react-ui-chart="line"]')).not.toBeNull();
    expect(container.querySelector('[data-react-ui-chart="bar"]')).not.toBeNull();
    const line = chartCalls.find((c) => c.kind === "line")!.props;
    expect(line.categoryKey).toBe("Day");
    expect(line.data).toEqual([
      { Day: "Monday", Value: 10, Other: 4 },
      { Day: "Tuesday", Value: 12, Other: 6 },
    ]);
    expect(line.customPalette).toEqual(["#b6a6f2", "#7fb2e0"]);
    const toggle = container.querySelector<HTMLButtonElement>(".va-series[aria-pressed]")!;
    act(() => toggle.click());
    flush();
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    const state = sent.filter((m) => m.type === "state").at(-1)!.state as { fields: { view: Record<string, { hiddenSeries?: string[] }> } };
    expect(state.fields.view["s:line"].hiddenSeries).toEqual(["value"]);
    expect(container.textContent).toContain("Values in items");
  });

  it("selects a design without sending, and isolates each design frame", () => {
    render(boot(designDocument));
    const options = container.querySelectorAll<HTMLElement>("[role=radio]");
    expect(options).toHaveLength(4);
    const frame = container.querySelector("iframe")!;
    expect(frame.getAttribute("sandbox")).toBe("");
    expect(frame.getAttribute("srcdoc")).toContain("Content-Security-Policy");
    act(() => options[2].click());
    flush();
    expect(options[2].getAttribute("aria-checked")).toBe("true");
    expect(sent.filter((m) => m.type === "state").at(-1)).toMatchObject({ state: { selectedOptionId: "tonal" } });
    expect(sent.some((m) => !["ready", "state"].includes(String(m.type)))).toBe(false);
    expect(document.body.dataset.pwned).toBeUndefined();
  });

  it("lays out page comparisons at their natural size and scrolls them without selecting or sending", () => {
    const pages = {
      ...designDocument,
      options: designDocument.options.slice(0, 2),
      openui_lang: `root = Answer([choices])\nchoices = DesignOptions("page", 2400)`,
    };
    render(boot(pages));
    const group = container.querySelector<HTMLElement>("[role=radiogroup]")!;
    // jsdom has no layout: the plan uses its 640px fallback, wide enough for two pages.
    expect(group.dataset.columns).toBe("2");
    const frame = container.querySelector<HTMLIFrameElement>("iframe")!;
    expect(frame.style.width).toBe("1280px");
    expect(frame.style.height).toBe("2400px");
    expect(frame.getAttribute("srcdoc")).toContain("overflow:hidden");
    const viewport = container.querySelector<HTMLElement>(".va-stage-viewport")!;
    expect(viewport.style.height).toBe("320px");
    expect(container.querySelector(".va-stage-more")?.textContent).toBe("Scroll for more");
    expect(container.textContent).toContain("expand the answer for a larger view");
    const scrolled: number[] = [];
    viewport.scrollBy = ((options: ScrollToOptions) => scrolled.push(options.top ?? 0)) as typeof viewport.scrollBy;
    const radio = container.querySelector<HTMLElement>("[role=radio]")!;
    act(() => {
      radio.dispatchEvent(new KeyboardEvent("keydown", { key: "PageDown", bubbles: true }));
    });
    expect(scrolled).toHaveLength(1);
    flush();
    expect(radio.getAttribute("aria-checked")).toBe("false");
    expect(sent.some((m) => m.type === "state")).toBe(false);
  });

  it("keeps each preview's scroll through snapshots, remounts, expand and dock", () => {
    // jsdom performs no scrolling; give elements a stored scrollTop.
    const positions = new WeakMap<Element, number>();
    const original = Object.getOwnPropertyDescriptor(Element.prototype, "scrollTop")!;
    Object.defineProperty(Element.prototype, "scrollTop", {
      configurable: true,
      get() {
        return positions.get(this) ?? 0;
      },
      set(value: number) {
        positions.set(this, value);
      },
    });
    try {
      const pages = {
        ...designDocument,
        options: designDocument.options.slice(0, 2),
        openui_lang: `root = Answer([choices])\nchoices = DesignOptions("page")`,
      };
      render(boot(pages));
      const viewports = () => [...container.querySelectorAll<HTMLElement>(".va-stage-viewport")];
      // jsdom has no layout: the 640px fallback gives two 315px columns.
      const zoom = Math.floor((640 - 10) / 2 - 14) / 1280;
      const scroll = (index: number, top: number) => {
        const el = viewports()[index];
        el.scrollTop = top;
        act(() => {
          el.dispatchEvent(new Event("scroll"));
        });
      };
      scroll(0, 47);
      scroll(1, 94);
      // A host snapshot in the same tick already carries both positions.
      act(() => window.choroVisual!.snapshot(5));
      const reply = sent.find((m) => m.type === "snapshot") as { state: SavedState };
      const stages = reply.state.fields.stages!;
      expect(stages.solid).toBe(Math.round(47 / zoom));
      expect(stages.outline).toBe(Math.round(94 / zoom));
      expect(sent.some((m) => m.type === "state" && (m.state as SavedState).selectedOptionId)).toBe(false);

      // Reopened (detach/reopen, chat switch, restart): restored, stale ids dropped.
      act(() => root.unmount());
      root = createRoot(container);
      sent.length = 0;
      render(boot(pages, { state: { ...reply.state, fields: { ...reply.state.fields, stages: { ...stages, removed: 300, outline: -5 } } } }));
      expect(viewports()[0].scrollTop).toBe(47);
      expect(viewports()[1].scrollTop).toBe(0);
      act(() => window.choroVisual!.snapshot(6));
      const restored = (sent.find((m) => m.type === "snapshot") as { state: SavedState }).state.fields.stages;
      expect(restored).toEqual({ solid: stages.solid });

      // Expanded the whole 2000px page fits, so it shows from the top, but the
      // saved position is not overwritten and returns when docked again.
      act(() => window.choroVisual!.setMode("expanded"));
      expect(viewports()[0].scrollTop).toBe(0);
      act(() => window.choroVisual!.setMode("docked"));
      expect(viewports()[0].scrollTop).toBe(47);
      sent.length = 0;
      act(() => window.choroVisual!.snapshot(7));
      expect((sent.find((m) => m.type === "snapshot") as { state: SavedState }).state.fields.stages).toEqual({
        solid: stages.solid,
      });
    } finally {
      Object.defineProperty(Element.prototype, "scrollTop", original);
    }
  });

  it("refuses size hints outside the supported range", () => {
    const wrong = { ...designDocument, openui_lang: `root = Answer([choices])\nchoices = DesignOptions("section", 20000)` };
    render(boot(wrong));
    expect(container.querySelector("[role=alert]")?.textContent).toContain("height must be 60–6000");
  });

  it("ignores selection on a submitted (read-only) answer", () => {
    render(boot(designDocument, { readOnly: true, state: { selectedOptionId: "solid", fields: {}, scrollTop: 0 } }));
    const options = container.querySelectorAll<HTMLElement>("[role=radio]");
    act(() => options[1].click());
    flush();
    expect(options[0].getAttribute("aria-checked")).toBe("true");
    expect(sent.some((m) => m.type === "state")).toBe(false);
  });

  it("refuses an invalid program and tells the host", () => {
    const broken = { ...projectsDocument, openui_lang: `root = Answer([x])\nx = Frame("https://example.com")` };
    render(boot(broken));
    expect(container.querySelector("[role=alert]")?.textContent).toContain("can’t be shown");
    expect(container.querySelector("table")).toBeNull();
    expect(sent.find((m) => m.type === "invalid")).toBeTruthy();
  });

  it("hands every Escape press to the host once, docked or expanded", () => {
    for (const mode of ["expanded", "docked"] as const) {
      sent.length = 0;
      render(boot(projectsDocument, { mode }));
      act(() => {
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
        // Key repeat from a held Escape is not a second press.
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", repeat: true, bubbles: true }));
      });
      const escapes = sent.filter((m) => m.type === "escape");
      expect(escapes).toHaveLength(1);
      expect(escapes[0]).toMatchObject({ surface: 7, revision: 1 });
      expect(sent.some((m) => m.type === "dock" || m.type === "focusExit")).toBe(false);
      act(() => root.unmount());
      root = createRoot(container);
    }
  });

  it("reports its content height so a docked panel can fit it", () => {
    const observers: Array<() => void> = [];
    const original = globalThis.ResizeObserver;
    globalThis.ResizeObserver = class {
      constructor(callback: () => void) {
        observers.push(callback);
      }
      observe() {}
      disconnect() {}
      unobserve() {}
    } as unknown as typeof ResizeObserver;
    let height = 212.4;
    const rect = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
      () => ({ height }) as DOMRect,
    );
    try {
      render(boot(designDocument));
      expect(sent.filter((m) => m.type === "size").at(-1)).toMatchObject({ height: 213, surface: 7 });
      const before = sent.length;
      act(() => observers.forEach((notify) => notify()));
      expect(sent.length).toBe(before);
      height = 300;
      act(() => observers.forEach((notify) => notify()));
      expect(sent.filter((m) => m.type === "size").at(-1)).toMatchObject({ height: 300 });
    } finally {
      rect.mockRestore();
      globalThis.ResizeObserver = original;
    }
  });

  it("hands keyboard focus to the native composer instead of trapping it", () => {
    render(boot(projectsDocument));
    const items = [...container.querySelectorAll<HTMLElement>("input, button")];
    act(() => items.at(-1)!.focus());
    const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    act(() => {
      document.dispatchEvent(tab);
    });
    expect(tab.defaultPrevented).toBe(true);
    expect(sent.at(-1)).toMatchObject({ type: "focusExit", direction: "forward" });
    act(() => items[0].focus());
    const back = new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true });
    act(() => {
      document.dispatchEvent(back);
    });
    expect(sent.at(-1)).toMatchObject({ type: "focusExit", direction: "backward" });
    // Inside the content, Tab moves normally.
    act(() => items[0].focus());
    const inner = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
    act(() => {
      document.dispatchEvent(inner);
    });
    expect(inner.defaultPrevented).toBe(false);
  });

  it("saves sorting at once with a readable view summary for follow-ups", () => {
    render(boot(projectsDocument));
    type(container.querySelector<HTMLInputElement>("input[type=search]")!, "maya");
    const status = [...container.querySelectorAll<HTMLButtonElement>(".va-sort")].find((b) => b.textContent?.includes("Status"))!;
    act(() => status.click());
    // No timer advance: the sort is reported immediately, carrying the search.
    const state = sent.filter((m) => m.type === "state").at(-1)!.state as { fields: { summary: string } };
    expect(state.fields.summary).toBe("Table “projects”: search “maya” (2 of 6 rows shown); sorted by Status ascending");
  });

  it("hands over the latest search keystroke at once when the host asks (switch or send)", () => {
    render(boot(projectsDocument));
    type(container.querySelector<HTMLInputElement>("input[type=search]")!, "jo");
    type(container.querySelector<HTMLInputElement>("input[type=search]")!, "jon");
    // No timer advance: the debounced report has not been sent yet.
    expect(sent.some((m) => m.type === "state")).toBe(false);
    act(() => window.choroVisual!.snapshot(7));
    const reply = sent.at(-1)!;
    expect(reply).toMatchObject({ type: "snapshot", token: 7, surface: 7, revision: 1 });
    const state = reply.state as { fields: { view: Record<string, { search?: string }>; summary: string } };
    expect(state.fields.view["s:projects"].search).toBe("jon");
    expect(state.fields.summary).toContain("search “jon” (2 of 6 rows shown)");
    // The superseded debounced report is not sent afterwards with older state.
    flush();
    expect(sent.filter((m) => m.type === "state")).toHaveLength(0);
  });

  it("includes the latest scroll position and selection in an immediate snapshot", () => {
    render(boot(designDocument));
    const options = container.querySelectorAll<HTMLElement>("[role=radio]");
    act(() => options[1].click());
    // jsdom has no layout; stand in for a scrolled document.
    const page = document.documentElement;
    Object.defineProperty(page, "scrollTop", { configurable: true, value: 64 });
    act(() => {
      window.dispatchEvent(new Event("scroll"));
    });
    act(() => window.choroVisual!.snapshot(8));
    delete (page as { scrollTop?: number }).scrollTop;
    expect(sent.at(-1)).toMatchObject({
      type: "snapshot",
      token: 8,
      state: { selectedOptionId: "outline", scrollTop: 64 },
    });
  });

  it("drops restored view state that no longer matches the data", () => {
    render(
      boot(projectsDocument, {
        state: {
          selectedOptionId: null,
          fields: { view: { "s:projects": { sortKey: "removed_column", sortDirection: "descending", search: "jon" }, "s:gone": { search: "x" } } },
          scrollTop: 0,
        },
      }),
    );
    // The stale sort is ignored; the program's initial order applies with the search.
    expect(rows()).toEqual(["Ember", "Beacon"]);
    expect(container.querySelector('[aria-sort="descending"]')?.textContent).toContain("Open issues");
  });
});
