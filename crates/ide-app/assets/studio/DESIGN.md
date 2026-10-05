---
name: Choro Studio
description: Native Studio workspace and named project system library; token values below describe the legacy starter only.
colors:
  color-background: "#ffffff"
  color-surface: "#f4f5f7"
  color-text: "#202124"
  color-muted: "#60646c"
  color-primary: "#335cff"
  color-on-primary: "#ffffff"
typography:
  body:
    fontFamily: "system-ui, sans-serif"
    fontSize: "16px"
  heading:
    fontFamily: "system-ui, sans-serif"
    fontSize: "32px"
rounded:
  radius-control: "6px"
  radius-card: "12px"
spacing:
  space-small: "8px"
  space-medium: "16px"
  space-large: "32px"
components:
  button:
    backgroundColor: "{colors.color-primary}"
    textColor: "{colors.color-on-primary}"
    rounded: "{rounded.radius-control}"
  input:
    backgroundColor: "{colors.color-background}"
    textColor: "{colors.color-text}"
    rounded: "{rounded.radius-control}"
  card:
    backgroundColor: "{colors.color-surface}"
    rounded: "{rounded.radius-card}"
    padding: "{spacing.space-medium}"
  heading:
    textColor: "{colors.color-text}"
    typography: "{typography.heading}"
---

# Design System: Choro Studio

## Overview

Studio is a compact native Choro workspace for viewing and editing screens. Design workspaces have **Agent**, **Screens**, and **Design system** sidebar tabs. Named system workspaces have **Agent** and **Library**, with a generated specimen in the center. Ordinary screens retain the overview and embedded editor with a right inspector in Edit mode.

This document describes the implemented surface from source. It is scoped to Studio, not the entire Choro application. The generated specimen has offscreen WebKit evidence; native library, sidebar, and review-dialog captures were unavailable. This document does not certify native rendered layout or contrast.

**Token scope:** frontmatter preserves the legacy starter defined in `crates/ide-core/src/studio/mod.rs`, for compatibility documentation only. New systems start empty. New designs use an explicitly selected applied system or an explicitly configured project default; otherwise they have no system. These values are neither extracted project identity nor Choro chrome tokens. Named systems own their tokens, recipes, and bundled font faces; design overrides take precedence. Native controls and editor chrome use Choro’s theme.

**Key Characteristics:**

- Native sidebar navigation and shared Choro control builders.
- Cached screen previews in a scrolling overview.
- One live screen with the upstream Vvveb inspector and inline editing toolbar, shared between Canvas and Focus.
- Named, project-owned systems with separate draft and applied versions.
- Explicit per-design selection and local overrides.
- Generated specimens, native editing dialogs, and before/after review.

## Colors

### Primary

The starter's `color-primary` supplies the button fill; `color-on-primary` supplies its text. This is retained legacy content, not the palette of a newly created system or the Studio shell.

### Neutral

The starter's `color-background` is the screen background, `color-surface` the card recipe fill, `color-text` the main text, and `color-muted` the supporting text.

### Editor and native shell

The editor and canvas receive one shared payload from `studio_editor::web_theme`: `bg` (`design::base`), `panel` (`nav`), `stage` (`design::stage`, the deepest plane), `track` (`design::track`), `seg` (`design::track_choice`), `field` (`design::field_well`), `surface`, `raised` (`surface_2`), `float` (`focus`), `text` (`t1`), `muted` (`t2`), `faint` (`t3`), `line`, `line2` (`line_2`), `accent`, `ink` (`accent_ink` on the base plane), `danger` (`rose`), `ok` (`sage`), `warn` (`amber`), and `scheme` (`light` or `dark`). These override the standalone Choro Dark fallbacks in `editor.html`. `scheme` selects `color-scheme`, Bootstrap's `data-bs-theme`, and the color picker mode, so light themes get light form controls. Native separators, preview tiles, labels, and errors also resolve through `ui/design`.

Interactive emphasis in web chrome uses `ink`, not `accent`: the raw accent is pale by design and does not carry as a line or glyph on light planes.

The embedded editor keeps a fixed blue element-selection outline (`#335cff`) because it must read over arbitrary authored content. Save errors use the `danger` role on the `float` plane.

### Visual direction: lit stage, one track

Studio's chrome is a quiet bench around a lit stage. Three decisions carry it:

- **Planes.** All chrome — native sidebar, editor toolbar, inspector, and the native bar under the stage — sits on `base`. The stage (editor canvas, Canvas overview, Grid overview) sinks to `stage`. The authored artboard is therefore the brightest object in the workspace and carries the only shadow.
- **One track for every choice.** Agent/Screens/Design system, Design/Prototype, Content/Style/Advanced, Desktop/Mobile, Canvas/Grid/Focus, and the inspector's option groups (Float, Text align…) all use the same recessed `track` with the chosen segment rising to `seg`. Location stays neutral; there is no accent underline. `seg` is `surface_2` in dark themes and `surface` in light themes, because light `surface_2` lands on the track's own lightness.
- **Actions are quiet, status is a dot.** Actions (Undo, Redo, Retry save, Export PNG, Fit all…) are borderless fills that appear on hover. Save state is a 6px semantic dot (`ok`, `warn`, `danger`) plus the existing words. Semantic color lives on the glyph; only a failed save colors its text.

**The Theme Ownership Rule.** Use Choro's theme for workspace controls. Resolve authored screen styling through project tokens and design overrides.

## Typography

Native Studio labels use `design::text_ui()` (12.5px). Sidebar mode tabs use the shared tab treatment with semibold text (12px); the workspace title uses the shared header builder.

The embedded editor uses the platform system stack (`-apple-system, BlinkMacSystemFont, sans-serif`) at 12.5px, matching native `text_ui`. Inspector section headings use the same size at weight 600; field labels are 11px (`text_label`) in `faint`; track segments are 12px semibold like the native sidebar tabs. Controls inherit the editor font. Sizes and zoom values use tabular figures.

Legacy starter screens use the body and heading roles in frontmatter. Named systems may bundle local WOFF, WOFF2, TTF, or OTF fonts and declare family, weight, and italic metadata. The generated specimen uses the draft’s body and heading tokens, with 16px and 32px fallbacks and 1.5 body line height. Its 36px system title and 18px section headings label the specimen; these are not authored product type tokens.

## Layout

- **Workspace:** a full-width shared Choro header sits above the working area. The left sidebar is fixed at 318px, with a right divider. Studio uses `stage_sidebar_header`: a 44px native tab row aligned with the screen header, without positional offsets. Screens and Design system scroll independently; Agent uses the native conversation scroller with its composer anchored below. Design workspaces use three tabs; system workspaces use Agent and Library. Each set shares one row. Collapsing the 318px sidebar leaves a 38px reopen rail.
- **Overview:** non-archived screens appear in a virtualized list of grid rows. The column count is `floor((window width - 800) / 280)`, clamped to 1–4. Rows are 225px tall; previews are 175px tall and contain the full image. This sizing follows the window width, not the measured center-pane width.
- **Standalone editor/harness:** a 44px toolbar sits above the stage and 300px upstream Vvveb inspector. Left to right: the Edit/Preview track, a short rule, Undo/Redo icon actions, free space, save status (with Retry save and Keep draft & reload when a save fails), zoom, and the Inspector toggle, which sits over the inspector's own column. The stage has 28px padding and a camera for pan and zoom. A matching 44px native `stage_bar` closes the stage from below, so the artboard is bracketed by two equal bars with 12px gutters. Inspector tabs, section headers, fields, and inline toolbar retain upstream markup. The toolbar is a wrapping row with a 44px minimum height, so no control can be clipped or pushed outside the host. Below 760px a failed save moves Retry save and Keep draft & reload onto their own right-aligned second row, as a pair. Below 640px the Inspector label gives way to its icon; below 520px the status keeps its dot and drops its word, except for Save failed, which keeps its word down to 440px. No action is removed at any width, and the stage simply refits: the authored viewport is never touched. `editor.test.py` asserts that every visible toolbar control stays inside the toolbar and the host without overlap at 560, 520, and 400px, with the inspector open and collapsed. The Inspector toolbar button collapses or reopens the right column without rebuilding the document or inspector.
- **Preview:** hides the inspector and expands the canvas to one column, with 12px padding.
- **System specimen:** a centered 960px content area uses 48px padding, reduced to 24px below 600px. Typography, color swatches, component examples, foundations, and source notes form a single document. Swatches use an automatic grid with 170px minimum columns and 24px gaps; the foundations list becomes one column below 600px.
- **Screen size:** new screens start at 1440 × 960. Screen menu actions can change the viewport. The iframe receives its authored dimensions before the page loads. Zoom transforms a fixed-size inner surface inside a wrapper matching its scaled bounds; window and inspector resizing therefore do not trigger authored responsive breakpoints. Fit centers the screen using available canvas width and height after padding, capped at 100%. The single-screen camera supports 2%–400% zoom with 25%, 50%, 75%, 100%, 200%, and 400% presets; Fit selection centers the selected element. Pinch or Ctrl/Cmd-wheel zooms around the pointer. Two-finger scrolling pans in Edit; Preview keeps ordinary page scrolling for prototype interactions. Space-drag and middle-button drag pan in either mode, without stealing Space from text inputs. Gestures transform the same live iframe and leave source, selection and undo untouched. Free zoom preserves its center when sidebars or the host resize; Fit continues fitting. Camera preferences are versioned localStorage, keyed by screen and explicit viewing dimensions, with finite-value validation and an in-memory fallback when storage is unavailable. Preview uses a view-only input relay from its opaque iframe, checked against the active frame window. System specimens retain their original scrolling behavior, and exports ignore the camera. Desktop/Mobile viewing controls explicitly change the temporary viewport.

The native header uses the same workspace bar, Back button, title column, contextual subline, and action slot as other Choro workspaces. Captured source documents and implementation agents appear as clickable subline indicators; available implementation pull requests use the shared indicator. Implement becomes Reimplement when an implementation agent exists. There are no attachment buttons or separate source banner. Preview refresh, saved-edit undo/redo, and subset implementation live under Screens → Screen actions.

## Elevation & Depth

The Studio overview and editor use surface fills, borders, and spacing to distinguish regions. Preview tiles have a tonal background without a Studio-specific shadow. Native dialogs and menus inherit the shared Choro elevation treatment.

The starter system defines `shadow-card` as `0 2px 8px #00000012`; the starter card recipe does not apply it automatically. Do not describe this token as an active card shadow.

## Shapes

Native preview tiles use `design::r_sm()` (7px). Shared native tab tracks and controls retain the radii owned by their builders.

Embedded controls share native geometry: 28px controls with 5px corners (`r_xs`), and 30px tracks with 7px corners (`r_sm`), 2px inset, and 26px segments. Toolbar actions have no resting fill or stroke; they take `surface` on hover and `raised` when pressed. Zoom keeps a resting `surface` fill because it holds a value. Inspector fields are wells: `field` fill, 1px `line2` border, `faint` on hover, `ink` with a soft ring on focus, `danger` when marked invalid. Inspector section rows are 36px with a quiet chevron. Bootstrap's color transitions are disabled in the inspector so state changes are immediate, as in native chrome. The editor artboard and canvas artboards use a 1px ring plus a soft two-layer shadow (box-shadow only), never a border, so chrome cannot change authored dimensions; the selected canvas artboard uses a 2px `ink` ring. Keyboard focus uses a 2px `ink` outline with 2px offset. Disabled buttons reduce opacity to 0.45. These editor measurements are distinct from the authored starter's control and card radii in frontmatter.

## Components

### Workspace navigation

The native project header holds the design title on the left and the project actions on the right, in order: the Design / Prototype track, Compare, Implement. A hairline separates it from the workspace below. All screens is not a project action. It is the leading action of the native canvas header (`stage_header_bar`), a separate row at the top of the stage column, to the right of the sidebar. A `stage_bar_rule` follows it, then the stage context: the current screen name and dimensions while editing or in Focus, “Playing <screen>” in Prototype, otherwise the view and screen count (“Canvas · 4 screens”). The canvas header is present with any sidebar tab or a collapsed sidebar. All screens returns to Canvas and fits all artboards; it does not end the current edit. Undo/Redo, save state, recovery actions, and Inspector share this native context row; the embedded toolbar and its duplicate screen label occupy no space in Canvas, Focus, or Prototype. Agent / Screens / Design system retain their existing sidebar content.

Canvas / Grid / Focus lives in the native bottom bar in Design only. New designs default to Canvas; explicit saved view choices are respected. Grid selects a preview on click and opens Focus on double-click. Focus hides the other artboards in the same canvas host. Canvas ↔ Focus preserves the live iframe, source, selection and undo stack; the previous canvas camera is restored on return. The screen keeps its authored dimensions, regardless of window width. Archiving the selected Focus screen chooses the next available screen. Named system workspaces keep their separate specimen surface.

There is no Edit / Done editing step in Design. Double-clicking image text forwards its authored coordinates into the one live editor once ready, so the first double-click enters text editing. Clicking empty canvas finishes text editing and clears element selection without destroying the live document. Switching to Grid, Prototype or another screen still waits for the existing save acknowledgement; conflicts keep the draft available.

In Design, Cmd+Z or Ctrl+Z undoes the active screen's edits; Cmd/Ctrl+Shift+Z or Ctrl+Y redoes them. These shortcuts share the toolbar history, including when focus is on empty canvas space. Finishing a text-editing session counts as one step, and property changes have their own steps. The live editor retains more than ten steps across autosaves and Canvas/Focus switches; a new edit discards the redo branch. Inspector text fields keep their native typing undo. Prototype, the agent composer, and an overview without a live editor do not invoke this screen-edit history. Screen navigation replaces the live history; the separate Undo saved edit action uses persisted design history.

Canvas and Focus use native bottom zoom controls; their screen name, Undo/Redo, save recovery and Inspector live in the native screen header. Prototype uses native bottom zoom and Fit screen controls, Desktop/Mobile viewing controls, Back and PNG export. Canvas/Grid/Focus is hidden in Prototype, and Desktop/Mobile is hidden in Design. The embedded standalone editor's Edit/Preview and zoom controls remain available to its regression harness and legacy callers; they are not exposed by these Studio workspace modes.

### Native controls

All new or modified feature buttons must use `ui/style.rs` builders or canonical `ui/design` helpers. The canvas header above the stage is `stage_header_bar`. `stage_bar_choices` sizes its track from its content (`flex_none` alone keeps the sidebar track's zero basis, which collapses it over neighbouring actions). The bar under the stage is built from `stage_bar`, `stage_bar_choices`, `stage_bar_choice`, `stage_bar_readout`, `stage_bar_rule`, and `stage_bar_notice`; the track is the sidebar tab track, and `stage_bar_choice` is a real `Button` (keyboard focus, activation, and selected state) whose custom variant uses the same `track_choice` fill and ink as the sidebar tabs. Studio also uses `design_sidebar_tabs_header`, `sidebar_mode_tabs`, `sidebar_mode_tab`, `ghost_button_compact`, `header_icon_button`, `refresh_icon_button`, and `implement_button`. Naming dialogs use `primary_button_compact` for Save and `dialog_neutral_button` for Cancel. Destructive modal actions, if added, must use `danger_button_compact`.

### Comments tool

Comments is an independent tool over Design or Prototype. Its single pressed-state control lives in the bottom stage toolbar, outside the viewing-mode track; there is no duplicate top-header or floating web control. Unmodified C toggles it outside text entry. Entering or leaving the tool preserves the live authored iframe, editing state, and camera; the comment capture layer temporarily owns screen clicks and uses a comment-bubble cursor. Grid temporarily uses Canvas while the tool is active.

Clicking the design opens a composer beside its pin; clicking a saved pin opens the full note in that same canvas popover without moving the camera. The right-docked list is only an index, with numbered pins matching its rows. Selecting a row focuses its location and opens the canvas popover, navigating to the note's screen in Prototype when necessary. The list collapses to a narrow rail (44px) while retaining the open-note count, unsent-draft indicator and open popover. No editor or note actions live in the sidebar.

The popover uses the existing surface, text, muted, line, and ink roles, a 320px preferred width, 8px corners, 12px padding and compact shared controls. It narrows or flips left near the right edge and clamps within the available stage, excluding the sidebar; narrow stages use an above/below placement to keep the pin visible. Long notes scroll inside it. Actions use the shared web `StageButton` family and wrap together at compact widths; the native Comments action uses the shared compact ghost builder. Posting, failures and handoff confirmation stay beside the active note; list-level loading or navigation errors stay in the index.

Send to agent sits beside Resolve on an open note. It submits the saved note to the existing design assistant conversation with the pinned screen frozen as the request target. Sending leaves the note open; Resolve remains an explicit, separate action after the work is complete. An unsent text draft or pending request prevents leaving the tool until it is handled.

### New design form

The 480px native dialog uses a single full-width form, with 20px between field groups and 8px between labels and controls. Design name starts empty with a short example placeholder and receives focus. Design system uses the shared field selector, with a project default preselected when available; draft systems remain visible but cannot be selected before application. One short hint sits below the selector. Cancel and Create design use shared modal builders. Enter and Create design share validation; an empty name keeps the dialog open with a field message.

### Screen navigation and preview cards

Screens provides All screens, selectable screen rows, per-screen overflow menus, and Add screen. Archived screens remain labeled in the sidebar and are omitted from the overview. Preview cards show the screen name under a cached image. An older cached image remains visible with an Updating label while its replacement is pending; missing images show Rendering screen…. Refresh screen previews retries rendering.

The 318px native sidebar uses the shared Choro tab track and collapse icon. Collapsing leaves a 38px reopen rail and gives the canvas the remaining width. The selected tab, selected screen, expanded sections, and each tab's scroll position remain in workspace state. Screens uses flat 28px rows with page icons and a selected fill, a Screens section with an Add action, and an initially collapsed Archived section.

System **Library** groups draft tokens into Colors, Typography, Spacing, Corners, Shadows, and Other. Compact rows show readable names, values, and hex swatches; tooltips retain exact names and full values. Recipes expand into property/value pairs. An ordinary design’s **Design system** panel uses a 36px full-width system dropdown with 12px horizontal padding, a quiet description, an Open design system action when linked, and a separated local-overrides section. The dropdown refreshes records on open, marks unapplied systems Draft, and includes Manage design systems. Studio tabs use content-based widths and horizontal padding; the collapse control sits separately outside the track, with symmetric vertical header spacing. All of this chrome retains Choro’s theme.

In the Designs hub, Studio shares the 252 × 190 card shell with Figma. Its 124px preview area contains a cached image from a non-archived screen, or a quiet design placeholder. A Studio badge identifies the provider; the footer contains the design name and active screen count. Catalog images are resolved off the UI thread and refreshed after returning from a design or finishing a thumbnail batch. No live webview is created for a hub card.

### Sections

Sections are optional, flat, named flows (“Add post”, “Mobile app”). Each screen belongs to at most one section; unsectioned screens stay valid. Membership, order and presentation are revisioned design metadata (`sections`, `section_layout` in `design.json`, omitted for designs without sections). Every grouping change is one saved, undoable edit; an unsaved screen editor is flushed successfully first and stays open with its caret and history.

**Screens sidebar.** The Screens header carries New section beside Add screen. Groups appear in canvas order: a 28px section row (fold chevron, semibold name, active count, overflow menu), its indented screen rows, then **Unsectioned** and the separate Archived group. A Stacked / Side by side track sets the design-wide arrangement. Clicking a section selects it and opens its inspector; double-clicking fits it. Add screen targets the selected section. Section menus offer Rename, Add screen, Move earlier/later, Fit section and Ungroup section. Screen menus (overflow and right-click) add Move to section › sections, New section…, Unsectioned, plus Move earlier/later within the group. Rows drag onto section rows, between screens (an insertion line), onto an empty section's drop row or onto Unsectioned; section rows drag to reorder. Escape cancels a drag.

**Canvas.** Each section is a quiet surface behind its screens: a faint surface fill and a 1px line stroke, with the title resting on its top edge in a reserved band. Left title is plain semibold text; Full-width header is a tinted bar joined to the surface, aligned left or center. Selection and drop targets use the existing ink emphasis. Titles render at 16–30 display pixels regardless of artboard scale; the band clears 16px titles down to ~11% zoom, below which a title grows upward and collisions resolve in favor of the selected section (others shorten, then hide). Long titles truncate with the full name in the tooltip and inspector. Fit all and Fit section include title bands. In Focus, the header reads “Section › Screen”.

**Layout.** Deterministic Rust geometry is authoritative: horizontal sections run left to right with top alignment, vertical sections top to bottom with left alignment, using real dimensions plus caption space; no wrapping. Defaults: stacked sections, horizontal screens, 96 between screens, 48 padding, 160 between sections, Left title. Archived screens keep membership but take no space; an empty section is a compact drop area. The first section freezes an integer board origin below the existing screens so later edits never move the board. Grouped screens keep their old free positions in personal layout; newly unsectioned screens move outside the board, while a drag-out keeps its drop position.

**Drag and drop.** Dragging an artboard shows the target section and an insertion marker without reflowing anything; only an accepted drop reflows. Drops carry the interaction-start revision; conflicts roll back to authoritative positions with the normal error. Same-slot drops snap back without an edit.

**Section inspector.** Selecting a section replaces the screen inspector with a native 272px panel: Name, Screen direction (Horizontal/Vertical), Space between screens, Title (Left title/Full-width header), Header alignment, Fit section and Edit section (opens the agent with this flow as the default target). Grid groups previews under section headings in the same order.

### Inspector and editing toolbar

In ordinary workspaces, one native screen header provides All screens, the screen name or overview context, Undo/Redo, save status, and Inspector. The embedded toolbar is hidden and occupies no layout space; its existing editor actions are invoked through the session-checked toolbar-command bridge. Toolbar-state replies carry validated save, history, inspector, and recovery state. Failed saves expose Retry save and Keep draft & reload on a recovery row. The standalone editor harness retains its web toolbar. With nothing selected, the inspector shows a centered, non-dismissible empty state — “Nothing selected. Click an element on the screen to edit its content and style.” — in place of upstream's alert; the adapter rewrites the alert's text in place and leaves the pinned vendor file unchanged. The right inspector is upstream Vvveb's Content/Style/Advanced panel, with its real templates, inputs and component definitions. Collapsing preserves the selected element, inspector inputs, dirty draft, and undo history while returning the column's space to the stage. The collapse preference survives Edit/Preview transitions and uses versioned localStorage when available, with an in-memory fallback. Preview, system specimens, and thumbnails hide the toggle. Double-clicking text opens the floating rich-text toolbar, including partial-text formatting.

The integration uses unmodified, pinned upstream files. Choro supplies a separate adapter for local image imports, screen links, custom token overrides, clean document saves and revisioned undo. The full Vvveb left navigation, page manager and server features are not initialized. Layout overrides keep the native Choro sidebar outside the editor.

### Screen footer and implementation comparison

An open ordinary screen has the native canvas header above the web surface and a native `stage_bar` below it: the Canvas / Grid / Focus track in Design, or Back, Desktop/Mobile and dimensions in Prototype, with Export PNG on the right. The bar wraps at narrow widths rather than hiding actions. All screens flushes the camera and active edit before navigation; failed saves retain the editor and draft. Export progress and results (“Exporting screen…”, “Exported <path>”) appear inside the bar beside Export PNG, truncated with the full text in a tooltip, not as a second row under the header. The overview uses the same bar for Canvas / Grid / Focus, zoom, Fit all, Fit selected, and Arrange. Named system workspaces have no bar and keep the notice row. Errors keep their full-width row. Viewing changes preserve the editor document and undo state; they are temporary and reset on screen navigation. Export displays a pending state while the editor flushes or the PNG is being written, and surfaces errors in the workspace. PNGs render saved source at the displayed dimensions, without transient prototype state.

Compare sits beside Implement in the shared header and becomes Close Compare while open. It reuses the existing implementation preview panel. Review feedback targets the linked coding agent, with a saved Studio snapshot as context.

### Save and recovery states

The editor reports Saved, Unsaved, Saving…, and Save failed. A failed save exposes Retry save and Keep draft & reload plus an error message. Native structural actions are disabled while edits are dirty or saving. Implement stays actionable and waits for its correlated editor flush acknowledgement; save failure keeps the request pending and preserves the buffer. Choose screens flushes before opening its selection dialog. Screen changes flush pending edits before navigation.

### Named system library and specimen

The project’s Design systems library uses the shared design-hub card shell. Cards show a cached specimen or color strip, name, platform, Draft/Applied/Archived state, and linked-design count. New system asks for a name, platform, and project source or proposed direction. System actions provide details, duplication, explicit project-default selection/clearing, and archive/restore.

The system workspace reuses shared native navigation and regular agent conversation history. Before any conversation starts, Build with agent submits a source-aware draft request through the shared composer flow; existing typed text is preserved. The center is a deterministic, script-free specimen generated from draft tokens and recipes. Selecting a specimen token or component opens a native token or recipe dialog; the specimen itself is not a freeform screen document. Draft undo/redo uses the existing Studio transaction history.

Review changes shows token/recipe/font changes, affected design names, and before/after images for the system and the first linked design when present. Apply system publishes the draft; Keep editing leaves it uncommitted to consumers. An ordinary design’s Change system flow also presents a comparison and preserves local overrides. All active systems appear in the sidebar selector. Draft choices open a guided Open draft dialog; only applied systems can be bound. No system is explicit. Missing token references must be resolved before switching.

**The Explicit Publication Rule.** System editing changes the draft. Linked designs inherit only the applied version, published through the native review action.

**The Explicit Override Rule.** Local values stay with their design when its system changes. System edits belong in the named system workspace.

## Do's and Don'ts

- Do keep design navigation in Agent/Screens/Design system and system navigation in Agent/Library, using the shared native sidebar.
- Do show the cached overview or selected screen in design workspaces, and the generated specimen in system workspaces.
- Do use shared native button and header builders.
- Do apply host theme roles to editor chrome and design tokens to authored screens.
- Do keep pending, failed, and recoverable save states visible.
- Don't add a second screen-tab row or recreate the full Vvveb application shell.
- Don't give a choice an accent underline or an outlined button group; use the track. Don't put a shadow or a lifted fill on chrome; the artboard owns elevation.
- Don't instantiate `gpui_component::Button` directly in Studio feature modules.
- Don't silently turn local styling changes into project-wide token changes.
- Don't install the legacy starter silently or treat its colors and recipes as extracted project identity.
- Don't treat this source-based description or the offscreen specimen capture as native visual validation.


### Editing an artboard on the canvas

Double-click an artboard (or select it and press Enter) to activate its editor in place. While editing, clicking another artboard saves and switches the active editor. Clicking empty canvas clears element selection while retaining the live editor; there is no Done editing step. All other screens remain images; distant nodes and images are still culled. The active editor is independent of node culling, so panning it offscreen cannot discard its draft or undo history. Its authored viewport stays fixed while the canvas camera transforms its visible bounds. Canvas editing zoom follows the overview's 2%–200% range.

The native host generates one trusted editor document and grants its editor session to the current canvas session. The canvas mounts that document in one sandboxed `srcdoc` wrapper, with the existing sanitized authored iframe inside. The full-size wrapper is clipped to the artboard, toolbar, inspector and visible floating controls; other canvas pixels remain interactive. The editor relays gestures to the canvas camera. Its save messages travel through the parent, which checks the source window and session; Rust checks the parent origin and active session grant again. Unmounting or switching revokes that grant. Overview images remain within their existing budgets; the active editor's DOM and assets are additional memory, not part of the decoded-image budget.

Saving, conflicts, recovery, asset uploads, implementation flushes and undo reuse the single-screen editor paths. Canvas navigation flushes the active editor before acknowledging its own correlated navigation request. An unrelated autosave acknowledgement cannot release pending canvas navigation. Failed saves keep the draft and recovery controls available. Only the changed screen's preview is refreshed unless its shared tokens/assets affect other screens.

**Prototype** plays one authored screen with scripts enabled inside the existing opaque-origin sandbox. Its linked elements use `data-studio-screen` to navigate to an unarchived screen. **Back** follows the linked-screen history, and **All screens** returns to the saved canvas camera. Editing controls are hidden in Prototype. The existing standalone editor and Grid fallback remain available.

`inline.test.py` exercises the real bundled canvas and editor together in isolated WebKit with 100 screens: geometry, hit-testing through the clipped shell, gestures, retained offscreen drafts, save failures, retry, undo, one-active-editor switching, stale/foreign message rejection and Prototype links. It is a correctness test, not a native frame-pacing or combined-process memory benchmark.

Canvas trackpad panning uses full-speed deltas, matching the live editor. Camera persistence is debounced for 300 ms, including programmatic move-end events from inline gestures. Movement requests at most 512-pixel previews for missing screens; sharper tiers are requested after 150 ms idle. Identical or already fulfilled demand is not resent. Recent decoded images remain in an LRU across offscreen culling, so returning does not require another render; distant DOM nodes remain unmounted. Offscreen images are evicted above a 32 MiB retention target, preserving the existing 64 MiB combined resident/in-flight decode cap and two-decode limit. Visible previews downgrade when necessary under memory pressure. Completed renderer jobs are cached even if the camera has moved on, but only current screen content can be delivered. The active editor pans by transforming its wrapper, keeping authored iframe dimensions and editing state intact.

### Header layout contract

The project header groups its identity on the left and Design/Prototype, Compare, and Implement on the right. Its bottom divider spans the workspace. The left sidebar has a continuous right divider; the single native screen header begins to its right, followed directly by the stage and inspector. All screens lives only in this screen header. Prototype retains the same context row without editing actions or a duplicate embedded screen label. Canvas/Grid/Focus and camera controls stay in the bottom bar.

Choice tracks reset the sidebar helper's zero flex basis to Auto before using intrinsic button widths. Native geometry checks measure these tracks and their neighbors with long names at 480–1400px. Project actions and screen-header actions wrap as complete groups when necessary; screen context truncates. Colors, typography, and controls come from the existing Choro builders and tokens. Editor chrome never changes authored viewport dimensions.
