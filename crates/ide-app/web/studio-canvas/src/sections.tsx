import { memo, useEffect, useRef, useState } from "react";
import type { Node, NodeProps } from "@xyflow/react";
import { shortTitle, TITLE_GAP_PX, type LabelPlan, type Rect, type Section } from "./model";

export type SectionData = {
  section: Section;
  selected: boolean;
  /** The section under an in-flight screen drag. */
  target: boolean;
  /** Insertion point for that drag, in canvas units. */
  marker?: Rect;
  label: LabelPlan;
  zoom: number;
};
export type SectionNode = Node<SectionData, "section">;

/** A quiet grouping surface behind its screens. The title is sized in display
 * pixels (divided by zoom), so it stays readable however small artboards get. */
export const SectionBoard = memo(function SectionBoard({ data }: NodeProps<SectionNode>) {
  const { section, label, zoom } = data;
  const header = section.title_style === "full_width_header";
  const fontSize = label.size / zoom;
  const line = fontSize * 1.35;
  const empty = !section.active_screen_ids.length;
  const title = label.mode !== "hidden" && (
    <span className="section-title" title={section.name} style={{ fontSize, maxWidth: header ? "100%" : label.maxWidth }}>
      {shortTitle(section.name, label)}
    </span>
  );
  return (
    <div
      className={`section${data.selected ? " selected" : ""}${data.target ? " drop-target" : ""}`}
      data-section-id={section.id}
      aria-label={`Section ${section.name}, ${section.active_screen_ids.length} screens`}
    >
      {/* The reserved band stays open above the title, so stacked sections read
          as separate flows; the title sits on the surface's top edge. */}
      <div className="section-band" style={{ height: section.header_height }}>
        {header ? (
          <div
            className={`section-bar ${section.header_alignment}`}
            style={{ height: Math.min(section.header_height, line + 20 / zoom) }}
          >
            {title}
          </div>
        ) : (
          <div className="section-label" style={{ paddingBottom: TITLE_GAP_PX / zoom }}>{title}</div>
        )}
      </div>
      <div className={`section-surface${header ? " joined" : ""}`} style={{ top: section.header_height }} />
      {empty && section.width * zoom >= 150 && (
        <div className="section-empty" style={{ top: section.header_height, fontSize: 13 / zoom }}>
          Drag screens here
        </div>
      )}
      {data.marker && (
        <div
          className={`section-marker${empty ? " area" : ""}`}
          style={{
            left: data.marker.x - section.x,
            top: data.marker.y - section.y,
            width: data.marker.width,
            height: data.marker.height,
          }}
        />
      )}
    </div>
  );
});

export type MenuState = { x: number; y: number; kind: "screen" | "section"; id: string; revision: number; fingerprint: string } | null;
type MenuProps = {
  menu: NonNullable<MenuState>;
  sections: Section[];
  /** The section containing a screen, if any. */
  current: string | null;
  onClose: () => void;
  onAction: (action: string) => void;
  onMove: (section: string | null) => void;
};
/** Themed context menu for artboards and sections; actions go to the host. */
export function CanvasMenu({ menu, sections, current, onClose, onAction, onMove }: MenuProps) {
  const root = useRef<HTMLDivElement>(null);
  const [submenu, setSubmenu] = useState(false);
  useEffect(() => {
    root.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const close = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as globalThis.Node)) onClose();
    };
    addEventListener("pointerdown", close, true);
    return () => removeEventListener("pointerdown", close, true);
  }, [onClose]);
  const item = (label: string, action: () => void, disabled = false) => (
    <button key={label} role="menuitem" disabled={disabled} onClick={() => { action(); onClose(); }}>
      {label}
    </button>
  );
  const index = menu.kind === "section" ? sections.findIndex((s) => s.id === menu.id) : -1;
  const navigate = (event: React.KeyboardEvent) => {
    const buttons = [...(root.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [])];
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      buttons[(at + (event.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length]?.focus();
    }
  };
  return (
    <div
      ref={root}
      className="canvas-menu"
      role="menu"
      style={{ left: Math.min(menu.x, innerWidth - 220), top: Math.min(menu.y, innerHeight - 260) }}
      onKeyDown={navigate}
      onContextMenu={(event) => event.preventDefault()}
    >
      {menu.kind === "screen" ? (
        <>
          <button
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={submenu}
            className="has-submenu"
            onClick={() => setSubmenu((open) => !open)}
            onMouseEnter={() => setSubmenu(true)}
          >
            Move to section
          </button>
          {submenu && (
            <div className="canvas-submenu" role="menu">
              {sections.map((s) =>
                item(s.name, () => onMove(s.id), s.id === current),
              )}
              {sections.length > 0 && <div className="canvas-menu-rule" />}
              {item("New section…", () => onAction("new-section"))}
              {item("Unsectioned", () => onMove(null), current === null)}
            </div>
          )}
          <div className="canvas-menu-rule" />
          {item("Move earlier", () => onAction("up"))}
          {item("Move later", () => onAction("down"))}
          <div className="canvas-menu-rule" />
          {item("Rename…", () => onAction("rename"))}
          {item("Duplicate", () => onAction("duplicate"))}
          {item("Archive", () => onAction("archive"))}
        </>
      ) : (
        <>
          {item("Rename…", () => onAction("rename"))}
          {item("Add screen…", () => onAction("add-screen"))}
          <div className="canvas-menu-rule" />
          {item("Move earlier", () => onAction("earlier"), index <= 0)}
          {item("Move later", () => onAction("later"), index < 0 || index + 1 >= sections.length)}
          {item("Fit section", () => onAction("fit"))}
          <div className="canvas-menu-rule" />
          {item("Ungroup section", () => onAction("ungroup"))}
        </>
      )}
    </div>
  );
}
