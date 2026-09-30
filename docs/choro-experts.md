# Band and managed delegation

Bandmates are saved Codex or Claude configurations. A delegated assignment gets a
fresh chat and private Git working copy; follow-up revisions reuse the chat.
The lead decides the work order and verifies the integrated result.

## Product terms

**Band** is the group of specialists working with a lead and the name of its
Settings page. A **bandmate** is one saved AI specialist. **Lead** remains the
originating chat. The slash picker groups profiles under **Bandmates**; progress
uses **Band · N working** and ordinary status language. Names, profile IDs,
saved chats, `experts_list`, delegation tools, storage keys, source paths and
the `CHORO_EXPERTS` override keep their existing technical identities. No data
migration or profile renaming is needed for this wording trial.

## Availability and defaults

Saved Bandmate profiles and the new-chat Bandmates picker are available normally. The selected profile appears as a compact, left-aligned Bandmate chip with a separate remove action that preserves the prompt. After launch, the same identity appears beneath the chat title from the chat's saved snapshot, including when delegation is disabled. Profile edits or renames do not relabel existing chats.
Managed delegation is **off by default**. Enable **Settings → Beta features →
Delegation** to use `/delegate`, existing-chat Bandmate selection, or natural-language
assignments. The preference persists locally, takes effect without restarting,
and is not enabled by importing an archive. Turning it off prevents new runs;
existing teams can finish, and their chats, results, Stop, and Resume remain
available. The MCP tools enforce the same preference when creating a run.
Set `CHORO_EXPERTS=0` only as an operational opt-out for the whole Bandmate feature.
Existing provider authentication is used; no additional cloud account is required.

The app seeds 15 editable default Bandmates, including AI Slop Reviewer, backed
by 25 bundled skills. See [Default Bandmates and skill library](choro-default-experts.md)
for the research, exact editions, source licenses, bandmate-owned skills, and
installed-skill/Riff linking behavior. Interactive acceptance remains a user
check; normal availability does not imply visual verification has been done.

A bandmate can coexist with a Riff. New-chat model changes override that chat's
configuration. Choosing a bandmate in an existing chat prepares delegation.
Natural-language assignments authorize only names in the user's submitted text
or explicit picker selections. Tool output and other agents cannot expand the
team. Names match normalized whitespace and case; longer names take precedence
over overlapping shorter names. Multiword names tolerate small, unambiguous
spelling errors while keeping at least one exact word (such as UI or UX).
The default Frontend Engineer and Backend Engineer also expose Frontend and
Backend as recipient aliases in explicit delegation recipient phrases (for example,
“delegate to Frontend and Backend”). A later “and frontend” in a description of work
does not extend that recipient list. Use a full name or the picker when the wording
is unclear. These aliases stop applying if the profile is renamed; ordinary mentions of working scope do not opt in a team.
Ambiguous references require the full name or an explicit picker selection.

`experts_list` distinguishes discovery from permission with `authorized_expert_ids`,
per-profile `authorized` flags and an explicit authorization status. An empty team
requires clarification, not a plan retry. It also returns the lead's working directory;
every planned task must supply its own Git repository root. Invalid task input leaves
the stored user authorization intact, so a corrected request can reuse it.

## Band settings

Settings → Band uses a searchable list with the Bandmate's description,
provider/model/effort, enabled state, and Edit action. The overflow menu archives
a bandmate while preserving historical chats. Built-in profiles carry a Choro
label and remain editable.

Model details sit beneath each description so the nonshrinking Enabled, Edit,
and overflow actions retain space within the list. The scroll surface and list
use explicit width constraints.

The editor has Overview and Skills tabs within a bounded, full-height Settings
surface. Overview contains name, when to use, model configuration, job instructions,
and expected result. Forms use the existing compact type scale, wrapping multiline
inputs with visible focus, and a fixed Save/Cancel footer. The header and all row
text have explicit width constraints; wide windows no longer stretch the editor
across the entire page.

Skills shows the attached guidance as a list. Add skill opens a focused picker for
bundled Choro skills, installed provider skills, or Riffs. Writing or editing a
custom skill replaces the list with a dedicated form. Apply skill stages it in the
bandmate draft; Save bandmate persists the whole setup. Cancelling the skill form
leaves its prior draft untouched; cancelling the bandmate discards staged changes.

View opens a bundled skill in a dedicated scrollable reader with its source,
license, and adaptation details. YAML metadata is omitted from the displayed
instructions; the original skill package and supporting references stay unchanged.
Long documents are virtualized and clipped to their own reading area, separate
from the fixed actions. Bundled skills are read-only; custom skills are editable.
Missing or incompatible links show repair guidance. Detaching a skill changes the
profile without removing its files.

Delegation limits appear under the enabled beta toggle, with bounded controls for concurrent
Bandmates per lead, concurrent Bandmates across Choro, and work revisions. Warm
amber accents identify Bandmates; validation errors retain their error styling.

## Conversation and sidebar experience

Delegation starts through `/delegate`, a bandmate in the slash picker, or a named
Bandmate in the user's request. There is no persistent Delegate button. A lead does
not automatically assemble a team for an ordinary request: the selected or named
Bandmates define the task's authorized team.

Before submission, a compact recipient chip opens a native searchable popover,
with **On-demand teammate** first and saved Bandmates below. Each option shows
its description, and a checkmark identifies the current recipient. The chip
supports keyboard opening; arrow keys navigate, Enter selects, and Escape closes
the picker. A separate **×** cancels delegation. Changing the recipient or
cancelling delegation preserves the draft and does not submit it.

Each originating request has a compact chat card with one row per assignment,
its real activity, dependencies and Open/Stop/Resume actions. The composer chip
opens the **Band side panel**, listing Active and Finished assignments. Selecting
a row opens that assignment's chat in the same resizable space; **All bandmates**
returns to the list. At narrow widths these surfaces occupy the main conversation
area, and Close returns to the lead. They share the auxiliary space with Preview.
The panel is scoped to the selected parent and never opens from background activity.

The task brief is a compact card above the Bandmate's chat. Full instructions and
the original request are available behind **Show full brief**. Rows lead with the
task, then show the Bandmate/model and a fixed two-line activity preview. Multiline
commands are flattened and truncated in previews; full text stays in the chat.
Repeated assignments to the same Bandmate remain distinct and open their own conversations.
The sidebar has expandable children under both normal and pinned parents for
bandmates still working or needing attention. Idle, paused and finished rows
stay in the Band panel; the sidebar dropdown disappears when no active rows
remain. A parent with delegated work stays in progress even when its provider is idle.
Working indicators use warm amber and respect macOS Reduce Motion.

Each delegated bandmate has a musical identity icon, assigned in the run's
original assignment order: drum, guitar, piano, microphone, speaker, then album.
The same icon appears in its sidebar row, assignment cards, overview, full-chat
title and completion summary. Status sorting, pauses, retries and reopening
history do not reassign it; larger bands repeat the six icons. These marks use
the bundled Lucide font and remain separate from activity and completion glyphs.

The lead's hover card shows a small amber Lucide Blend icon with the label
**Lead** once the chat has delegated assignments, including finished history.
The sidebar parent row keeps its task name and existing child dropdown without
an added role badge.

**View summary** opens the Bandmate's structured report: outcome, addressed
requirements, unfinished work and checks. The initial view bounds long text;
**Show full report** preserves the complete report. It updates as the coordinator
changes and marks corrected reports as earlier revisions. Summary dialogs open
only on an explicit click. A submitted report is not announced until its turn
settles. Bandmate finished, Integrating, Integrated, and Task complete remain
separate states; a settled Bandmate no longer increases the working count.

Stopped runs retain **Resume bandmates** and **End delegation · Keep files** beside
the composer. The lead conversation remains usable while Bandmates are stopped.
Neither opening a panel nor receiving a result resumes their work.

## Isolated demo build

`scripts/bundle-experts-demo.sh` creates a uniquely named **Choro Band Demo** under
`target/release/bundle`, with saved Bandmates and fresh sample projects under a
short `/tmp/choro-experts-*` data root. It never replaces an existing app or
resets an existing data directory. Reopening that app preserves its tasks and
working files. The demo uses a separate Chromium profile and Keychain service
names; its relay is disabled. Existing Codex and Claude authentication remains
available for provider acceptance.

Enable Delegation on the demo's Beta features page before trying managed tasks.
The demo's preference is separate from the production app's preference.

Set `CHORO_SKIP_DOC_EDITOR_BUILD=1` when an up-to-date editor distribution already
exists and the demo should reuse it. Local ad-hoc builds sign the CEF helpers
with the library-loading entitlement needed by the separately signed runtime.

## Ownership and persistence

`state/delegation.rs` owns scheduling independently of the mounted chat view.
`state/chat_dispatch.rs` shares hydration and dispatch with ordinary chats.
Only `AgentRecords` adopts and persists newly created child chats. MCP commands
use scoped caller identity, operation keys, expected revisions, and durable
deliveries. A provider becoming idle does not finish an assignment.

Store schema 35 adds Bandmate profiles, authorization records, delegation runs,
and optional agent bindings. A run is a transactional aggregate containing its
tasks, attempts, events, deliveries, operation receipts, and integration journal
references. Export format 9 includes these records and private-copy snapshots.
Imports recover inactive and never start a provider.

## Working files and recovery

Snapshots include dirty tracked files and non-ignored untracked files. They
preserve source HEAD, branch and index identity without staging or committing
in the source. Private copies have independent Git metadata.

Integration applies the change from the recorded baseline to the captured
result into the current parent files. Conflicts use a separate resolution
directory. Every apply rechecks source identity and affected-file hashes and
records a recoverable journal. Replacements and deletions both recheck the durable
Stop gate, repository identity and file preimage immediately before mutation. These
checks reduce races; they do not make multi-file application atomic. File deletions require an explicit confirmation
of the displayed operation. Working copies remain after Stop, completion and
failure; cleanup requires a preview and confirmation.

Managed provider startup only initializes or resumes the session. The durable
delivery queue sends the assignment, so startup cannot also execute the chat brief.
Ordinary fresh chats retain their initial-prompt behavior. Standalone bandmate
configuration is attached before initial persistence, including its frozen skills.

Parent Stop pauses the run and all managed runtimes. An individual Bandmate can
also be stopped. Resume is a user action and reconciles uncertain deliveries
against provider history. Missing sessions remain an explicit recovery error.
Queued corrections invalidate older results and do not clear a Stop gate.
“End delegation — keep files” releases a stopped parent for unrelated work.

## Verification

`scripts/verify.sh` checks formatting, every workspace target, core/app/MCP and
Claude bridge tests, Velotype tests, offline skill package/provenance checks,
and production app/MCP builds.

The Band list also has an opt-in GPUI layout fixture:

```sh
cargo test -p ide-app --features ui-layout-tests band_list_keeps_edit_actions_inside_viewport
```

It checks the scrolling list and action bounds at three content widths in a
test window with synthetic font metrics. It uses no application database and
does not replace visual verification in the demo.

Authenticated provider checks are opt-in and use a new disposable data root:

```sh
cargo build -p ide-mcp
CHORO_EXPERTS=1 \
CHORO_DATA_DIR="/tmp/choro-delegation-acceptance-$(uuidgen)" \
cargo test -p ide-app natural_language_two_expert_provider_acceptance \
  -- --ignored --nocapture
```

Set `CHORO_ACCEPTANCE_LEAD=claude` to exercise a Claude lead. Both lead directions
have completed the two-Bandmate to-do fixture, including automatic integration
and combined verification. `real_managed_providers_return_structured_results`
also exercises a second revision through each provider's existing session.

These checks do not control the live application. Interactive verification of
the side panel, focus, drafts, Preview, and Stop/Resume still requires permission
under this workspace's AGENTS.md.


## Lead-only Plan mode and review fixes (2026-09-14)

Plan mode belongs to the lead, including when a bandmate is opened as a full
chat. Consultation assignments run in ordinary provider conversation mode with
read-only scope. They deliver the requested answer, copy, analysis or plan as a
structured result to the lead, without a separate user-facing plan-approval flow.
A work-plan decision goes to the lead through a blocking coordination message.
Real permission requests and human questions still belong to the user.

Each child binding records its assignment kind, so the restriction also applies
to user corrections and resumed turns. Older bindings without this field default
to consultation restrictions until Choro repairs their scope from the current
assignment at a safe dispatch boundary. The original provider session and access
choice are preserved; Choro never grants Full access to solve a planning prompt.
Standalone bandmate chats are ordinary top-level chats and may still use Plan mode.

Claude removes EnterPlanMode and ExitPlanMode for managed children and enforces
consultation tool limits in PreToolUse as well as its permission callback, including
Full access sessions. Read-only consultation can use reading and Choro coordination
tools. A separate review-checklist pass remains stricter and cannot send coordination
mutations. Codex uses Default mode for children and a read-only sandbox without
escalation for consultations. Managed leads keep their own Plan-mode policy; they
can delegate consultations only and cannot integrate implementation while in Plan.

The feature review covered profile snapshots and skills, user authorization and
MCP scope, queue delivery, lifecycle and Stop/Resume, workspace capture and
integration, archive recovery, provider adapters, and the shared chat/sidebar UI.
Additional fixes:

- Background preparation updates assignment configuration while preserving live
  chat titles, session IDs, links, access choices and file attribution.
- An older terminal run can restore runtime policy only for its own lead binding;
  it cannot detach a newer active run on the same chat.
- A user correction cannot reopen a cancelled or superseded assignment.
- A blocked runtime marks dispatched work as uncertain, just like Stop. Resume
  must reconcile provider history instead of leaving the delivery stuck forever.
- Replaying an operation receipt does not advance the run revision, including
  concurrent retries, so it cannot invalidate another pending command.
- File integration rechecks Stop, repository identity and the destination after
  preparing a postimage, just before replacement. Newer user edits are retained;
  interrupted operations keep their journal and preimages for reconciliation.
  Confirmed deletions also sync the containing directory before journal completion.

Regression tests cover consultation delivery and legacy bindings, inherited access,
lead versus child modes, native Plan-tool denial, coordination in read-only scope,
strict review checklists, historical-run ownership, cancelled corrections, and
parent edits during file replacement. Runtime visuals and real provider behavior
in the final demo remain a separate human acceptance check.

Final verification for this revision: `scripts/verify.sh` passed on 2026-09-14,
including workspace formatting and compilation, 349 core tests, 748 app tests,
33 MCP tests, 17 Claude bridge tests, the Velotype suites, bundled catalog and
license verification, and optimized app/MCP builds. Existing opt-in tests stayed
ignored; this pass did not start authenticated model work or control the live UI.

## On-demand teammates

Delegation can use saved Bandmate presets or temporary teammates. An explicit
user request such as “Please delegate research and testing” can authorize a
temporary setup when no saved recipient resolves. `/delegate` is discoverable
in new and existing supported chats even when the beta is off; selecting it
then explains how to enable Settings → Beta features → Delegation.

In an existing chat, `/delegate` defaults to **On-demand teammate**; a saved
Bandmate can be selected instead. In a new chat it inserts an editable
“Delegate:” request. Temporary teammates inherit the lead's provider, model,
and effort by default; a fresh assignment can select a user-requested model as
described below. They receive the existing scoped handoff and
assignment brief, without creating saved profiles or automatically adding skills.
`experts_list` exposes their authorized IDs in `on_demand_expert_ids`. The lead
can reuse one ID for multiple distinct task keys, goals, and briefs; each task
gets its own chat and working copy through the normal coordinator. All existing
limits, dependencies, result checks, Stop, and restart gates apply.

Authorization comes only from literal user submissions or the explicit composer
selection. The conservative request detector recognizes direct delegation or
teammate-creation instructions; questions, quoted instructions, and negations
require clarification or `/delegate`. It does not decompose tasks. Runtime
instructions route requests to Choro and prohibit replacing requested delegation
with native helpers, including when the beta is disabled or a profile needs repair.
Temporary configurations remain in task history and are not added to Settings.

### Resume from the Band side panel

The right-hand Band overview and individual bandmate conversation expose the same
Stop/Resume task controls as the lead's timeline. When the whole run is stopped,
both surfaces show **Resume bandmates** and **End delegation · Keep files** instead
of individual task controls. Resuming a run does not bypass a separately paused
bandmate. Recovery errors appear in the open panel; existing session, workspace
and delivery checks still apply. No work resumes just by opening the panel.

### Different models for on-demand teammates

A lead may assign the same brief to several temporary teammates with different
models. In `delegation_plan`, each on-demand task may include `model_request`,
copying the model name exactly as the user typed it in the submission that
authorized that temporary teammate. For example: “Delegate four teammates using
GPT-6 Astra, GPT-6 Sol, Fable 5.1 and Sonnet 5; keep the outputs separate and compare
them.” The same temporary ID can be reused for all four assignments. Existing
concurrency limits still apply.

Choro resolves names against the same Codex/Claude catalog as the model picker.
It accepts labels, CLI names, picker shorthand and unambiguous spelling errors.
Version numbers are exact; an ambiguous family name or unsupported model returns
a clarification error instead of substituting another model. `experts_list`
includes `model_choices` to help the lead explain those options. Catalog support
does not guarantee provider account access; normal launch checks still apply.

Omitting the field preserves the inherited configuration. Overrides only affect
fresh on-demand assignments: saved profiles and the lead are unchanged. An already
started task keeps its model and session across revisions; changing it requires
a new assignment. Follow-up model authorization stays bound to its own temporary
teammate, and the configuration and original request persist across restart/export.

### Natural-language requests and follow-ups

An explicit request such as “Please delegate four on-demand teammates” prepares
Band delegation directly from the literal submitted message. The request may
appear later in a long message, use a supported common misspelling, or ask politely
with a question mark. Quotes elsewhere (such as a video title) and unrelated
constraints such as “without saved profiles” do not block it. Quoted examples,
code blocks, explanatory questions and negated delegation instructions do not
create temporary teammates.

Desktop and remote launches use the same durable submission path. A recognized
request prepares a run with its authorized saved or temporary configurations;
it does not create assignments or launch models. The lead still decides the
briefs, dependencies and execution order through the delegation tools. Once
prepared, the run keeps that authority through clarifications, even when the
next user message names no teammates. An unrelated message after the run ends
does not inherit it. Stop and the delegation beta preference remain gates.

The lead should act on a clear authorized request instead of asking the user to
type `/delegate` or create a saved profile. Actual ambiguities (for example four
teammates but three model choices) may still need a focused question. A missing
authorization for an already clear request is a configuration/recognition failure,
not a requirement to use a special phrase. Existing sessions receive a per-message
routing hint for recognized natural-language requests.

### Stopping, correcting and extending a Band

Stop preserves the run, chats and working files. An ordinary correction to the
lead keeps the Band stopped and retains its team authorization. A new explicit
user request to delegate, add teammates, or resume the Band goes through the same
session and integration-journal reconciliation as the Resume button. Provider
output cannot clear that gate, and restart alone never resumes work.

A correction sent directly to an individually stopped bandmate is saved first,
then resumes that bandmate when the whole Band is active. The composer explains
this before sending. If reconciliation fails, the message stays queued and the
UI reports the recovery error without inviting duplicate submission. Messages
to a stopped whole Band stay queued. Send-now interrupts only the addressed
bandmate; normal corrections wait for its safe turn boundary. Other assignments
keep their states and working copies.

Corrections can update not-yet-started briefs. Correcting an integrated result
within an active run schedules a new baseline in the same child conversation;
it does not apply the old contribution twice. Corrections wake a waiting lead,
and batched delivery explicitly includes the current task revision. A stopped
integration must be recovered before a correction can replace its journal.
Corrections arriving during working-copy preparation update the starting brief
without failing the run. An interrupted initial capture can be prepared again;
the task keeps its child identity and retains previous working files.

Adding teammates extends the active run without replacing existing assignments.
After End delegation, an explicit request such as “please delegate it again”
creates a fresh run; the ended run remains historical. On-demand model selection
can use literal user messages from the referenced task, including corrections.
Unrelated new requests and completed tasks do not inherit this model context.
Common misspellings such as “deleage” work in this path too. Empty ended cards no
longer claim the lead is still preparing assignments.
