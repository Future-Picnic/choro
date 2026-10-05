# Studio canvas

The canvas is integrated into Studio's All screens view, with native Canvas/Grid, zoom, Fit, and Arrange controls. Artboards contain images only; Enter or double-click opens the existing editor. Canvas selection supplies the default agent target. Design-system workspaces keep their existing viewer.

Resizing commits one `UpdateScreen` transaction, checks the revision/fingerprint captured at drag start, preserves other screen properties, and participates in saved-edit undo/redo. Cancellation restores authoritative bounds. Replies are correlated to request IDs. Existing previews keep their previous authored proportions until a replacement arrives.

Workspace layout is stored atomically outside the project under the Studio cache. Missing state gets defaults. Corrupt state is preserved; explicitly choosing Arrange copies it to a preserved file before recovery. Canvas/editor navigation flushes pending layout changes. Archived positions remain available for restoration.

## Sections

Bootstrap and `state` carry host-authored `sections` (metadata plus authoritative geometry) and `arrangement`; grouped screens' `layout.positions` are their derived board positions. Section boards are non-selectable, non-draggable React Flow nodes at `zIndex: -1`, culled like artboards, with exact `measured` sizes. Titles size from `labelPixels(zoom)` and `sectionLabels` (collision priority for the selected section); strokes scale with the `--zoom` CSS variable.

Session-scoped messages: `select-section`, `move-screen {screen_id, section_id, before_screen_id, position?, revision, fingerprint}` (revision/fingerprint captured at drag start), `reorder-sections`, and `context-action` (whitelisted actions for exactly one screen or section). Rust validates bounds and shapes before queueing. `move-result` is correlated by request ID and carries authoritative screens, sections and positions; uncorrelated or older results are ignored. A dropped screen stays at its drop position, non-draggable, until the result arrives. Escape cancels a drag back to its authoritative position. `dropTarget`/`sameSlot` mirror the Rust insertion rule. Fixture hooks (`begin`, `hover`, `finish`, `drop`, `escape`) exist only when `boot.test` is set.

## Comments

Comments is an independent tool over Design or Prototype. The native Comments
button lives in the bottom stage toolbar; `C` toggles it from the canvas, editor or authored
prototype, except while typing in a field. It preserves the underlying mode,
live iframe and camera. Grid temporarily displays the canvas and returns to Grid
when the tool closes. There is one Comments control, with no duplicate web or
top-header button. The cursor becomes a comment bubble over the design.
Click a screen to place a pin and compose beside it. A saved pin opens the same
canvas popover for reading, Resolve and Send to agent. The docked right sidebar
is an index of every open note on active screens, without composers or actions.
Selecting a row centers its pin and opens that popover; in Prototype it opens
the note's screen first. The canvas reserves the list's width; popovers flip
and clamp to the available stage, avoiding the sidebar. Collapse the list to
free canvas space while retaining the open popover and unsent draft. Clicking
a pin does not move the camera or expand a collapsed list.
Closing the tool unmounts its comment layer. Standalone screens defer evaluating
the separate React overlay bundle until first activation. There are no
replies, cloud services, new npm dependencies, or background comment polling.
Screen dragging, resizing, editor opening, and design context menus are disabled
while commenting. Closing Comments restores the previous Canvas/Grid/Focus
view. Typed drafts must be posted or cancelled, and pending requests must finish,
before leaving.

Next to Resolve, Send to agent submits the persisted note to the current design
assistant using the existing provider and queue. Its frozen target is the pinned
screen, independent of canvas selection or a running turn. The note stays open
for manual resolution; errors keep it retryable, and accepted submissions are
disabled while the current comment layer remains mounted.

Pins use fractions of screen dimensions and retain their position through
layout changes, resizing, and zooming. The host atomically saves
`choro_designs/<design-id>/comments.json`, separately from design revisions,
undo history, preview keys, and exports. Reads and writes run off the UI thread;
revision conflicts return current comments and preserve the draft. Corrupt or
future-version files are preserved. Resolved notes remain saved and disappear
from open comments. Archived screens retain their notes but have no visible pins.

The offscreen WebKit fixture covers posting, resolving, sidebar collapse, failed-save draft
recovery, request correlation, mode isolation, preview reuse, pin positioning,
multiple-screen navigation, long lists, draft restoration, and popover geometry:
`python3 crates/ide-app/assets/studio/canvas.test.py --protocol`.
Add `--comments` for a showcase capture of a selected note.
Add `--width=560 --height=720` for a compact capture or `--light` for a light theme.
Fixture screenshots contain synthetic screens and notes. They do not control the
live app or establish production frame pacing.
`python3 crates/ide-app/assets/studio/editor.test.py --comments` and
`--comments --prototype-mobile` cover screen-local pins, shortcut exclusions,
handoff failures, retry, Resolve, cross-screen index selection and preservation of the live screen. The player
fixture also covers scrolling under the comment surface. Agent handoffs are
mocked and never start real provider turns. `inline.test.py` verifies the same
live editor remains mounted when toggling the tool on the canvas.

## Rendering and isolation

The separate `StudioCanvas` webview intent uses validated, session-scoped messages and a read-only image protocol with opaque, host-issued keys. Its CSP blocks network connections, frames, forms, and external navigation. Canvas cannot send editor-save messages.

The frontend requests visible artboards plus a margin, updates demand while panning, and refines after camera idle. Images below 48 displayed pixels are omitted. Preview tiers are 256/512/1024/2048 pixels, with a 64 MiB estimated decoded budget and two controlled decodes. Temporary pressure retains deferred requests and prioritizes downgrades. The host's encoded LRU is bounded to 32 MiB. Only the requested screen document is cloned for a rendering job.

Canvas jobs share the serial renderer lock with exports and agent-review screenshots. Explicit jobs take priority between canvas jobs. A reusable Swift helper exits after the queue remains empty for 500 ms. Higher-resolution previews use a single worker scratch PNG, not a disk pyramid. Hidden canvases stop requesting work; stale jobs cannot update the current view.

## Build and verification

Pinned dependencies: React Flow 12.11.6, React/React DOM 19.2.7. The lockfile and bundled JS/CSS are included. `scripts/bundle.sh` builds the bundle and collects its dependency notices. No runtime CDN, server, fonts, or credentials are required.

From this directory: `npm run build` and `npm test`.

From the repository root:

```sh
cargo test -p ide-core --lib studio::canvas
cargo test -p ide-core --lib studio::sections
cargo test -p ide-mcp studio
cargo test -p ide-app studio_ -- --test-threads=1
python3 crates/ide-app/assets/studio/canvas.test.py --protocol
python3 crates/ide-app/assets/studio/editor.test.py
```

The isolated WebKit fixture checks 10/50/100/200 screens (60% grouped into sections, stacked and side by side), section boards and zoom-independent titles, section selection, drop targets and insertion markers, same-slot snap-back, Escape cancellation, interaction-start revisions despite metadata mid-drag, correlated and stale move results with rollback, drag-out positions, empty-section drops, canvas context menus, Fit section, section culling, cancellation, keyboard opening, preservation of an active drag during metadata updates, stale resize acknowledgements, ongoing preview demand, culling, and exact PNG output dimensions. `--protocol` uses raster PNGs through a custom WebKit protocol. The model tests cover budget recovery. Native tests cover message validation, session-bound image keys, encoded cache limits, serial helper reuse/idle exit, export priority, and existing PNG export behavior.

Tests retain their temporary evidence rather than deleting files. Offscreen frame timings are deliberately null because WebKit throttles animation frames.

`--protocol --unsectioned` uses the original loose-screen layout for a like-for-like comparison. Selection-only replies carry IDs without regenerating preview metadata. Camera title updates and insertion feedback reconcile section nodes while preserving artboard data. Context-menu moves freeze their revision at menu open, correlate their result, and surface conflicts; unanswered moves time out and request authoritative state. Fit includes display-sized title extents at its resulting zoom.

`--protocol --dense` stresses 200 screens and 100 sections. `--bundle=/path/to/dist` tests a copied baseline bundle with the same fixture without reverting workspace changes.

## Performance gate and test build

**The full production performance gate remains unverified.** A normal fresh design defaults to Grid until that gate passes; users can explicitly select Canvas. The isolated onboarding test build sets `CHORO_STUDIO_CANVAS_PREVIEW=1`, enabling Canvas as its fresh-design default. Each design's subsequent explicit Canvas/Grid choice is saved locally.

The remaining gate requires actual Studio-host cold/warm entry and frame pacing, a Grid comparison with the same design loaded, combined Choro/WebKit/helper resident memory, 100→200-screen steady overhead, and repeated editor navigation. Offscreen image accounting and bootstrap timings are not substitutes for those measurements. A standalone visible probe can be run with `--protocol --visible` after workspace-required UI permission; it is still only a preliminary pacing check.
