# Simplify Review — Suggested Changes (Not Applied)

**Date:** 2026-08-20
**Scope:** Uncommitted Orbit feature changes on `dev` (~4,900 lines across 38 files, including the new `orbit.rs` state/UI/store files).
**Method:** Four parallel review passes — reuse, simplification, efficiency, altitude. No correctness/bug hunting (that's `/code-review`). No files were changed.

---

## High impact

### 1. Stop smuggling the Orbit grant through `AgentCapability.invocation`
- **Where:** `crates/ide-app/src/ui/center/agent_chat_runtime.rs:1480`, `crates/ide-app/src/ui/center/agent_launcher.rs:345`
- **Problem:** The per-turn Orbit invocation UUID is minted into the `invocation` field, which is documented as the composer's provider-specific slash token. The value is sometimes empty (not yet minted), sometimes a UUID, never a slash token — which is why `is_orbit()` guards had to be added in `insert_agent_chat_command_invocation` (`mod.rs:1196`) and `agent_launcher.rs:164`. The struct is serialized into the capability cache file and cloned freely across the composer, launcher, chips, and tags, with no type-level defense against the UUID leaking.
- **Suggestion:** Hold the minted ID in a turn-scoped field (e.g. `orbit_pending_invocations: HashMap<AgentId, Uuid>`) and pass it to the submission-text builder explicitly. The special-case guards then collapse to "this source has no token".

### 2. Remove the re-entrant "create invocation, then resubmit" send path
- **Where:** `crates/ide-app/src/ui/center/agent_chat_runtime.rs:~1420–1500`
- **Problem:** The chat send path creates the Orbit invocation in a spawn, then calls `submit_agent_chat_message_for_surface` **again** with a mutated selected-command entry, guarded by `orbit_invocations_pending`, an "is the chip still selected?" re-check, and a compensating `complete_orbit_invocation` for orphaned grants. ~80 lines, three failure branches, one extra `HashSet` field on `CenterArea`.
- **Suggestion:** Do what `agent_launcher.rs` (~330, ~545) already does for the same feature: mint `invocation_id` locally, set it on a cloned submission command, create the invocation row inside the existing send spawn. Removes the recursion, `orbit_invocations_pending`, and the orphan-completion branch.
- **Related:** The store-open + `create_orbit_invocation_with_id` block and its `"Could not start Orbit access: {error:#}"` string are duplicated between `agent_launcher.rs:552` and `agent_chat_runtime.rs:1441` — extract one shared helper.

### 3. Kill the per-frame deep clones in the Orbit table render
- **Where:** `crates/ide-app/src/ui/center/services.rs:217–222, 322–327, 334, 359–360, 442`
- **Problem:** Records are deep-cloned three times per frame (`records(...).to_vec()`, `filtered`, `groups` — each record holds a `BTreeMap<String, serde_json::Value>`), and field definitions are cloned once per section plus once per row. The center pane re-renders on every keystroke/hover/agent event, so a 500-record module does thousands of map+string clones per frame.
- **Suggestion:** Work by reference throughout: `filtered: Vec<&OrbitRecord>`, `groups: BTreeMap<&str, Vec<&OrbitRecord>>`, iterate `fields.iter()` directly (only clone the `SharedString` label/key actually used per cell).
- **Same pattern elsewhere:**
  - `crates/ide-app/src/ui/settings/orbit_page.rs:13–23` — every module cloned three times per render (`.to_vec()`, `active`, `archived`); partition by reference instead.
  - `crates/ide-app/src/ui/services_panel.rs:92–105` — `available_custom` deep-clones every module (dragging `agent_job`, up to 24 000 chars) to fill a name-only dropdown; collect `Vec<(Uuid, SharedString)>` and precompute a `HashSet` of enabled bindings instead of a per-module linear scan.

### 4. Batch the DB access; stop full-reloading on every mutation
- **Where:** `crates/ide-app/src/state/orbit.rs:188–230` (`refresh`), `crates/ide-core/src/local_store/orbit.rs:705–733`, `crates/ide-core/src/local_store/maintenance.rs:553`
- **Problem:**
  - Every store method builds a brand-new Turso connection plus two PRAGMA round-trips (`LocalStore::connect`).
  - `refresh` issues `1 + P + (P×M)` separate connection setups (modules, bindings per project, records per enabled binding).
  - `load_orbit_modules_async` is an N+1: one query for IDs, then two more per module.
  - Every mutation (`set_enabled`, `save_module`, `set_archived`, `save_record`, `delete_record`) ends with a full `refresh(projects)` — toggling one checkbox re-reads every module, binding, and record for every project. The schema's per-binding `data_revision` (designed for targeted invalidation) is bypassed; the `generation` counter exists solely to discard the overlapping full reloads.
- **Suggestion:**
  - One batched `LocalStore::load_orbit_snapshot(&[ProjectId])` that opens a single connection.
  - Two queries total for modules (modules + all fields, grouped in memory).
  - For mutations, apply the returned value in place (the store methods already return the new binding/record) and reload only the affected `(project, module)` slice; add `OrbitState::refresh_module(project, module_id)` reconciling against `data_revision`.
- **Also:** `restore_archived_field_ids_async` (`local_store/orbit.rs:676–709`) does one SELECT per field (up to 24) per module save — replace with a single SELECT into a `HashMap`. Export snapshot (`maintenance.rs:614–626`) runs `P×M` mostly-empty record queries — one query over `orbit_records` would do.

### 5. Delete `ServicesScanKind`; key on `OrbitBuiltin` directly
- **Where:** `crates/ide-app/src/state/services.rs:18`, bridged via `unreachable!()` at `crates/ide-app/src/ui/center/services.rs:50–53`
- **Problem:** `ServicesScanKind::{Environment, Integrations}` is a structural clone of `OrbitBuiltin::{Environment, Integrations}`. The builtin two-way split is re-spelled by hand in at least seven places: scan dispatch (`state/services.rs:139`), header title/pluralization, scanning/empty-state copy (`center/services.rs:107, 120, 131, ~990`), sidebar row name+icon, "Add to Orbit" menu labels (`services_panel.rs`), and the hardcoded rows in `settings/orbit_page.rs`. Adding a third builtin means seven edits plus a newly reachable `unreachable!()`.
- **Suggestion:** Key `ServicesState` on `OrbitBuiltin` and hang all per-builtin data off one table: `impl OrbitBuiltin { label(), icon(), description(), scanning_message(), empty_title(), empty_body(), detect(&Path) }` (or a `const BUILTINS: &[BuiltinSpec]`). All sites become data lookups.
- **Sub-point:** `state/services.rs:162` — `invalidate` special-cases `Environment` to clear `env_cache` for all projects; owning the cache per built-in scan record removes the branch.

### 6. Collapse the five copy-paste async mutators in `OrbitState`
- **Where:** `crates/ide-app/src/state/orbit.rs:245, 299, 335, 367, 409`
- **Problem:** `set_enabled`, `save_module`, `set_archived`, `save_record`, `delete_record` share the same 25-line skeleton (`saving = true; error = None; notify; spawn → background → match { Ok => refresh, Err => error = format!(...) }`) with only the store call and error prefix varying.
- **Suggestion:** One private `run_mutation(label, op, on_ok, cx)` helper; each public method becomes 3–8 lines.
- **Also — stop threading `projects` everywhere:** every mutator takes `projects: Vec<ProjectId>` only so `refresh` can reload, forcing ~8 call sites (`services_panel.rs:76, 287`, `orbit_page.rs:689, 698`, `center/services.rs` save/delete, `agent_chat_runtime.rs:1640, 1720`) to repeat `workspace.read(cx).projects.iter().map(|p| p.id).collect()`. `state/docs.rs:117` and `state/tasks.rs:28` store the `Entity<Workspace>` on the state struct for exactly this reason — do the same and drop the parameter.

### 7. One timeline upsert + one card conversion instead of two hand-rolled copies
- **Where:** `crates/ide-app/src/ui/center/agent_chat_runtime.rs:1661` and `:1744`
- **Problem:** `complete_orbit_invocation_for_agent` and `undo_orbit_update` each contain the same ~25-line block (find `OrbitUpdate` in timeline by `invocation_id`, replace-or-push, `persist_timeline_item`) plus an 8-field manual `OrbitUpdateCard` construction (`OrbitUpdateCard` is a strict field subset of `OrbitInvocationUpdate`).
- **Suggestion:** `state/agent_chat/timeline.rs:104–215` already has five sibling `upsert_timeline_*` helpers with the identical shape — add `upsert_timeline_orbit_update(timeline, card)` and `impl From<OrbitInvocationUpdate> for OrbitUpdateCard`.
- **Altitude note (grant lifetime):** invocation closure depends on two hand-placed call sites (`TurnFinished` handler + failed-dispatch fallback). `AgentChatEvent::WorkFinished` — documented as covering settlements `TurnFinished` misses — is not hooked, and agent stop / session close / app quit aren't covered, leaving grants open until the 2-hour TTL. Prefer binding the grant to the turn/session lifecycle (or one subscriber handling both events) over per-call-site cleanup.

---

## Medium

### 8. `execute_transaction_returning<T>` helper
- **Where:** `crates/ide-core/src/local_store/orbit.rs:242, 295, 332, 458, 549` (plus pre-existing `api.rs:209`)
- **Problem:** Five new copies of the `let x = RefCell::new(None); execute_transaction(...) { x.replace(Some(...)) }; x.into_inner().context("…produced no…")` dance — 12 lines of ceremony per write.
- **Suggestion:** Add `execute_transaction_returning<T>(conn, f) -> Result<T>` next to `execute_transaction` in `local_store/schema.rs:1496`; each write collapses to a single `.await?`.

### 9. Import path re-types raw INSERT SQL
- **Where:** `crates/ide-core/src/local_store/maintenance.rs:353, 390, 411, 431`
- **Problem:** Full column lists and bindings for `orbit_project_modules`, `orbit_records`, `orbit_invocations`, `orbit_mutation_batches` are re-typed in the import path, mirroring `local_store/orbit.rs:923, 1213, 1342, 1566` with no compiler link. Every other domain in the same import block uses `insert_*_async` helpers.
- **Suggestion:** Add `insert_orbit_record_async` / `insert_orbit_invocation_async` / `insert_orbit_mutation_batch_async` in `orbit.rs` and call those.
- **Also:** `maintenance.rs:336` — module import calls `save_orbit_module_async` (validates, dedupes, bumps revision, stamps `now`) then immediately UPDATEs those values back. Use one verbatim insert like the neighboring loops.

### 10. Dedupe capabilities by priority, not name-matching — and memoize
- **Where:** `crates/ide-app/src/ui/center/mod.rs:985–1020`, `crates/ide-app/src/state/orbit.rs:162`
- **Problem (altitude):** `capability_shadowed_by_orbit` compares lowercased Riff titles against Orbit module names — two unrelated identity spaces. Rename a module and a stale Riff silently reappears; each new source needs another pairwise predicate. `AgentCapabilitySource::priority()` already encodes the precedence (and this diff renumbered it wholesale to insert `Orbit`).
- **Problem (efficiency):** `agent_chat_slash_capabilities` is called from three render/query paths while the user types; per call it deep-clones every visible module (including `agent_job`, ≤24 000 chars), formats multi-KB instruction strings, and sits atop two synchronous `fs::read_to_string` + JSON parses on the UI thread.
- **Suggestion:** Build the list through one `BTreeMap<slash_key, capability>` keeping the lowest-`priority()` entry (shadowing falls out for all sources). Memoize the derived `Vec<AgentCapability>` on `CenterArea` keyed by `(provider, project, orbit generation)` — `OrbitState.generation` already exists.

### 11. Shared context-tag helpers
- **Where:** `crates/ide-app/src/ui/center/mod.rs:1046, 1089, 1219`
- **Problem:** `escape_choro_orbit_context` is byte-for-byte `escape_choro_riff_context` with the tag swapped; the tag literal appears in ~6 places across wrap/escape/strip; forgetting the strip arm renders the hidden block in the transcript, forgetting the escape arm makes the delimiter user-injectable.
- **Suggestion:** One `const` tag per source, shared `wrap_context_block(tag, body)` / `escape_context_block(tag, value)`, and make `visible_agent_chat_submission_text` strip any leading `<choro-…-context>` block generically (or drive the `if/else if` chain off the constant array).

### 12. Reuse existing UI helpers instead of new copies
| New code | Existing helper to use |
|---|---|
| Hand-rolled loading spinner block, `center/services.rs:18` | `style::loading_state(title, description, seed, cx)` (`style.rs:2343`) |
| Rose error banner ×5 (`orbit_page.rs:50, 370, 382`; `center/services.rs:290, 793`) | one `style::error_banner(cx)` (or existing `chat_notice`, `style.rs:2525`) |
| `orbit_form_field` (`orbit_page.rs:805`) + `orbit_editor_field` (`center/services.rs:1588`) | build on `tasks_panel/helpers.rs:243 form_field_label` (repo already has 4 copies of this stack) |
| Lane header, `services_panel.rs:395 render_lane` | promote `asset_lane` (`designs_panel.rs:700`) to a shared helper |
| `normalize_orbit_field_key` (`local_store/orbit.rs:61`) — 5th slugify loop in repo | one `ide_core::slugify(input, separator, max_len)` (must live in ide-core; `docs.rs:1519`, `tools.rs:1288`, `lanes.rs:129`, `operations.rs:441` can adopt later) |
| `read_invocation_id` (`ide-mcp/tools.rs:222`) | existing `required_trimmed_arg` (`tools.rs:1067`) + `Uuid::parse_str` |
| `preferred_orbit_selection` (`state/orbit.rs:436`) + `preferred_services_tab` (`state/services.rs:169`) — added in this diff with near-identical tests | one generic `preferred_selection<T: PartialEq + Clone>` in `state/mod.rs` |
| `OrbitFieldKind` label match ×2 (`orbit_page.rs:829`, `center/mod.rs:870`) + `storage_label` | one `pub fn label(self) -> &'static str` on `OrbitFieldKind` in ide-core |

### 13. Dead code added by this diff
- **Where:** `crates/ide-app/src/ui/style.rs:349` (`context_panel_selectable_row_button`), `:366` (`orbit_table_row_button`)
- **Problem:** Zero call sites. `render_orbit_module_row` hand-rolls the row the first helper was for; `orbit_section_toggle_button` (`style.rs:392`) is the second minus one `.hover` line.
- **Suggestion:** Route `render_orbit_module_row` through `context_panel_selectable_row_button` and delete `orbit_table_row_button` — or delete both.

### 14. Services panel per-frame waste
- **Where:** `crates/ide-app/src/ui/services_panel.rs`
- `74–75`: `visible_modules(project)` computed twice per frame (once inside `selected()`, once directly) — compute once and derive selection from the slice.
- `107–111, 230–245`: builtin/custom partition computed three ways (two counts, two mirrored `filter_map` blocks, plus an `!any(Custom)` scan) — one `partition()` into `(builtins, customs)`, use `customs.is_empty()`.
- `287–293`: `projects` Vec rebuilt from `workspace.projects` inside `render_orbit_module_row`, per row per frame — pass the Vec built at line 76.
- `24, 299, 321–326`: hand-rolled `hovered_module` field + `on_hover` calling `cx.notify()` unconditionally (re-render on no-op hover events) — use `.group("orbit-module-row")` + `.group_hover(..., |b| b.visible())` as in `git/status_list.rs:415/478`.

---

## Smaller / judgment calls

- **"Analytics" name-sniffing** — `center/services.rs:231`: empty-state copy keyed on `module.name.eq_ignore_ascii_case("Analytics")`, a user-editable name. Add `empty_prompt: Option<String>` to `OrbitModuleDefinition` (the analytics template supplies it as data), or just use the generic branch.
- **Schema contract composed in the view layer** — `center/mod.rs:858`: `orbit_module_capability` hand-formats the field list/kinds/section the agent reads, while `orbit_read` in ide-mcp serializes the same schema as JSON — two descriptions of one contract across crates, plus a hardcoded `"View: grouped table"` that contradicts `OrbitViewType` the moment a second view exists. Put `fn agent_context(&self) -> String` on `OrbitModuleDefinition` in ide-core, next to `validate_orbit_module`, reusing `OrbitFieldKind` labels.
- **`orbit_table_cell` flags** — `center/services.rs:1442`: called once, with `header = true`; `primary` is inert; `.text_size(if header { text_ui() } else { text_ui() })` has identical branches. Rename to `orbit_table_header_cell(text, cx)`, no flags.
- **`orbit_value_table_preview`** — `center/services.rs:1554`: returns `(String, Option<Vec<String>>, usize)` all derivable from one `Vec<String>`; also materializes the full tooltip line list for every cell every frame even though tooltips only show on hover. Return the `Vec` (or first line + count) and build tooltip lines lazily inside the `tooltip(move |…|)` closure.
- **Search filter allocations** — `center/services.rs:1399–1407` (`orbit_record_matches`): `orbit_value_display(value).to_lowercase()` allocates two Strings per field per record per keystroke. Compare on borrowed `&str` case-insensitively, or precompute a lowercased haystack per record when records are populated.
- **Composer mode conflict checked post-hoc** — `center/mod.rs:1085` + `agent_chat_runtime.rs:1381`: `orbit_target_conflict` is a pairwise "Orbit + #agent" guard at submit time over three independent selection fields; the check count grows quadratically and other combinations are unvalidated. Longer-term: one exclusive `enum ComposerIntent { Plain, Capability(..), Target(..), Preview }` field, enforced at selection time.
- **`refresh_all`** — `state/orbit.rs:184`: one-line alias for private `refresh` with the same signature. Make `refresh` pub, delete the wrapper.
- **`OrbitViewType`** — `local_store/mod.rs:212`: one-variant enum with `storage_label`/`from_storage_label`/serde plumbing; UI hardcodes "Grouped table" anyway. A validated TEXT constant would do until a second view type exists. (Persisted schema — lowest priority.)
- **MCP test fixture** — `ide-mcp/tools.rs:~2168–2440`: three Orbit tests repeat ~25 lines of identical setup. One `orbit_fixture()` cuts ~50 lines.
- **Settings opener copy-paste** — `ui/settings.rs:395` (`SettingsView::new_orbit` = `new_remote` with one identifier changed) + `root_view/settings.rs:119` (`open_orbit_settings` = `open_remote_settings` likewise). Make `new_in_section` pub and collapse to one `open_settings_section(section, window, cx)`.
- **`orbit_page.rs` misc** — `471–474, 148–149`: six `let x = cx.entity().clone()` aliases where `cx.listener` (already used at 341/457) suffices. `725`: `badge: Option<&str>` where all four call sites pass `Some` — make it `&str`. `799`: `orbit_form_field` applies height to both the Input and the wrapper div — one is enough.
- **Environment detector scaffolding** — `ide-core/services.rs:171`: `detect_project_environment_files` copies the sub-dir collection, root-injection, sort/dedup preamble, and root-first comparator verbatim from `detect_project_services`, and the `rel_path` derivation from `detect_in_dir`. Extract `sub_app_dirs_in_order`, `sub_app_rel_path`, `sort_root_first` shared by both.

---

*Generated by `/simplify` (suggest-only mode). To apply, pick items and ask — e.g. "apply high impact only" or "apply everything except 1 and 2".*
