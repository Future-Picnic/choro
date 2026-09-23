# Choro Studio

Studio is Choro's default design workspace, using the native sidebar and existing Codex or Claude access. **Design** on a task or document creates a Studio design with a captured source link and a prepared Agent request. The design, source task/document, and implementation agent show reciprocal links. Document design mentions open Studio inside Choro.

**Implement** from Studio, a linked task, or a linked document attaches immutable Studio snapshots. No external design service or browser setup is required. Task identity and source requirements travel with the snapshot; later design edits do not change it.

The implementation composer shows the design as visual context: a cached snapshot thumbnail when available, its name, selected-screen count, and applied design-system name. The editable request stays short; tool identifiers and detailed implementation instructions travel behind the attachment. The agent is explicitly instructed to read and reuse the captured system's effective tokens, component recipes, fonts, assets, and local overrides. Designs without a selected system use their local screen styles. Removing the design attachment also clears its implementation target. Previews reuse one selected screen's immutable PNG, capped at 1 MiB encoded and one megapixel decoded, with an icon fallback.

Studio is the built-in design workspace. Figma links remain available alongside local Studio designs. Historical integration records are preserved for backup compatibility.

## Availability

Studio is available under Design → + in development and release builds without an environment flag. Native desktop, live-provider, signed-bundle, and total-process performance verification remain outstanding as listed below.

## Workflow

1. Enter a **Design name**, choose an applied **Design system** or **No system** from the dropdown, then click **Create design**. Empty names show a field message and keep the dialog open. An explicitly configured project default is preselected. With no selection or configured default, the design starts without a system; no generic starter is installed.
2. Use **Screens → All screens** for a cached overview. Edit/Preview is remembered for the design across screen navigation, All screens, editor reloads, and reopening the design. Open a screen to select elements, edit text on the page, or use the right inspector. The screen menu supports rename, duplicate, reorder, archive/restore, and desktop/mobile viewports.
3. Use **Design system** to change or open the selected system, browse the library, and edit local token overrides. Custom inspector values create screen/element-specific tokens in the design’s overrides. Saves register each screen’s stylesheet and inline-style document as file-backed `screen_styles` overrides for agent and manual writes alike. These local styles and overrides survive system changes; unresolved token references must be mapped before a switch.
4. Use **Agent** to discuss, create, or refine the design naturally. It can create multiple screens and edit across this design without a scope toggle or special wording. The current screen and selected element are the default target for local requests; broader requests and exclusions guide its work. All screens supports normal chat and creation; the agent asks only when the intended edit is ambiguous.
5. The workspace header provides Back, design title, contextual source/implementation links below the title, and Implement. A captured source document opens from its link; there are no attachment buttons or separate captured-source banner. Implementation agents and their pull requests use shared agent and pull-request indicators.
6. **Implement** flushes the live editor and snapshots all active screens only after the matching save acknowledgement. **Screens → Screen actions → Implement selected screens…** selects a subset. The ordinary coding-agent composer receives the immutable snapshot ID and instructions to translate it into the existing application framework.

### Compare, screen export, and viewing sizes

Ordinary Studio designs expose **Compare** beside Implement. It reuses Choro’s live implementation preview and review controls. Reviews go to a linked implementation agent, with an immutable snapshot of the currently selected saved screen (or all active screens in All screens). A Solo preview only accepts reviews for its own linked implementation agent. Unsaved design changes must finish saving before feedback can be submitted. Closing Compare returns to the design workspace.

In Prototype, the footer provides **Desktop**, **Mobile**, the current dimensions, and **Export PNG**. Viewing-size changes resize the same document without changing its saved viewport, creating another screen, or generating responsive CSS. Each screen starts at its saved size; switching screens resets the temporary viewing size. A mobile screen returns to its authored mobile dimensions when Mobile is selected. The alternate defaults are 1440 × 960 and 390 × 844.

Export PNG flushes pending edits and waits for the matching save acknowledgement, then asks where to save. It renders the saved design at the current viewing size, at one PNG pixel per CSS pixel, independently of the smaller overview thumbnails. Exports show the saved design, not transient JavaScript state from an interactive Preview. Design-system specimens do not expose these screen controls.

When asked for new screens, the agent first creates named empty screens, then builds and saves one screen at a time. This makes requested screens visible before detailed generation finishes; it does not reduce model generation time.

The Studio agent designs visual HTML/CSS and prototype interactions. It does not implement application services, authentication, persistence, or production behavior. Its bundled instructions require inspecting existing UI and conventions, reusing tokens, and reviewing affected screenshots.

The Agent tab supports separate design conversations: **+ New agent** starts a separate conversation, and the adjacent history picker restores earlier conversations for this Studio design. New conversations inherit the current provider/model preferences but receive fresh session identities. History entries use the first request as their title. The selected conversation survives reopening Choro, and existing single-conversation Studio designs retain their original chat. Creating or switching conversations waits until the current agent finishes or stops. Opening history temporarily hides the native web surface so it cannot cover the menu.

## Named design systems

**From code** in the library analyzes the current project with the existing Codex or Claude connection. The agent reads apps, shared UI packages, components, themes, and local font declarations, then proposes distinct systems with source evidence, tokens, and recipes. Apps sharing one visual foundation should produce one system; separate products or platforms may produce several. Light/dark and responsive variants remain within their product system. The agent can ask for clarification when boundaries are ambiguous, or report that no UI was found.

Select the proposed systems and choose **Create drafts**. Existing systems can be opened and are never overwritten. New drafts keep source evidence and bundled local fonts; inspect their specimens and use **Review changes** before applying them. Analysis can read source and save proposals but cannot edit code, create project files, or publish systems. Source receipts must match the files actually read; changed sources require another analysis. Repeated creation is idempotent, and the latest analysis and already-created draft identities survive reopening. **New analysis** starts a separate saved run.

Open **Design systems** from the Designs hub. Create a named system for a platform or product, describe a direction or point the agent at existing project styles, or duplicate an existing system. New systems start empty. Source notes distinguish extracted decisions from proposed styles. Library cards show the name, platform, Draft/Applied/Archived state, linked-design count, and a specimen preview or palette.

A system opens in the shared native Studio workspace with a collapsible **Agent / Library** sidebar. Each system has independent agent conversations and history, using the same New agent, Stop, retry, resume, and provider/model controls as regular Studio designs. Before any system conversation starts, **Build with agent** submits a source-aware draft request through the regular agent flow. It leaves existing typed composer text untouched. The agent reads relevant project files through Studio tools and edits that system’s draft; it cannot publish changes or retarget other designs.

The center shows a generated, script-free specimen with typography, swatches, button/input/card examples, foundations, and source notes. Select a token or recipe in the specimen to open a native editing dialog. The Library tab lists grouped draft tokens and recipes. Local font assets can be bundled with family, weight, and italic metadata. The specimen is generated from structured data rather than stored as an editable screen.

**Review changes** compares the draft with the applied version, lists changed tokens/recipes/fonts and affected designs, and renders before/after images of the system plus the first linked design when one exists. **Apply system** publishes only after the reviewed revision, fingerprint, and linked-design set still match. Linked designs receive the applied tokens, recipes, and assets while keeping local overrides. Draft edits alone do not change those designs. Publication rejects missing referenced fonts and unresolved token references.

An ordinary design’s **Design system** sidebar has a full-width dropdown. It reads the project’s current systems each time it opens, including unapplied systems marked **Draft**. Selecting a draft offers **Open draft**, with instructions to use **Review changes** before it can be linked. Applied systems show before/after images before selection and preserve local overrides and hardcoded styles. **No system** is available when token references remain valid. **Manage design systems** opens the library. **System actions** supports details, duplicate, archive/restore, and setting or clearing the project default. Archiving preserves files and existing references; archived systems are unavailable for new selection or publication.

## Files and recovery

Editable project files live only in `choro_designs/`:

```text
choro_designs/design-systems/default.json
choro_designs/design-systems/<system-id>/system.json
choro_designs/design-systems/<system-id>/assets/
choro_designs/<design-id>/design.json
choro_designs/<design-id>/overrides.json
choro_designs/<design-id>/assets/
choro_designs/<design-id>/screens/<screen-id>/index.html
choro_designs/<design-id>/screens/<screen-id>/styles.css
choro_designs/<design-id>/screens/<screen-id>/prototype.js
```

Each named system uses one atomically written `system.json` envelope containing its identity, metadata, `draft`, optional `applied` version, and applied asset names. Drafts do not use a separate `draft.json`. IDs are UUIDs and remain stable when names change. Legacy `choro_designs/design-system/system.json` is imported as **Legacy starter** or **Imported project system**, preserving the original files, asset paths, and existing design references; migration does not select a default for new designs.

Every screen explicitly declares `files: {html: "index.html", css: "styles.css", js: "prototype.js"}` relative to its identity-based directory. Legacy manifests receive these defaults on read; alternate or traversing paths are rejected. Archive preserves files; there is no permanent-delete action. The application-data Studio directory stores conversations/selection associations, host scopes, draft recovery, transaction journals, thumbnail indexes, and handoffs. Generated images are not added to the project.

Manual and agent writes share the revisioned transaction service. Saves based on an older revision can proceed when they only create new screens or edit unchanged screens, with unchanged tokens/system and existing assets. Changes to the same screen or shared dependencies still preserve the proposal and report a conflict. Updating another screen keeps the active editor and caret mounted. Transactions verify both revision and a content fingerprint, stage a journal, write files atomically, and commit the manifest last. Recovery only resumes a transaction when each file matches its old or new content. A conflict preserves both the proposal and current files. External changes reload a clean editor; dirty drafts are retained for reconciliation. Navigation flushes pending edits. Failed saves leave the editing buffer available.

The editor maintains local undo/redo. **Screens → Screen actions → Undo saved edit / Redo saved edit** use journal history across committed edits; writes from one agent scope form one history step even when manual saves occur between tool calls. Undo applies only that turn's changed fields; unrelated manual content and styles survive. Overlapping edits report a conflict before writing. A new edit invalidates redo. Shared-system reversal uses the explicit Design system flow, so it cannot silently affect other designs.

Limits: 200 screens per design, 4 MiB combined HTML/CSS/JS per screen, 4 MiB per asset, and 1,000 assets / 64 MiB in the combined design and shared asset bundle. Resulting asset limits are checked before journaling.

## Agent boundary and MCP

Studio uses the shared agent controller for Stop and retry. Cancelled turns revoke write scope without running completion review; the next turn receives a fresh scope and still requires review. Claude cancellation drains pending receipts before signaling idle and ignores an interrupted startup's late continuation. Async bridge setup errors and chat restart errors are surfaced in the design conversation. The shared resume guard permits retry after an explicit Stop before any provider response, tool activity, or usage, while preserving UI history; conversations with provider activity still require their resume ID.

Studio is a distinct optional agent context, reconstructed from its native design conversation. Existing serialized agents remain compatible. Both provider adapters attach a frozen request-context payload with effective tokens, recipes, overrides, screen metadata, read-only screen IDs, captured sources, and available repository convention files. Each request receives a host-created scope with the original screen, selected element, base revision/fingerprint, writable screen IDs and permitted operations. An ordinary design’s Studio chat scope permits the whole bound design and screen creation. The frozen current screen is a conversational default, not an enforced single-screen boundary; following narrower user intent is an agent instruction. Switching the visible screen does not retarget that default. Scopes expire and are revoked on completion, cancellation and backend teardown.

Codex runs read-only with shell, spawning, plugins/apps and inherited MCP servers disabled. Startup verifies the shell-tool gate, agent-spawning restrictions and disabled apps/plugins. It accepts newer Codex versions where `unified_exec` stays enabled: `shell_tool=false` prevents command-tool registration for either execution backend. Claude verifies the pinned SDK and required CLI flags before launch, then checks SDK initialization and the exact connected MCP tool surface before enqueueing any prompt. Missing capabilities, disconnected MCP, or unexpected tools produce a repair message. It exposes only read tools and scoped Choro MCP, with both permission callbacks and pre-tool hooks enforcing the role. Studio restrictions apply independently of Full Access. Unsupported providers fail with a compatibility message. Studio roles remain confined to their scoped design operations.

System workspaces use this same tool surface with `manifest.system_workspace=true`. Their scopes allow draft `set_system`, `system_details`, rename, and local asset additions, and forbid screen creation or document writes. System selection and publication remain native host actions. Agent completion still requires a current specimen screenshot and review receipt.

MCP tools:

- `studio_context`: fixed scope, effective tokens, recipes, revision and screens.
- `studio_project_read`: contained, read-only repository context without a shell.
- `studio_read`: design documents, assets and recovered drafts.
- `studio_apply`: revisioned, idempotent mutations with host scope validation.
- `studio_snapshot`: requires screen ID, revision, and fingerprint; queues an exact saved-revision image using frozen assets and reports pending when unavailable. Successful reads record a screenshot receipt.
- `studio_review`: records concrete passed-review findings only after a screenshot read for the current content. Both providers reject completion when an affected screen lacks a current passed review; subsequent changes invalidate its review.
- `studio_handoff_read`: immutable implementation manifest, tokens, screens and assets. Asset listings include the local bundle path for efficient copying; individual assets can also be read as base64.

Tool listing and dispatch both enforce Studio restrictions. Mutation paths reject traversal and symlink components. Agents cannot issue or broaden their own scopes. Handoffs live outside the checkout and are resolved through the original Choro project, so untracked designs remain accessible from Solo worktrees. Later edits do not change an existing snapshot. Handoffs freeze the chosen system’s identity/source metadata, effective tokens and recipes, and bundled assets, including local fonts. Snapshot manifests retain the explicitly captured source document/task references and content and identify sample data and visual-only JavaScript.

## Rendering

Only one visible native WebKit surface hosts the trusted editor. The native left sidebar remains Choro UI. Studio loads the actual pinned VvvebJs 2.0.9 Content/Style/Advanced inspector, property inputs, inline rich-text toolbar, component registry, styles, and undo engine. Upstream files are unchanged; a separate Choro adapter handles local assets, tokens, clean-source saves, isolation, and revisioned persistence. Templates are precompiled with upstream’s compiler and icon fonts are bundled locally. Vvveb’s page manager, gallery, CMS endpoints, cloud AI, and full application shell are not initialized. See `crates/ide-app/assets/studio/vendor/NOTICE.md` for provenance, regeneration instructions, and dependency licenses.

Studio uses the shared native webview host. Screen and token menus, composer dropdowns, and dialogs hide the native page temporarily without reloading its editing document. Overlapping menus keep it hidden until their final dismissal. Opening another screen replaces the editor; All screens, another project, and other workspace destinations release it. App-level routes such as Settings temporarily hide it through the shared route suspension. Studio document polling and automatic overview thumbnail batches pause while its workspace is not visible; an already-running batch drains and its helper exits. A lightweight request check continues so a running agent can obtain an explicitly requested review screenshot without reopening the editor.

Edit displays a sanitized clone: authored scripts/handlers are removed and a leading CSP blocks script execution. The same-origin editor frame permits trusted parent event listeners. Preview replaces it with an opaque-origin sandbox that runs only `prototype.js`. CSP blocks network access, form submission and external resources. Native IPC accepts only the trusted wrapper's exact URL and session. Saving always serializes the clean source model, never the running prototype DOM.

The overview virtualizes rows and uses content/viewport/assets/token-keyed PNGs. Visible cards receive rendering priority. Stale cards retain the last valid image while updating. **Refresh screen previews** retries failed jobs.

Saved revisions retain source and assets in application data, so requested old screenshots never pick up newer files. Queued revision requests share the serial renderer. A bundled Swift helper uses one serial offscreen WebKit renderer and exits after its queue drains. A process-side deadline bounds each job; the parent also has a watchdog. It does not expose a server. Development builds embed the helper; app bundles ship it beside the main executable and include it in signing. It targets macOS 13 or later.

## Verification

The original Studio implementation recorded the following checks (these counts are historical, not a full rerun for named systems):

- Full `ide-core` suite: 376 passed, 4 ignored.
- MCP suite: 37 passed, including fail-closed tool listing/dispatch without project state.
- Claude bridge policy tests: 8 passed. Desktop provider/protocol regression tests: 15 passed, including the new compatibility checks. Desktop application and test compilation passed.
- Real offscreen WebKit integration: selection, text edits, immediate drafts, token edits, autosave acknowledgement, correlated Implement flush, failed-save retry, neighboring-content preservation, undo/redo, inactive Edit scripts/handlers, CSS serialization, Preview origin/network isolation, and clean source after Preview.
- With the upstream Vvveb bundle, a 50-screen fixture rendered serially in 7.9 seconds on the development Mac and exited when idle. Helper peak resident memory was approximately 85 MiB. This excludes WebKit's separate processes and is not a total app-memory measurement.

The upstream-editor checks also cover inspector focus preservation, custom-token undo, partial-text formatting, and source round trips. Interaction tests and thumbnail benchmarks use the bundled offscreen helper; no user-managed server is needed.

For the named-system update, 37 Studio core tests and 28 Claude bridge tests passed. Offscreen WebKit checks covered the generated specimen, token and recipe control activation, script blocking, and the regular editor Preview regression. Native library/sidebar/review-dialog captures were unavailable, so the visual reviewer requested recapture and gave no native fidelity verdict. These checks do not establish a live-provider or native end-to-end result.

Commands:

```sh
cargo test -p ide-core --lib
cargo test -p ide-mcp
cargo check -p ide-app
cargo test -p ide-app --bin choro --no-run
node --test crates/ide-app/assets/agent-chat/claude_delegation.test.mjs
python3 crates/ide-app/assets/studio/editor.test.py /path/to/choro-studio-thumbnail --benchmark
```

The running desktop application, a signed distribution bundle, and a live Codex/Claude design conversation were not exercised in this pass. Before release, verify those flows and measure total WebKit/app resource use on representative projects. This implementation uses HTML/CSS and isolated visual JavaScript; React editing and component authoring are outside this release.
