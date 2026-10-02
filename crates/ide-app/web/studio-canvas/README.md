# Studio canvas

The canvas is integrated into Studio's All screens view, with native Canvas/Grid, zoom, Fit, and Arrange controls. Artboards contain images only; Enter or double-click opens the existing editor. Canvas selection supplies the default agent target. Design-system workspaces keep their existing viewer.

Resizing commits one `UpdateScreen` transaction, checks the revision/fingerprint captured at drag start, preserves other screen properties, and participates in saved-edit undo/redo. Cancellation restores authoritative bounds. Replies are correlated to request IDs. Existing previews keep their previous authored proportions until a replacement arrives.

Workspace layout is stored atomically outside the project under the Studio cache. Missing state gets defaults. Corrupt state is preserved; explicitly choosing Arrange copies it to a preserved file before recovery. Canvas/editor navigation flushes pending layout changes. Archived positions remain available for restoration.

## Sections

Bootstrap and `state` carry host-authored `sections` (metadata plus authoritative geometry) and `arrangement`; grouped screens' `layout.positions` are their derived board positions. Section boards are non-selectable, non-draggable React Flow nodes at `zIndex: -1`, culled like artboards, with exact `measured` sizes. Titles size from `labelPixels(zoom)` and `sectionLabels` (collision priority for the selected section); strokes scale with the `--zoom` CSS variable.

Session-scoped messages: `select-section`, `move-screen {screen_id, section_id, before_screen_id, position?, revision, fingerprint}` (revision/fingerprint captured at drag start), `reorder-sections`, and `context-action` (whitelisted actions for exactly one screen or section). Rust validates bounds and shapes before queueing. `move-result` is correlated by request ID and carries authoritative screens, sections and positions; uncorrelated or older results are ignored. A dropped screen stays at its drop position, non-draggable, until the result arrives. Escape cancels a drag back to its authoritative position. `dropTarget`/`sameSlot` mirror the Rust insertion rule. Fixture hooks (`begin`, `hover`, `finish`, `drop`, `escape`) exist only when `boot.test` is set.

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
