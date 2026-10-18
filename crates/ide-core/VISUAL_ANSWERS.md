# Visual answers

Implementation status: **QA feedback implemented; combined source checks passed.
Informational answers belong in the conversation;
only explicit choices belong beside the composer. Native surfaces hide outside
their visible agent/result region. Updated manual visual QA remains with the user.**

`VISUAL_ANSWERS_READY` is true: `visual_present` and `visual_read` are advertised,
and Codex and Claude conversations receive instructions generated from the same
component library the renderer bundles. Set it to false to withdraw the feature;
the tools, instructions and composer surface all follow that one gate. Preview
and existing HTML visualizations are unchanged.

## How it works

1. The agent calls `visual_present` (Choro MCP). The document is validated and
   stored in LocalStore under the caller's bound project and agent identity.
2. Choro checks a cheap publication stamp (row count and newest row id) when the
   conversation changes (at most every 0.9 s) and when a turn finishes. Only when
   it changes does it read answer heads (id and latest revision) and load changed
   documents. Full history is never polled.
3. A new answer or revision gets a transcript card. Informational results open
   inside the conversation; explicit choices open beside its composer. Placement
   uses `VisualDocument::requires_user_response()` (explicit `options`), not
   prose, local filters, or optional follow-up actions. Answers never switch the
   selected chat or take keyboard focus.
4. The renderer is a locally bundled OpenUI app in the shared native web host
   (`WebPreviewIntent::VisualAnswer`, `choro-visual://` document). It parses the
   program with the OpenUI SDK parser built from Choro's library and refuses
   unknown components, Query/Mutation, `$variables`, unresolved or orphaned
   statements and references to undeclared datasets or columns.
5. Searching, sorting, series toggles and option selection stay local. They are
   saved as interaction state and never start an agent turn.
6. Sending is explicit and native: **Use this style** (select actions), follow-up
   action buttons, or Enter/the send arrow beside a choice panel (a refinement).
   An informational answer leaves ordinary composer sending alone. Choro records the
   receipt with `begin_visual_submission_with_draft` before dispatching through
   the normal conversation path, so mode, permissions and queueing are unchanged.

## Surface and lifecycle

- **Conversation result**: informational tables, charts and summaries appear in
  their transcript card. Their local controls never turn them into choice
  panels. The native content is shown only while its complete rectangle is
  inside the visible transcript; partly scrolled content hides instead of
  painting over the header or composer. It returns when fully visible.
- Inline content fits the measured transcript height. When a small window or
  tall composer leaves too little room, a native explanation and **Expand**
  replace the surface. This uses the same shared host; it is not pixel clipping.
- **Docked choice panel**: native header (title, summary, kind or revision,
  Expand), OpenUI content fitted to the height the renderer reports (at least
  72 px, at most min(420 px, 45% of the window height); taller content scrolls
  with a visible scrollbar), a native action row, then the normal composer text
  box and footer. Inline results use the same measured height.
- **Band row**: the attached choice sits in the composer's own Band row (inside
  the composer, with the delegation status, recipient, activity and recovery
  controls; shown even when no Band is active): icon, title, **Hide** (while
  shown) or **Open** (while collapsed), and **×** to detach. The row wraps; the
  choice keeps its actions together and only its title truncates. A Preview
  notice appears under the row. No separate strip exists; the Solo lane keeps
  its own strip on the composer's top border. Informational results never
  appear in the Band row.
- **Expanded**: the same panel fills the conversation column; the transcript
  returns unchanged on Escape or Dock. Restart always restores docked.
- **Collapsed**: the choice waits in the Band row; the composer sends
  normal messages. Every answer keeps its transcript card with **Open visual answer**.
- **Detached** (× in the Band row): the choice leaves the composer and the row;
  selection, note, view and composer text are kept, and `interaction.detached`
  is persisted for that revision, so refresh, chat switches and restart do not
  reattach it. It returns only from its transcript card (which clears the flag).
  A new revision published by the agent (`visual_present` with the answer's
  id) starts attached and opens, as any publication does; it carries the note
  and any still-valid selection. Detaching applies to one revision only.
- **Outside the conversation**: New agent, overview, other agents/projects/tabs,
  Files, Docs, Settings, and native dialogs cannot reveal an old visual answer.
  Native visibility follows the actually rendered conversation, not merely a
  selected agent ID. Transcript-local layout proofs and immediate rejection of
  out-of-bounds placement prevent old native bounds from surviving scrolling
  or cached center frames. Detail drawers and the context picker hide content
  they cover; native dialogs, sheets and PopupMenus retain their suspension.
- **Preview** keeps the shared surface. Opening Preview saves and collapses a
  shown answer first. While Preview is open, **Open visual answer** shows “Close
  Preview to open this visual answer.” and changes nothing else. Answers never
  reopen by themselves after Preview closes. `preview_open` is never used.
- Pending approvals, structured questions and plan decisions replace choice or
  expanded panels without changing their saved state. Informational results may
  remain readable inside the bounded transcript above the native decision UI.
- Tab from the composer moves into the answer; Tab past either end of the
  content returns to the composer.
- **Escape** has one handler, `composer_escape`, and each press reaches it by
  exactly one route chosen by focus: the text box (gpui-component binds
  `escape` → `Escape` only in its `Input` context; the composer frame captures
  it), the panel's native controls (no `Input` context, so that action never
  fires; the panel's key-down listener), or the web content (WebKit owns the
  keyboard; the renderer only posts `escape`). Order: a pending handoff
  swallows it, the context picker closes, then `visual_escape_step`. While the
  conversation is Running or Cancelling, Escape is the normal stop (first
  graceful, second force) and never touches the answer. Otherwise an expanded
  answer docks (one press), then a docked choice collapses into the Band row so
  the composer is ordinary chat. From panel controls or web content, focus then
  returns to the composer unless the answer only docked. Held-key repeats are
  ignored. Nothing is discarded or sent. A GPUI test (`ui-layout-tests`)
  proves the routes are disjoint and that each press takes one step.
- **Design comparisons** are laid out from explicit hints, never by measuring
  the sandboxed frames: `DesignOptions(scale?, height?, width?)` with
  `"control"` (default, actual size), `"section"` (1024 px wide, scaled to the
  column) or `"page"` (1280 px wide). `height` (60–6000) is the design's full
  natural height; `width` (320–2560) overrides the layout width. Out-of-range
  hints are rejected with a reason. Frames are never measured, so the agent
  must give `height` whenever a design is taller than its scale's default
  (control 132, section 560, page 2000); otherwise the rest is cut off. Columns are chosen from the option count
  and available width without orphans (2: 2|1, 3: 3|1, 4: 4|2|1); a preview
  taller than its slot scrolls inside the option (wheel, or PageUp/PageDown on
  the focused option) with a visible scrollbar and a “Scroll for more” cue.
  Frames stay inert (no pointer, no focus, no scrollbar of their own), so a
  click selects and Escape is never swallowed. Each preview's scroll is saved
  per option id in `fields.stages`, in design pixels, so it survives detach and
  reopen, chat switches, restart and expand/dock (where the zoom changes). It
  is read synchronously by host snapshots, restored after each layout change
  and clamped to what the current preview can show without overwriting the
  saved value; ids missing from the revision and invalid values are dropped.

## Contract and guarantees

Shared types live in `ide-core::visual_answers`; persistence in
`local_store/visual_answers.rs`; host IPC in `ui/center/visual_answer_surface.rs`;
conversation state in `ui/center/agent_chat_visual_answer.rs`; native chrome in
`ui/center/agent_chat_visual_answer_panel.rs`; the Band-row attachment (joined
to `experts.rs` `render_expert_composer`), detach, the Escape handler and
measured height in `agent_chat_visual_answer_attach.rs`;
comparison layout in `web/visual-answer/src/compare.ts`; renderer in
`crates/ide-app/web/visual-answer/`.
Submission and notes live in `agent_chat_visual_answer_submit.rs`, ordered
writes in `agent_chat_visual_answer_jobs.rs`, and the tested routing, merge and
outgoing-surface rules in `agent_chat_visual_answer_flow.rs`.

- Publications are immutable revisions with idempotent `operation_key` retries;
  updates need the latest `expected_revision`. Historical revisions stay readable.
- A revision keeps the previous view state when its dataset names, column keys
  and types are unchanged (scroll only when the program is also unchanged); the
  renderer drops restored state that no longer applies.
- Renderer messages must echo the surface generation, answer id and revision of
  the surface that received them, and are checked again against the
  conversation's current answer before use. A registered outgoing surface may
  send one final snapshot for its own unchanged revision only. It cannot update
  a newer revision or another answer. Renderer messages cannot carry a receipt.
  `escape` is a request the host decides; `size` carries a positive content
  height (capped at 20 000) and is applied only to the reporting revision, and
  ignored while expanded.
- Before replacement or sending, the host requests a complete renderer snapshot
  and waits at most 300 ms. The outgoing view remains hidden with its single
  WebKit lease until the reply or deadline. An unresponsive renderer falls back
  to the last state received; the next surface and normal chat stay usable.
- Choice-panel composer notes are cached immediately and saved after 400 ms of idle
  time, independently per answer. A save already in flight cannot overwrite a
  newer local note. Saved notes restore only into an empty composer; save or
  size failures are visible. Attachments and chips use normal chat dispatch.
- A send is bound to the answer revision the user saw. If that answer changes
  during its snapshot wait or queued reservation, the send is cancelled visibly
  and the note remains. Repeated Enter or send clicks never become a plain
  duplicate while that visual submission is in flight.
- State writes, submissions and receipt updates for one conversation run in
  order. Newer revisions and interaction revisions always win over stale reads.
- Client state is bounded at 64 KiB; the receipt separately at 48 KiB, so a full
  32 KiB note can always be sent. Receipts include a readable summary of the
  current view (search, sort, hidden series, selection), bounded to 2 KiB.
- A submitted revision is a record: it can be reopened, filtered and scrolled,
  but its selection and note cannot change.
- Accepted dispatch marks the receipt sent; definite rejection clears it for
  retry. A restored `sending` receipt shows “It was sent” and “Send again”; Choro
  never resends on its own.
- Null values stay missing: tables mark them, aggregates skip them, and charts
  break the line or omit the bar instead of plotting zero.
- Design examples render in `srcdoc` frames with an empty sandbox and their own
  CSP. The answer document allows no network (`connect-src 'none'`), navigation
  or forms, and no CDN. OpenUI's optional telemetry is not enabled.

## Build and packaging

`crates/ide-app/web/visual-answer`: `npm ci`, `npm test`, `npm run build`. The
build type-checks, bundles `dist/visual-answer.js` and `.css`, and generates
`dist/instructions.md` and `dist/library.json` from the library. All four are
embedded with `include_str!`; `scripts/bundle.sh` rebuilds and tests them before
Cargo (or reuses them with `CHORO_SKIP_VISUAL_ANSWER_BUILD=1`) and adds their
licenses to the packaged inventory. `.npmrc` disables install scripts.

## Manual test checklist

None of these have been run yet. Run each with both Codex and Claude, in light
and dark themes. Record results here; do not mark an item done without doing it.

| Done | Case | Type or do this | Expected result |
| --- | --- | --- | --- |
| [ ] | Table | “Using sample data, compare six projects by owner, status, and open issues.” | A searchable, sortable table inside the conversation, labeled as sample data; composer stays normal and local interactions start no turn. |
| [ ] | Trend | “Using sample data, show daily signups for two products over 14 days.” | A two-series line chart with toggleable series, Show data, units and time range. |
| [ ] | Categories | “Using sample data, compare support tickets across five categories.” | A bar chart with readable category labels and units. |
| [ ] | Summary | “Using sample data, show this week’s signups, active users, and change from last week.” | Metrics computed from data, followed by the supporting chart or table and a time range. |
| [ ] | Visual options | “Show me four visual variations of a Create project button.” | Four real designs in the composer panel; selecting one sends nothing. |
| [ ] | Refinement | Select a direction, then type “Make the selected direction rounder and quieter.” and press Enter. | The same answer gets a new revision; still-valid selections and the view are retained. |
| [ ] | Selection | Select a variation, then click **Use this style** twice quickly. | One message and one receipt; no duplicate turn. Mode and permissions unchanged. |
| [ ] | Follow-up | With a chart open: “Focus that chart on the last seven days.” | A revised chart; the agent received the answer id, revision and current view. |
| [ ] | Expand | Expand, change a filter, scroll, then press Escape. | Docked again with the filter, scroll position and composer text retained. |
| [ ] | Double Escape, idle | With four button options expanded and a note typed, press Escape twice — from the composer, then repeat from inside the options and from the panel's Expand button. | First press only docks; second collapses the choice into the Band row. The composer sends normal messages; selection and note remain; nothing is sent. |
| [ ] | Double Escape, working | While the agent is running with a choice open (docked or expanded), press Escape twice from the composer, then from inside the options, then from a panel button. | Each route: first press requests a graceful stop, second force-stops — as without a visual answer. The answer stays where it was. |
| [ ] | Detach and reopen | Select an option, scroll a tall preview, type a note, click **×** in the Band row, send an ordinary message, switch chats and back, restart. | The row no longer shows it; the message is ordinary; it does not reattach. **Open visual answer** in its card restores selection, note and each preview's scroll. |
| [ ] | New revision | After detaching, ask the agent to revise that answer. | The new revision attaches and opens with the note and still-valid selection. |
| [ ] | Band row | During an active Band (a Bandmate running), open a choice, hide it, then narrow the window. Repeat with no Band and in a Solo lane. | The choice's Open/Hide/× sit in the same composer row as the Band status and activity, wrapping as a group; only the title shortens. With no Band it is alone in that row; the Solo strip is unchanged. |
| [ ] | 4 buttons | “Show me four visual variations of a Create project button.” | Four columns when wide (2×2 when narrower); the panel ends just below the cards — no blank band before the footer. |
| [ ] | 2 heroes | “Compare two hero sections for a project-management landing page.” | `DesignOptions("section", …)`; side by side when wide, rows when narrow; each preview scaled, scrollable if taller, Expand gives larger previews. |
| [ ] | 3 layouts | “Show three dashboard layout options.” | Three columns when each stays readable, otherwise three rows (never 2+1). |
| [ ] | Full page | “Compare two complete landing pages.” | `DesignOptions("page", height)` with each page's full height (nothing cut off); previews scroll with a visible scrollbar and “Scroll for more”; PageDown scrolls the focused option without selecting it; Expand shows them much taller; each preview keeps its own scroll across expand/dock. |
| [ ] | Preview | Open Preview, then request a visual answer. Click **Open visual answer**. | Preview is untouched; the answer stays collapsed and says “Close Preview to open this visual answer.” |
| [ ] | Reverse switch | Open Preview while a visual answer is shown. | The answer is saved and collapsed before Preview takes the shared surface. |
| [ ] | Chat switching | Select an option and type a note; switch chats and return. | Selection and note remain; a background chat's new answer does not take focus. |
| [ ] | Restart | Restart with an unfinished answer, including an expanded one. | It returns docked with its saved state; nothing is sent. |
| [ ] | Interrupted send | Quit during a submission, then restart. | “Choro couldn’t confirm…” with **It was sent** and **Send again**; no automatic resend. |
| [ ] | Simple answer | “What does HTTP 404 mean?” | Ordinary text, no visual answer. |
| [ ] | Missing data | “Chart these values: Monday 10, Tuesday missing, Wednesday 15.” | Tuesday is marked missing and not plotted as zero. |
| [ ] | Empty result | “Show an empty sample dataset with columns Project and Open issues.” | A useful empty state without invented rows. |
| [ ] | Restricted interaction | Trigger an approval or structured question while an answer is open. | The native decision UI replaces the panel; the answer returns afterwards with its state. |
| [ ] | Keyboard and themes | Tab into and out of the answer, select with arrow keys, expand and collapse, in both themes and a small window. | Visible focus, readable contrast, reachable composer; no trapped focus. |
| [ ] | Malformed program | Ask the agent to present a program using an unknown component. | Publication returns a correction error before saving; the agent can retry with supported components, and previous answers remain intact. |
| [ ] | Older revision | Create two answers in one chat, reopen the first and ask for a change. | Its new revision comes forward; the other answer remains available. |
| [ ] | Screen containment | With a visual answer visible, open New agent, then return; repeat with another tab or agent and scroll its result past the composer/header. | No visual overlay outside its agent or visible result region; saved state returns on the correct screen. |

## Verification record

Lead review (2026-10-16): `./scripts/release-macos.sh --check` passed, including
the production workspace build. The frontend's 23 tests and its type check,
production bundle and generated instructions also passed on the combined tree.
This does not verify the running app or real provider behavior.

Lead re-review (2026-10-17): the saved UI Designer implemented note persistence,
bounded snapshot handoff, chart limits on resolved series, and one composer send
path. The lead fixed publication ordering for older revised answers. Follow-up
review also resolved duplicate sends during snapshot waits, save replies racing
with local notes, per-answer debounce, submissions queued against superseded
revisions, same-chat outgoing snapshots, and hidden-surface expiry. Regression
tests cover these rules and the current chat-state cancellation paths.

User QA (2026-10-17): the six-project sample was successfully published and read
back through the live Choro tools (six rows, revision 1). The user then reported
that an informational table should be part of the conversation and that the
visual surface overlaid New agent. The saved UI Designer implemented the
placement/containment correction and a follow-up for cached scrolling and short
transcript areas. New real GPUI tests drive list wheel events, disappearance and
return of a previously shown surface, and tall-composer sizing/Expand fallback.
The updated behavior has not yet been manually confirmed in the running app.

Tracking disclosure: the designer reported a Python replacement in
`ui/center/visual_answer_surface.rs` in the first placement revision (the inline
field and related constructors/test). It was inspected directly by the lead,
but those edits may be missing from Choro's Files/review attribution. Subsequent
containment corrections used dedicated edit tools; no files were removed.

The Choro coordination API recovered; the findings were delivered through the
same saved UI Designer's task. No alternate designer was substituted. The
designer's latest result was accepted after the combined code review and tests.

An intermediate release check overlapped those shared-file edits and failed to
compile a partially updated submission module. The fresh check against the
settled implementation passed all seven steps, including the production build,
in 259 seconds. No compilation failures remain on the final tree.

Automated (this integration):

- `npm test` in `web/visual-answer`: 26 tests passed (real OpenUI parsing,
  table search and sort, null handling, charts, design isolation, invalid
  programs, focus handoff, view summary, stale view state, immediate snapshots,
  and chart limits on explicit series).
- `npm run build`: passed (type check, bundle, 6,073-character instructions,
  nine components).
- `cargo test --locked -p ide-core`: 550 passed, 4 ignored; integration 9 passed.
- `cargo test --locked -p ide-mcp`: 63 passed.
- `cargo test --locked -p ide-app`: 976 passed, 6 ignored.
- `cargo test --locked -p ide-app --features ui-layout-tests`: 1,031 passed, 6 ignored.
- `./scripts/release-macos.sh --check`: passed, including both app target checks,
  all default-feature crate tests, UI-layout tests and the production workspace
  build. Existing compiler warnings remain; no new release was published.
- Latest placement/containment correction: the full release check passed on the
  settled implementation in 226 seconds (including the production build).
  Renderer frontend code was unchanged; its previous 26 tests/build remain the
  frontend verification record. No on-screen QA is implied by these checks.
- Packaging script syntax (`bash -n`, `node --check`) and `git diff --check`: passed.
- HTML QA guide: 22 cases and 44 Codex/Claude checkboxes. Every case is labeled
  Conversation · Information, Composer · Question/choice, Both · Test each kind,
  or Conversation · Plain text. Programmatic checks confirmed all labels, saved
  checkmarks, progress tracking and search by placement.

Live Codex tool publication/readback of the six-project table ran successfully;
the user then identified the placement and overlay issues described above.
Not run after these corrections: manual app/visual checks, screenshots and a real
Claude feature conversation. The running installed app was not replaced or
restarted. Do not describe the feature as release-ready until the manual checks
pass; visual QA is explicitly assigned to the user.

### Composer and comparison QA follow-up (2026-10-17)

The saved UI Designer implemented adaptive control/section/page comparisons,
measured content height, visible scrolling, per-option scroll persistence,
the visual attachment in the actual Band context row, persistent detach/reopen,
and one Escape handler for input, native panel controls and renderer IPC. Lead
review requested and verified the Band-row correction, synchronous scroll
snapshots and a GPUI keyboard-routing regression test.

Combined lead verification after those corrections:

- `cargo test --locked -p ide-core -p ide-mcp -p ide-app`: passed. Core: 551
  unit tests and 9 integration tests; MCP: 63; app: 982. Existing ignored tests
  remain ignored (4 core, 6 app).
- `cargo test --locked -q -p ide-app --features ui-layout-tests`: 1,038 passed,
  6 ignored, including the Escape dispatch test.
- Frontend `npm test`: 37 passed. `npm run build`: type checking, local bundle
  and generated component instructions passed (6,810 characters, 9 components).
- `git diff --check`: passed. HTML guide smoke check: 31 distinct journeys,
  62 provider checkboxes, preserved saved checks and working search.

The earlier production/release check above predates this UX follow-up.
`./scripts/release-macos.sh --check` was not rerun for these changes; no native
production build, installation, restart, screenshot or manual UI QA was done.
The updated HTML guide is the user's checklist for the new behavior.

Tracking disclosure for this follow-up: the designer reported one shell edit
to `web/visual-answer/src/compare.ts` (the stage inset constant and its comment)
in the first revision. It may be missing from Choro's Files/review attribution.
The subsequent corrections used dedicated editing tools; no files were removed.

### Natural-language comparisons and publication preflight

User QA showed that an ordinary request for four sidebar variations took an
image-generation path until the user named Visual answers. The resulting
publication then used an unsupported `Text` component and failed in the renderer.

The shared Codex/Claude instructions and tool description now make native
selectable comparisons the default for ordinary requests to see UI alternatives,
without a feature-name keyword. Explicit requests for images, exports or other
formats retain their requested output. Design skills may inform the alternatives;
they do not replace delivery through `visual_present`. The instructions include
a minimal valid `Answer`/`DesignOptions` program and require correction/retry of
tool errors before reporting that a visual answer was presented.

Publication now performs a bounded structural OpenUI Lang preflight in core
before persisting a revision or reserving its operation key. It rejects unknown
components, malformed roots/syntax, undefined/cyclic/orphaned references and
options without a reachable `DesignOptions` component, returning an actionable
error to the agent. Rejected revisions preserve the prior saved answer. The
frontend still uses the actual OpenUI parser for component props and dataset
references; preflight is not a replacement for that parser. A shared fixture
corpus is checked by both Rust and the OpenUI SDK, and the catalog is compared
against the renderer's generated component list.

These changes do not repair historical malformed publications automatically,
and no live-provider behavior or installed-app QA is implied by the tests.

Verification for this follow-up: combined default-feature tests passed for
`ide-app` (1,017), `ide-core` (568 plus 9 integration), and `ide-mcp` (65).
Frontend tests passed (47), as did type checking, bundling and generated
instructions. `git diff --check` passed. Existing ignored tests remain ignored.
The initial focused check exposed an old invalid fixture; it was corrected and
the complete suites passed. No native app build/install/restart, live Codex or
Claude session, UI-layout suite or release-readiness check was run this pass.

Manual retest after updating the running app: start a normal Codex chat, attach
the sidebar screenshot, and ask “The waiting support count is too subtle. Show
me four variations that make it stand out.” Expect four selectable designs
without mentioning Visual answers; repeat with Claude. Also ask for two hero
sections, and separately request an image file to check that explicit output
preferences still take precedence. These model-behavior checks remain pending.
