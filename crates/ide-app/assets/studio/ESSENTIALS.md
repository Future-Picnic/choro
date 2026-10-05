# Studio essentials: gaps worth closing

Reviewed 5 October 2026 against the current Studio implementation and the official Figma and Penpot guides. This is a source review and product recommendation, not a live usability or performance study. Only Delete design / Restore is implemented in this change.

## Basic file management

**Delete and recover — addressed here.** Studio cards now offer Delete design with confirmation, and the Designs page offers a compact Trash menu with Restore. Recovery preserves screen files, comments, conversations and edit history. This follows the established reversible deletion model documented by [Figma](https://help.figma.com/hc/en-us/articles/360047512294-Delete-and-restore-files). There is no permanent-delete action or expiration timer.

**Rename and duplicate an entire design — next priority.** Screen rename/duplicate and design-system duplication already exist. The ordinary Studio card has no native Rename or Duplicate action; design rename exists as a transaction operation used by agents. Add these to the existing card menu. Rename should also be available from the open design's title. Duplicate should create fresh design/screen/section identities and preserve documents, local assets, system binding and screen links. Start with fresh conversations, comments and history so feedback on the original does not silently become feedback on a copy. This matches the distinction in [Figma's duplication guide](https://help.figma.com/hc/en-us/articles/360038511533-Duplicate-or-copy-files).

Evidence: `ui/center/studio.rs::render_studio_hub_card`, `studio_sections_sidebar.rs::screen_menu`, and `ide-core/src/studio/mod.rs::StudioOperation::RenameDesign`.

## Find work and recover decisions

**Find a design or screen by name — next priority.** The hub renders design cards and the Screens sidebar renders section groups, without a name filter. Add a small search input to each existing list. Match the in-memory manifests; selecting a screen result should reveal its section and focus it. Avoid a new indexing service or background scan. Name lookup comes before full-text search of authored HTML. [Figma's file guide](https://help.figma.com/hc/en-us/sections/360006050633-Files-and-projects) includes file search; [Penpot's workspace guide](https://help.penpot.app/user-guide/designing/workspace-basics/) documents layer filtering and search on the canvas.

Evidence: `design_workspace.rs::render_design_hub` and `studio_sections_sidebar.rs::render_studio_screens_tab`.

**Browse saved versions and restore one — next priority.** Studio already saves revisions, retains recovery drafts, and offers local undo/redo plus Undo saved edit. It lacks a user-facing list of dated design versions with preview and explicit restore. Add one compact History panel using the existing saved revisions. Restoring should be a new revision, preserve the current version for recovery, keep comments separate, and make any shared-system implications explicit. Both [Figma](https://help.figma.com/hc/en-us/articles/360038006754-View-a-file-s-version-history) and [Penpot](https://help.penpot.app/user-guide/designing/workspace-basics/) expose history and restore.

Evidence: `ide-core/src/studio/store.rs::saved_revision`, `move_history`, and `ui/center/studio.rs::studio_undo`. Agent conversation history is a separate existing feature.

## Follow-up needs

**Select nested or overlapping elements reliably.** Studio exposes the Vvveb property inspector and direct canvas selection, but no visible element hierarchy. If direct editing is a core workflow, add a collapsible tree for the active screen with selection and parent navigation first. Defer multi-selection, vector tools, and advanced layer operations. [Penpot's layers guide](https://help.penpot.app/user-guide/designing/layers/) explains hierarchy and selecting overlapping layers. Evidence: `assets/studio/editor.html` and `editor.js`; Studio initializes the inspector without the upstream full application shell.

**Share a complete result without installing Choro.** PNG export is per screen; implementation handoffs are immutable internal bundles. Neither is a simple user-facing portable prototype/export flow. Start with Export all screens, then consider an offline prototype package with local assets. Add hosted links only if users need them and existing access can support them. [Figma](https://help.figma.com/hc/en-us/articles/360040531773-Share-files-and-prototypes) treats prototype sharing as a standard review workflow. Evidence: `studio.rs::studio_export_png` and `ide-core/src/studio/store.rs::handoff`.

## Preserve what already works

Canvas pan/zoom and arrange, sections, screen archive/restore, autosave and failed-save recovery, Design/Prototype, independent Comments with resolve and agent handoff, viewport switching, design systems, PNG export, and implementation comparison already exist. They are not missing features.

Recommended order: complete card management, add name search, then expose saved versions. Keep each feature in an existing surface and reuse current storage. These are priorities inferred for Choro's current workflow, not a claim that every feature of a general-purpose vector editor is required.
