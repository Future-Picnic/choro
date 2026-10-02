#![allow(
    dead_code,
    reason = "retained agent-chat state for planned interaction paths"
)]

mod changed_files;
mod code_review;
mod handoffs;
mod interactions;
mod pending_approval;
mod pending_user_input;
mod persistence;
mod proposed_plan;
pub(crate) mod protocol;
mod review_checklist;
mod search;
mod studio_request;
mod timeline;
mod usage;
mod verification;
mod work_log;

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{Context, EventEmitter};
use ide_core::local_store::{LocalStore, StoredTimelineEvent};
use ide_core::{AgentAccessMode, AgentEffort, AgentModel, AgentRecord, TaskRef};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) use changed_files::bounded_line_diff_counts;
pub(crate) use changed_files::VisualizationArtifactFilter;
pub use changed_files::{ChangedFilesSummary, FileChangeActivity, FileChangeStat};
pub use code_review::{split_code_review, CodeReview, CodeReviewFinding, CodeReviewSeverity};
pub use pending_approval::{PendingApproval, PendingApprovalKind};
pub use pending_user_input::{PendingUserInput, PendingUserInputOption, PendingUserInputQuestion};
pub use proposed_plan::{split_proposed_plan, ProposedPlan};
pub(crate) use protocol::ChatBackendStopSignal;
use protocol::{
    spawn_chat_backend_after_stop, ChatBackendCommand, ChatBackendController, ChatBackendEvent,
};
pub use review_checklist::{
    split_review_checklist, ReviewChecklist, ReviewChecklistItem, ReviewChecklistStatus,
    REVIEW_CHECKLIST_REQUEST_MARKER,
};
pub(crate) use search::{
    delegation_delivery_action_label, fold_search_text, search_turn_is_hidden,
    searchable_message_text, TIMELINE_SEARCH_TEXT_VERSION,
};
pub use usage::{ConversationUsage, ModelUsage, UsageTotals};
pub use studio_request::StudioChatRequest;
pub use verification::{split_verification, Verification, VerificationItem, VerificationStatus};
pub use work_log::{WorkLogEntry, WorkLogEntryKind, WorkLogStatus};

use interactions::*;
pub(crate) use persistence::persist_timeline_item;
use persistence::*;
pub use persistence::{
    load_persisted_file_ledger, persist_timeline_snapshot, timeline_item_from_store_event,
};
use timeline::*;
pub(crate) use timeline::place_change_receipts;

mod changes;
pub use changes::{ChatChange, ChatChangeCategories};

pub enum AgentChatEvent {
    SessionChanged(ChatChange),
    /// A provider-backed work turn settled normally. Unlike `TurnFinished`,
    /// this does not require a user message or changed-files receipt, so
    /// remotely launched agents can reliably react to completion too.
    WorkFinished {
        agent_id: Uuid,
    },
    TurnFinished {
        agent_id: Uuid,
        source_turn_id: String,
    },
}

impl EventEmitter<AgentChatEvent> for AgentChatState {}

fn apply_usage_snapshot(session: &mut AgentChatSession, usage: ConversationUsage) {
    let is_current_or_newer = session.usage.as_ref().is_none_or(|current| {
        current.session_id != usage.session_id
            || usage.totals.total_tokens() >= current.totals.total_tokens()
    });
    if is_current_or_newer {
        session.usage = Some(usage);
    }
}

#[derive(Default)]
pub struct AgentChatState {
    change_tracker: changes::ChatChangeTracker,
    current_event_received_at: Option<std::time::Instant>,
    pub(crate) sessions: HashMap<Uuid, AgentChatSession>,
    controllers: HashMap<Uuid, ChatBackendController>,
    stopping_backends: HashMap<Uuid, ChatBackendStopSignal>,
    backend_generations: HashMap<Uuid, u64>,
    finished_file_turns: HashSet<(Uuid, String)>,
    cancellation_requested: HashSet<Uuid>,
    /// Stop pauses automatic dispatch until the user sends another message.
    paused_queues: HashSet<Uuid>,
    handoffs_sending: HashSet<Uuid>,
    /// Holds ordinary sends while a coordinator snapshots or applies files.
    pub(crate) delegation_reservations: HashSet<Uuid>,
}

#[derive(Clone, Debug)]
pub struct AgentChatSession {
    pub agent_id: Uuid,
    pub title: String,
    pub chat_session_id: Option<String>,
    pub cli_session_id: Option<String>,
    pub hidden_from_notifications: bool,
    pub status: AgentChatStatus,
    /// Live provider activity, never restored from conversation history.
    pub is_compacting: bool,
    pub interaction_mode: AgentInteractionMode,
    pub composer_text: String,
    pub messages: Vec<AgentChatMessage>,
    pub timeline: Vec<AgentChatTimelineItem>,
    pub queued_turns: Vec<QueuedChatTurn>,
    pub work_log: Vec<WorkLogEntry>,
    pub pending_user_input: Option<PendingUserInput>,
    pub pending_approval: Option<PendingApproval>,
    pub proposed_plan: Option<ProposedPlan>,
    pub changed_files: ChangedFilesSummary,
    pub usage: Option<ConversationUsage>,
    pub started_running_at: Option<u64>,
    pub last_activity_at: u64,
}

impl AgentChatSession {
    pub(crate) fn set_status(&mut self, status: AgentChatStatus) {
        self.status = status;
        self.is_compacting = false;
    }

    fn set_compacting(&mut self, active: bool) {
        self.is_compacting = active && self.status == AgentChatStatus::Running;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedChatTurn {
    pub id: Uuid,
    pub text: String,
    pub display_text: Option<String>,
    pub tags: Vec<AgentChatMessageTag>,
    pub mode: AgentInteractionMode,
    pub created_at: u64,
    pub handoff: Option<QueuedAgentHandoff>,
    pub studio_request: Option<StudioChatRequest>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedAgentHandoff {
    pub target_agent_id: Uuid,
    pub target_title: String,
    pub kind: String,
    pub original_text: String,
    pub references: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipResult {
    pub id: String,
    pub action: String,
    /// The repository label for multi-repository ships. Kept empty for
    /// single-repository ships so their cards stay compact.
    pub repository: Option<String>,
    pub branch: String,
    /// The PR's destination branch. Present only for Ship actions that opened
    /// a pull request; optional for backward compatibility with older cards.
    pub pr_base_branch: Option<String>,
    pub commit_sha: String,
    pub pr_url: Option<String>,
    pub pr_title: Option<String>,
    pub pr_body: Option<String>,
    pub created_at: u64,
    /// The task this ship relates to, captured at ship time (source task, else
    /// the first linked task). Drives the post-ship "update the task" actions.
    pub task: Option<TaskRef>,
    /// Status (by display name) suggested when the PR is done, from the task's
    /// connection settings. Only a suggestion — never applied automatically.
    pub suggested_status: Option<String>,
    /// Records what the user chose to apply, once they've applied it. `None`
    /// means the actions are still pending (or there was no linked task).
    pub applied: Option<ShipTaskApplied>,
}

/// The outcome of applying the post-ship task actions, persisted so the card
/// renders a static confirmation after the fact (and never re-prompts).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipTaskApplied {
    pub commented: bool,
    pub status_name: Option<String>,
    pub at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentChatStatus {
    Idle,
    Running,
    Cancelling,
    WaitingForUser,
    PlanReady,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgentInteractionMode {
    #[default]
    Default,
    Plan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentChatMessage {
    User {
        text: String,
        display_text: Option<String>,
        tags: Vec<AgentChatMessageTag>,
        created_at: u64,
    },
    Assistant {
        message_id: Option<String>,
        text: String,
        created_at: u64,
    },
    Thought {
        message_id: Option<String>,
        text: String,
        created_at: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentChatMessageTagKind {
    Preview,
    Orbit,
    Riff,
    Skill,
    Command,
    File,
    Folder,
    Doc,
    Design,
    Project,
    Brain,
    /// Keeps older or third-party tag values readable after capabilities are removed.
    #[serde(other)]
    LegacyVisual,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentChatMessageTag {
    pub kind: AgentChatMessageTagKind,
    pub label: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug)]
pub enum AgentChatTimelineItem {
    DelegationGroup { run_id: Uuid, created_at: u64 },
    Message(AgentChatMessage),
    WorkLog(WorkLogEntry),
    FileChangeActivity(FileChangeActivity),
    PendingUserInput(PendingUserInput),
    ProposedPlan(ProposedPlan),
    CodeReview(CodeReview),
    Verification(Verification),
    ReviewChecklist(ReviewChecklist),
    ChangedFiles(ChangedFilesSummary),
    ShipResult(ShipResult),
    Rejoined(RejoinedCard),
    RejoinConflict(RejoinConflictCard),
    Memorized(MemorizedCard),
    MemoryProposal(MemoryProposalCard),
    OrbitUpdate(OrbitUpdateCard),
    AgentSummary(AgentSummaryCard),
    AgentMessage(AgentMessageCard),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentSummaryCard {
    pub summary_text: String,
    pub last_summarized_sequence: i64,
    pub updated_at: u64,
    pub edited_by_user: bool,
    pub expanded: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentMessageCard {
    pub id: Uuid,
    pub source_agent_id: Uuid,
    pub source_title: String,
    /// Present for the sender's audit card; absent for the recipient's card.
    pub target_agent_id: Option<Uuid>,
    pub target_title: Option<String>,
    pub text: String,
    pub kind: String,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrbitUpdateCard {
    pub invocation_id: Uuid,
    pub module_id: Uuid,
    pub module_name: String,
    pub inserted: usize,
    pub updated: usize,
    pub deleted: usize,
    pub undone: bool,
    pub created_at: u64,
}

impl From<ide_core::local_store::OrbitInvocationUpdate> for OrbitUpdateCard {
    fn from(update: ide_core::local_store::OrbitInvocationUpdate) -> Self {
        Self {
            invocation_id: update.invocation_id,
            module_id: update.module_id,
            module_name: update.module_name,
            inserted: update.inserted,
            updated: update.updated,
            deleted: update.deleted,
            undone: update.undone,
            created_at: update.created_at,
        }
    }
}

pub(crate) fn upsert_orbit_update_card(
    timeline: &mut Vec<AgentChatTimelineItem>,
    card: OrbitUpdateCard,
) {
    timeline::upsert_timeline_orbit_update(timeline, card);
}

/// A memory the agent just saved via `memory_save` — surfaced in the chat so
/// nothing enters memory invisibly, with one-tap undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemorizedCard {
    pub memory_id: Uuid,
    pub text: String,
    /// `true` = a global (user-wide) memory; `false` = this project's.
    pub global: bool,
    pub created_at: u64,
}

/// A candidate memory distilled from a decision the user just made (plan
/// feedback, a typed answer). Nothing is saved until they accept the card;
/// dismissed proposals stay in the timeline invisibly so the same rule is
/// never proposed twice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryProposalCard {
    /// "mp-{uuid}" — identity for persistence and UI gating.
    pub id: String,
    /// The candidate rule text (≤ memory limit, enforced at parse time).
    pub text: String,
    /// One-line distiller rationale; empty hides the row.
    pub why: String,
    /// Which scope the distiller suggests; the user can pick either.
    pub suggested_global: bool,
    /// "plan_feedback" | "question_answer" — where the signal came from.
    pub source: String,
    pub status: MemoryProposalStatus,
    pub created_at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemoryProposalStatus {
    Pending,
    /// The scope actually chosen may differ from the suggestion.
    Accepted {
        memory_id: Uuid,
        global: bool,
    },
    Dismissed,
}

/// A Solo's Rejoin landing in the chat: its branch merged into the base, the
/// lane packed up. The durable record of where the side work went home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejoinedCard {
    pub id: String,
    pub branch: String,
    pub base: String,
    pub created_at: u64,
}

/// A Rejoin that hit merge conflicts. The target branch was left untouched
/// (the merge aborted); this card names the overlapping files and offers to
/// hand resolution to the Solo agent itself, inside its own lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RejoinConflictCard {
    pub id: String,
    pub branch: String,
    pub target: String,
    /// Conflicted paths parsed from git's merge output; empty when the
    /// failure wasn't a per-file conflict — `detail` carries git's words then.
    pub files: Vec<String>,
    pub detail: String,
    pub created_at: u64,
    /// Set when "Have the agent sort it" was tapped, so the card shows the
    /// hand-off happened instead of offering the button again.
    pub requested_at: Option<u64>,
    /// Set when the card was waved away. Persistence only ever upserts
    /// timeline events, so dismissal is a flag, not a deletion.
    pub dismissed_at: Option<u64>,
    /// Set when the post-hand-off check found the target merged into the
    /// lane — the card flips to "Ready to rejoin" with a one-tap Rejoin.
    pub resolved_at: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StoredTimelinePayload {
    DelegationGroup {
        run_id: Uuid,
        created_at: u64,
    },
    Message {
        role: String,
        text: String,
        #[serde(default)]
        display_text: Option<String>,
        #[serde(default)]
        tags: Vec<AgentChatMessageTag>,
        #[serde(default)]
        search_text_version: u64,
        #[serde(default)]
        search_text: String,
        created_at: u64,
        backend_message_id: Option<String>,
    },
    WorkLog {
        id: String,
        collapse_key: String,
        kind: String,
        title: String,
        detail: Option<String>,
        status: String,
        started_at: u64,
        updated_at: u64,
        count: usize,
    },
    FileChangeActivity {
        id: String,
        turn_id: String,
        file: StoredFileChange,
        observed: bool,
        updated_at: u64,
    },
    PendingUserInput {
        request_id: String,
        questions: Vec<StoredPendingQuestion>,
        answers: Vec<Option<StoredPendingAnswer>>,
        question_index: usize,
    },
    ProposedPlan {
        id: String,
        markdown: String,
        expanded: bool,
        implemented_at: Option<u64>,
    },
    CodeReview {
        id: String,
        markdown: String,
        expanded: bool,
    },
    Verification {
        id: String,
        markdown: String,
        expanded: bool,
    },
    ReviewChecklist {
        id: String,
        source_turn_id: String,
        status: String,
        items: Vec<StoredReviewChecklistItem>,
        expanded: bool,
        created_at: u64,
    },
    ChangedFiles {
        files: Vec<StoredFileChange>,
        #[serde(default)]
        observed_files: Vec<StoredFileChange>,
        #[serde(default)]
        turn_id: Option<String>,
        #[serde(default)]
        attribution_version: u8,
        #[serde(default)]
        ledger_revision: u64,
        #[serde(default)]
        snapshot_id: Option<Uuid>,
        #[serde(default)]
        commit_sha: Option<String>,
    },
    ShipResult {
        id: String,
        action: String,
        #[serde(default)]
        repository: Option<String>,
        branch: String,
        #[serde(default)]
        pr_base_branch: Option<String>,
        commit_sha: String,
        pr_url: Option<String>,
        #[serde(default)]
        pr_title: Option<String>,
        #[serde(default)]
        pr_body: Option<String>,
        created_at: u64,
        #[serde(default)]
        task: Option<TaskRef>,
        #[serde(default)]
        suggested_status: Option<String>,
        #[serde(default)]
        applied: Option<StoredShipTaskApplied>,
    },
    Rejoined {
        id: String,
        branch: String,
        base: String,
        created_at: u64,
    },
    RejoinConflict {
        id: String,
        branch: String,
        target: String,
        #[serde(default)]
        files: Vec<String>,
        #[serde(default)]
        detail: String,
        created_at: u64,
        #[serde(default)]
        requested_at: Option<u64>,
        #[serde(default)]
        dismissed_at: Option<u64>,
        #[serde(default)]
        resolved_at: Option<u64>,
    },
    Memorized {
        memory_id: Uuid,
        text: String,
        global: bool,
        created_at: u64,
    },
    MemoryProposal {
        id: String,
        text: String,
        #[serde(default)]
        why: String,
        suggested_global: bool,
        status: String,
        #[serde(default)]
        memory_id: Option<Uuid>,
        #[serde(default)]
        accepted_global: Option<bool>,
        #[serde(default)]
        source: String,
        created_at: u64,
    },
    OrbitUpdate {
        invocation_id: Uuid,
        module_id: Uuid,
        module_name: String,
        inserted: usize,
        updated: usize,
        deleted: usize,
        undone: bool,
        created_at: u64,
    },
    AgentSummary {
        summary_text: String,
        last_summarized_sequence: i64,
        updated_at: u64,
        edited_by_user: bool,
        expanded: bool,
    },
    AgentMessage {
        id: Uuid,
        source_agent_id: Uuid,
        source_title: String,
        #[serde(default)]
        target_agent_id: Option<Uuid>,
        #[serde(default)]
        target_title: Option<String>,
        text: String,
        kind: String,
        created_at: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredShipTaskApplied {
    commented: bool,
    #[serde(default)]
    status_name: Option<String>,
    at: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPendingQuestion {
    id: String,
    header: String,
    question: String,
    options: Vec<StoredPendingOption>,
    multi_select: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPendingOption {
    label: String,
    description: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredPendingAnswer {
    selected_option_labels: Vec<String>,
    custom_answer: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct StoredFileChange {
    #[serde(default)]
    prior_segments: Vec<FileChangeStat>,
    path: String,
    #[serde(default)]
    counts_unavailable: bool,
    additions: usize,
    deletions: usize,
    #[serde(default)]
    counts_are_projection: bool,
    #[serde(default)]
    clears_projection: bool,
    #[serde(default)]
    baseline_hash: Option<String>,
    #[serde(default)]
    result_hash: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredReviewChecklistItem {
    id: String,
    #[serde(default)]
    flow: Option<String>,
    action: String,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    checked: bool,
}

impl AgentChatState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn session(&self, agent_id: Uuid) -> Option<&AgentChatSession> {
        self.sessions.get(&agent_id)
    }

    pub fn session_mut(&mut self, agent_id: Uuid) -> Option<&mut AgentChatSession> {
        self.sessions.get_mut(&agent_id)
    }

    pub fn has_backend(&self, agent_id: Uuid) -> bool {
        self.controllers.contains_key(&agent_id)
    }

    pub(crate) fn record_delegation_integration(
        &mut self,
        id: Uuid,
        summary: ChangedFilesSummary,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&id) {
            if session.timeline.iter().any(|item|matches!(item,AgentChatTimelineItem::ChangedFiles(old)if old.turn_id==summary.turn_id)){return;}
            if let Some((receipt, ledger)) = apply_changed_files_summary(session, summary) {
                persist_changed_files_turn(id, receipt, ledger, cx);
            }
            self.publish_change(id, ChatChangeCategories::CONTENT, cx);
        }
    }

    pub(crate) fn background_reserved(&self, agent_id: Uuid) -> bool {
        self.delegation_reservations.contains(&agent_id)
    }

    pub(crate) fn safe_for_delegation(&self, agent_id: Uuid) -> bool {
        !self.background_reserved(agent_id)
            && !self.paused_queues.contains(&agent_id)
            && !self.handoffs_sending.contains(&agent_id)
            && self.session(agent_id).is_none_or(|s| {
                s.status == AgentChatStatus::Idle
                    && s.pending_approval.is_none()
                    && s.pending_user_input.is_none()
                    && s.queued_turns.is_empty()
                    && !s.is_compacting
            })
    }

    pub(crate) fn release_delegation_reservation(&mut self, id: Uuid, cx: &mut Context<Self>) {
        self.delegation_reservations.remove(&id);
        if !self.paused_queues.contains(&id) {
            self.schedule_next_queued_turn(id, cx);
        }
    }

    pub(crate) fn allow_managed_resume(&mut self, id: Uuid) {
        self.paused_queues.remove(&id);
    }

    pub(crate) fn backend_generation(&self, id: Uuid) -> u64 {
        self.backend_generations.get(&id).copied().unwrap_or(0)
    }

    pub fn dock_badge_label(&self) -> Option<String> {
        let unread_attention = self
            .sessions
            .values()
            .filter(|session| {
                !session.hidden_from_notifications
                    && matches!(
                        session.status,
                        AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady
                    )
            })
            .count();
        (unread_attention > 0).then(|| unread_attention.to_string())
    }

    pub fn ensure_session(
        &mut self,
        agent_id: Uuid,
        title: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> &mut AgentChatSession {
        let now = unix_now();
        let inserted = !self.sessions.contains_key(&agent_id);
        self
            .sessions
            .entry(agent_id)
            .or_insert_with(|| AgentChatSession {
                agent_id,
                title: title.into(),
                chat_session_id: None,
                cli_session_id: None,
                hidden_from_notifications: false,
                is_compacting: false,
                status: AgentChatStatus::Idle,
                interaction_mode: AgentInteractionMode::Default,
                composer_text: String::new(),
                messages: Vec::new(),
                timeline: Vec::new(),
                queued_turns: Vec::new(),
                work_log: Vec::new(),
                pending_user_input: None,
                pending_approval: None,
                proposed_plan: None,
                changed_files: ChangedFilesSummary::default(),
                usage: None,
                started_running_at: None,
                last_activity_at: now,
            });
        if inserted { self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx); }
        self.sessions.get_mut(&agent_id).unwrap()
    }

    pub fn set_interaction_mode(
        &mut self,
        agent_id: Uuid,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.interaction_mode = mode;
            self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        }
    }

    pub fn start_backend(
        &mut self,
        agent: AgentRecord,
        initial_mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        if self.controllers.contains_key(&agent.id) {
            return Ok(());
        }

        let generation = self.next_backend_generation(agent.id);
        let initial_turn_id = (agent.cli_session_id.is_none() && agent.delegation.is_none() && !agent.hidden_doc_assistant)
            .then(|| self.anchor_change_turn(agent.id, cx));
        let (controller, event_rx) = spawn_chat_backend_after_stop(
            agent.clone(),
            initial_mode,
            self.stopping_backends.get(&agent.id).cloned(),
            initial_turn_id,
        )?;
        self.stopping_backends.remove(&agent.id);
        self.controllers.insert(agent.id, controller);
        let agent_id = agent.id;
        // Await the channel instead of polling on a timer: an idle chat costs
        // zero wake-ups, and the task ends when the backend's senders drop.
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(event) = event_rx.recv().await else {
                    break;
                };
                let Some(this) = this.upgrade() else {
                    break;
                };
                if this
                    .update(cx, |state, cx| {
                        state.apply_backend_event(agent_id, generation, event, cx)
                    })
                    .is_err()
                {
                    break;
                }
                // Drain what queued up behind the first event so a streaming
                // burst is applied in one foreground pass — but bounded, with a
                // yield after the cap, so a flooding backend cannot monopolize
                // the main thread.
                let mut drained = 0usize;
                while let Ok(event) = event_rx.try_recv() {
                    if this
                        .update(cx, |state, cx| {
                            state.apply_backend_event(agent_id, generation, event, cx)
                        })
                        .is_err()
                    {
                        return;
                    }
                    drained += 1;
                    if drained >= 128 {
                        cx.background_executor()
                            .timer(Duration::from_millis(1))
                            .await;
                        drained = 0;
                    }
                }
            }
        })
        .detach();
        Ok(())
    }

    fn anchor_change_turn(&mut self, agent_id: Uuid, cx: &mut Context<Self>) -> String {
        let source = self
            .sessions
            .get(&agent_id)
            .and_then(|session| {
                session.timeline.iter().rev().find(|item| {
                    matches!(
                        item,
                        AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
                    )
                })
            })
            .and_then(persistence::stored_timeline_event_parts)
            .and_then(|(_, key, _, _)| key);
        let turn_id = match source {
            Some(key) => format!("{}|{key}", Uuid::new_v4()),
            None => Uuid::new_v4().to_string(),
        };
        let entry = WorkLogEntry::new(
            format!("file-receipt:{turn_id}"),
            format!("file-receipt:{turn_id}"),
            WorkLogEntryKind::System,
            "Updating changes",
            WorkLogStatus::InProgress,
        );
        let session = self.ensure_backend_event_session(agent_id, unix_now());
        upsert_timeline_work_log(&mut session.timeline, entry.clone());
        persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
        turn_id
    }

    pub fn send_turn(
        &mut self,
        agent_id: Uuid,
        text: String,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) {
        self.send_turn_with_studio_request(agent_id, text, mode, None, String::new(), cx);
    }

    pub fn send_turn_with_studio_request(
        &mut self,
        agent_id: Uuid,
        text: String,
        mode: AgentInteractionMode,
        studio_request: Option<StudioChatRequest>,
        title: String,
        cx: &mut Context<Self>,
    ) {
        let turn_id = self.anchor_change_turn(agent_id, cx);
        if let Some(controller) = self.controllers.get(&agent_id) {
            let command = match studio_request {
                Some(request) => ChatBackendCommand::SendStudioTurn { text, mode, turn_id, title, request },
                None => ChatBackendCommand::SendTurn { turn_id, text, mode, read_only: false },
            };
            let _ = controller.send(command);
        }
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            self.cancellation_requested.remove(&agent_id);
            // A new turn invalidates the previous turn's task plan: drop it so the
            // "Current tasks" footer starts empty and only repopulates if the
            // agent emits a fresh plan this turn — instead of showing stale tasks
            // from earlier, unrelated work.
            session
                .work_log
                .retain(|entry| entry.kind != WorkLogEntryKind::Plan);
            session.set_status(AgentChatStatus::Running);
            session.pending_approval = None;
            session.started_running_at = Some(unix_now());
            session.last_activity_at = unix_now();
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn send_read_only_turn(
        &mut self,
        agent_id: Uuid,
        text: String,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) {
        let turn_id = self.anchor_change_turn(agent_id, cx);
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::SendTurn {
                turn_id,
                text,
                mode,
                read_only: true,
            });
        }
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            self.cancellation_requested.remove(&agent_id);
            session.set_status(AgentChatStatus::Running);
            session.pending_approval = None;
            session.started_running_at = Some(unix_now());
            session.last_activity_at = unix_now();
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn update_access_mode(
        &mut self,
        agent_id: Uuid,
        access_mode: AgentAccessMode,
        cx: &mut Context<Self>,
    ) {
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::UpdateAccessMode { access_mode });
        }
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            let entry = WorkLogEntry::new(
                next_local_id(),
                "agent-chat-access-mode",
                WorkLogEntryKind::System,
                format!("Access mode changed to {}", access_mode.short_label()),
                WorkLogStatus::Completed,
            );
            upsert_work_log_entry(&mut session.work_log, entry.clone());
            upsert_timeline_work_log(&mut session.timeline, entry.clone());
            persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            session.last_activity_at = unix_now();
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn update_model_effort(
        &mut self,
        agent_id: Uuid,
        model: AgentModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::UpdateModelEffort { model, effort, external_model: None });
        }
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            let entry = WorkLogEntry::new(
                next_local_id(),
                "agent-chat-model-effort",
                WorkLogEntryKind::System,
                format!("Model changed to {} · {}", model.label(), effort.label()),
                WorkLogStatus::Completed,
            );
            upsert_work_log_entry(&mut session.work_log, entry.clone());
            upsert_timeline_work_log(&mut session.timeline, entry.clone());
            persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            session.last_activity_at = unix_now();
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn update_external_model(&mut self, agent_id: Uuid, id: String, variants: Vec<String>, effort: AgentEffort, cx: &mut Context<Self>) {
        if let Some(controller)=self.controllers.get(&agent_id) {let _=controller.send(ChatBackendCommand::UpdateModelEffort {model:AgentModel::OpenCode, effort, external_model:Some((id,variants))});}
        self.publish_change(agent_id, ChatChangeCategories::CONTROLS, cx);
    }

    pub fn update_title(&mut self, agent_id: Uuid, title: String, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        if session.title == title {
            return;
        }
        session.title = title;
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn queue_turn(
        &mut self,
        agent_id: Uuid,
        text: String,
        display_text: Option<String>,
        tags: Vec<AgentChatMessageTag>,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) -> Uuid {
        self.queue_turn_with_studio(agent_id, text, display_text, tags, mode, None, cx)
    }

    pub fn queue_turn_with_studio(
        &mut self,
        agent_id: Uuid,
        text: String,
        display_text: Option<String>,
        tags: Vec<AgentChatMessageTag>,
        mode: AgentInteractionMode,
        studio_request: Option<StudioChatRequest>,
        cx: &mut Context<Self>,
    ) -> Uuid {
        let id = Uuid::new_v4();
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.queued_turns.push(QueuedChatTurn {
                id,
                text,
                display_text,
                tags,
                mode,
                created_at: unix_now(),
                handoff: None,
                studio_request,
            });
            if session.status == AgentChatStatus::Idle {
                self.schedule_next_queued_turn(agent_id, cx);
            }
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        id
    }

    pub fn remove_queued_turn(&mut self, agent_id: Uuid, turn_id: Uuid, cx: &mut Context<Self>) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.queued_turns.retain(|turn| turn.id != turn_id);
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn resume_queue(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        if self.paused_queues.remove(&agent_id) {
            self.schedule_next_queued_turn(agent_id, cx);
            self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        }
    }

    fn can_drain_queue(&self, agent_id: Uuid) -> bool {
        !self.background_reserved(agent_id)
            && !self.paused_queues.contains(&agent_id)
            && !self.handoffs_sending.contains(&agent_id)
            && self.sessions.get(&agent_id).is_some_and(|session| {
                session.status == AgentChatStatus::Idle
                    && session
                        .queued_turns
                        .first()
                        .is_some_and(|turn| turn.handoff.is_some() || self.has_backend(agent_id))
            })
    }

    pub fn steer_queued_turn(&mut self, agent_id: Uuid, turn_id: Uuid, cx: &mut Context<Self>) {
        if self.handoffs_sending.contains(&agent_id) {
            return;
        }
        // A force-stop removes the backend. Keep the message until a new
        // composer submission can safely restart that backend.
        if !self.has_backend(agent_id)
            && self.session(agent_id).is_some_and(|session| {
                session
                    .queued_turns
                    .iter()
                    .any(|turn| turn.id == turn_id && turn.handoff.is_none())
            })
        {
            return;
        }
        if !self.validate_queued_studio_request(agent_id, turn_id, cx) {
            return;
        }
        let Some(turn) = self.take_queued_turn(agent_id, turn_id) else {
            return;
        };
        if turn.handoff.is_some() {
            self.send_queued_handoff(agent_id, turn, cx);
            return;
        }
        self.start_turn(
            agent_id,
            turn.text,
            turn.display_text,
            turn.tags,
            turn.mode,
            turn.studio_request,
            cx,
        );
    }

    fn drain_next_queued_turn(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        if !self.can_drain_queue(agent_id) {
            return;
        }
        let Some(id) = self.sessions.get(&agent_id).and_then(|s| s.queued_turns.first()).map(|t| t.id) else {
            return;
        };
        if !self.validate_queued_studio_request(agent_id, id, cx) {
            return;
        }
        let Some(turn) = self.take_next_queued_turn(agent_id) else {
            return;
        };
        if turn.handoff.is_some() {
            self.send_queued_handoff(agent_id, turn, cx);
            return;
        }
        self.start_turn(
            agent_id,
            turn.text,
            turn.display_text,
            turn.tags,
            turn.mode,
            turn.studio_request,
            cx,
        );
    }

    /// Give observers one foreground tick to react to the completed turn
    /// before queued work changes Idle straight back to Running. The verifier
    /// uses that completion window to insert its check ahead of later user
    /// turns; the guard keeps this scheduled drain from competing with it.
    fn schedule_next_queued_turn(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1))
                .await;
            this.update(cx, |state, cx| {
                // Recheck pause state when the callback runs: Stop may have
                // happened after this dispatch was scheduled.
                state.drain_next_queued_turn(agent_id, cx);
            })
            .ok();
        })
        .detach();
    }

    fn validate_queued_studio_request(&mut self, agent_id: Uuid, turn_id: Uuid, cx: &mut Context<Self>) -> bool {
        let Some(turn) = self.sessions.get(&agent_id).and_then(|s| s.queued_turns.iter().find(|t| t.id == turn_id)) else {
            return false;
        };
        let Some(request) = &turn.studio_request else { return true; };
        // Studio cannot steer a new scope into a running or cancelling turn.
        if self.sessions.get(&agent_id).is_none_or(|s| s.status != AgentChatStatus::Idle) {
            return false;
        }
        if let Err(error) = request.validate() {
            self.paused_queues.insert(agent_id);
            let entry = WorkLogEntry::new(next_local_id(), "studio-queue-error", WorkLogEntryKind::System,
                format!("Could not start queued design request: {error:#}"), WorkLogStatus::Failed);
            self.upsert_work_log(agent_id, entry, cx);
            return false;
        }
        true
    }

    fn take_next_queued_turn(&mut self, agent_id: Uuid) -> Option<QueuedChatTurn> {
        let session = self.sessions.get_mut(&agent_id)?;
        (!session.queued_turns.is_empty()).then(|| session.queued_turns.remove(0))
    }

    fn take_queued_turn(&mut self, agent_id: Uuid, turn_id: Uuid) -> Option<QueuedChatTurn> {
        let session = self.sessions.get_mut(&agent_id)?;
        let index = session
            .queued_turns
            .iter()
            .position(|turn| turn.id == turn_id)?;
        Some(session.queued_turns.remove(index))
    }

    fn start_turn(
        &mut self,
        agent_id: Uuid,
        text: String,
        display_text: Option<String>,
        tags: Vec<AgentChatMessageTag>,
        mode: AgentInteractionMode,
        studio_request: Option<StudioChatRequest>,
        cx: &mut Context<Self>,
    ) {
        let title = display_text.as_deref().unwrap_or(&text).to_owned();
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            let message = AgentChatMessage::User {
                text: text.clone(),
                display_text,
                tags,
                created_at: unix_now(),
            };
            append_or_extend_message(&mut session.messages, message.clone());
            append_or_extend_timeline_message(&mut session.timeline, message.clone());
            persist_chat_message(agent_id, message, cx);
        }
        self.send_turn_with_studio_request(agent_id, text, mode, studio_request, title, cx);
    }

    pub fn stop_backend(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.paused_queues.insert(agent_id);
        if !self.controllers.contains_key(&agent_id) {
            let _ = self.hard_stop_backend(agent_id, true, cx);
            return;
        }
        let force_stop = self.cancellation_requested.contains(&agent_id)
            || self
                .sessions
                .get(&agent_id)
                .is_some_and(|session| session.status == AgentChatStatus::Cancelling);
        if force_stop {
            let _ = self.hard_stop_backend(agent_id, true, cx);
            return;
        }
        if let Some(controller) = self.controllers.get(&agent_id) {
            controller.cancel_turn();
        }
        self.cancellation_requested.insert(agent_id);
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.set_status(AgentChatStatus::Cancelling);
            session.started_running_at = None;
            session.pending_user_input = None;
            session.pending_approval = None;
            session
                .timeline
                .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
            let entry = WorkLogEntry::new(
                next_local_id(),
                "agent-chat-cancelling",
                WorkLogEntryKind::System,
                "Stopping current turn",
                WorkLogStatus::InProgress,
            )
            .detail(Some(
                "Queued messages are kept and paused until you send another message. Click stop again to force-kill the backend process.".to_string(),
            ));
            upsert_work_log_entry(&mut session.work_log, entry.clone());
            upsert_timeline_work_log(&mut session.timeline, entry.clone());
            persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            session.last_activity_at = unix_now();
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    /// Quietly shut down the backend processes of a chat that has been idle
    /// long enough, without touching the visible conversation. The session,
    /// its timeline, and its resume ids stay in place, so the next submission
    /// restarts the backend and resumes the provider session transparently.
    ///
    /// Refuses to retire anything that could lose state: a running or
    /// cancelling turn, queued turns, a pending question or approval, or a
    /// chat whose provider resume id was never captured (restarting those is
    /// blocked by the resume-safety guard, so killing the backend would strand
    /// the chat). A proposed plan awaiting the user is covered by the status
    /// check: it forces `PlanReady`, which is not `Idle`.
    pub fn retire_idle_backend(&mut self, agent_id: Uuid, cx: &mut Context<Self>) -> bool {
        if !self.controllers.contains_key(&agent_id) {
            return false;
        }
        if self.cancellation_requested.contains(&agent_id) {
            return false;
        }
        let Some(session) = self.sessions.get(&agent_id) else {
            return false;
        };
        if !session_safe_to_retire(session) || self.handoffs_sending.contains(&agent_id) {
            return false;
        }
        // Bump the generation so trailing events from the dying backend cannot
        // flip the session's status or append late output.
        self.next_backend_generation(agent_id);
        // Dropping the controller sends Shutdown; the backend thread exits its
        // run loop and its Drop impl terminates the whole process group.
        self.remove_backend_controller(agent_id);
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        true
    }

    pub fn reset_session(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let _ = self.hard_stop_backend(agent_id, true, cx);
        self.sessions.remove(&agent_id);
        self.cancellation_requested.remove(&agent_id);
        self.paused_queues.remove(&agent_id);
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn force_stop_backend(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let _ = self.hard_stop_backend(agent_id, true, cx);
    }

    /// Stop a backend because its Solo lane is about to be merged or removed.
    /// This is a lifecycle transition, not a user pressing Stop, so it must not
    /// leave a misleading "Stopped by user" row in the conversation.
    pub fn stop_backend_for_lane_exit(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<ChatBackendStopSignal> {
        self.hard_stop_backend(agent_id, false, cx)
    }

    /// Stop every app-owned backend process before the application exits.
    pub fn shutdown_all(&mut self, cx: &mut Context<Self>) {
        for (_, controller) in self.controllers.drain() {
            controller.force_shutdown();
        }
        self.cancellation_requested.clear();
        for session in self.sessions.values_mut() {
            session.set_status(AgentChatStatus::Idle);
            session.started_running_at = None;
            session.pending_user_input = None;
            session.pending_approval = None;
        }
        for id in self.sessions.keys().copied().collect::<Vec<_>>() {
            self.publish_change(id, ChatChangeCategories::CONTENT, cx);
        }
    }

    fn hard_stop_backend(
        &mut self,
        agent_id: Uuid,
        record_user_stop: bool,
        cx: &mut Context<Self>,
    ) -> Option<ChatBackendStopSignal> {
        self.paused_queues.insert(agent_id);
        // A force-stop cannot wait for the provider's normal terminal event.
        // Promote the live action rows into the same immutable receipt shape
        // so the interrupted turn still lands in the drawer and survives a
        // relaunch.
        let interrupted_summary = self
            .sessions
            .get(&agent_id)
            .and_then(|session| pending_file_activity_summary(&session.timeline));
        if let Some(summary) = interrupted_summary {
            if let Some(session) = self.sessions.get_mut(&agent_id) {
                if let Some((receipt, ledger)) = apply_changed_files_summary(session, summary) {
                    persist_changed_files_turn(agent_id, receipt, ledger, cx);
                }
            }
        }
        self.next_backend_generation(agent_id);
        if let Some(controller) = self.remove_backend_controller(agent_id) {
            controller.force_shutdown();
        }
        let stop_signal = self.stopping_backends.get(&agent_id).cloned();
        self.cancellation_requested.remove(&agent_id);
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            if let Some(entry) = settle_hard_stopped_session(session, record_user_stop) {
                persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            }
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        stop_signal
    }

    fn remove_backend_controller(&mut self, agent_id: Uuid) -> Option<ChatBackendController> {
        let controller = self.controllers.remove(&agent_id)?;
        self.stopping_backends
            .insert(agent_id, controller.stop_signal());
        Some(controller)
    }

    pub fn submit_pending_user_input(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(pending) = session.pending_user_input.as_ref() else {
            return;
        };
        let Some(answers) = pending.build_answers() else {
            return;
        };
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::SubmitUserInput {
                request_id: pending.request_id.clone(),
                answers,
            });
        }
        session.pending_user_input = None;
        session
            .timeline
            .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
        session.set_status(AgentChatStatus::Running);
        session.last_activity_at = unix_now();
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn dismiss_pending_user_input(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        if let Some(controller) = self.controllers.get(&agent_id) {
            controller.cancel_turn();
        }
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        if session.pending_user_input.is_none() {
            return;
        }
        session.pending_user_input = None;
        session
            .timeline
            .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
        session.set_status(AgentChatStatus::Idle);
        session.started_running_at = None;
        session.last_activity_at = unix_now();
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub fn resolve_pending_approval(
        &mut self,
        agent_id: Uuid,
        request_id: &str,
        approved: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return false;
        };
        let Some(pending) = session.pending_approval.as_ref() else {
            return false;
        };
        if pending.request_id != request_id {
            return false;
        }
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::ResolveApproval {
                request_id: request_id.to_string(),
                approved,
            });
        } else {
            return false;
        }
        session.pending_approval = None;
        session.set_status(AgentChatStatus::Running);
        session.started_running_at = Some(unix_now());
        session.last_activity_at = unix_now();
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        true
    }

    pub fn append_message(
        &mut self,
        agent_id: Uuid,
        message: AgentChatMessage,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            let merged_message = append_or_extend_message(&mut session.messages, message.clone());
            append_or_extend_timeline_message(&mut session.timeline, message);
            persist_chat_message(agent_id, merged_message, cx);
            self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
        }
    }

    fn apply_backend_event(
        &mut self,
        agent_id: Uuid,
        generation: u64,
        event: ChatBackendEvent,
        cx: &mut Context<Self>,
    ) {
        let (event, _reservation) = match event {
            ChatBackendEvent::ReservedEvidence { event, reservation } => (*event, Some(reservation)),
            ChatBackendEvent::EvidenceOverflow => return,
            other => (other, None),
        };
        self.current_event_received_at = Some(std::time::Instant::now());
        self.process_backend_event(agent_id, generation, event, cx);
        self.current_event_received_at = None;
    }

    fn process_backend_event(&mut self, agent_id: Uuid, generation: u64, event: ChatBackendEvent, cx: &mut Context<Self>) {
        let change_categories = match &event {
            ChatBackendEvent::AssistantChunk { .. } | ChatBackendEvent::ThoughtChunk { .. }
            | ChatBackendEvent::WorkLog(_) | ChatBackendEvent::FileChangeActivity(_)
            | ChatBackendEvent::Usage(_) | ChatBackendEvent::Compaction(_) => ChatChangeCategories::CONVERSATION,
            _ => ChatChangeCategories::CONTENT,
        };
        let now = unix_now();
        let current_generation =
            self.backend_generations.get(&agent_id).copied() == Some(generation);
        // Receipts belong to immutable turns, including retired backends. They
        // cannot change provider status, consume queued prompts, or end new work.
        match &event {
            ChatBackendEvent::ChangeReceiptPending(turn_id) => {
                let session = self.ensure_backend_event_session(agent_id, now);
                let entry = WorkLogEntry::new(
                    format!("file-receipt:{turn_id}"),
                    format!("file-receipt:{turn_id}"),
                    WorkLogEntryKind::System,
                    "Updating changes",
                    WorkLogStatus::InProgress,
                );
                upsert_timeline_work_log(&mut session.timeline, entry.clone());
                persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
                self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
                return;
            }
            ChatBackendEvent::ChangeReceiptReady { summary, state } => {
                let session = self.ensure_backend_event_session(agent_id, now);
                let turn_id = summary.turn_id.clone().unwrap_or_default();
                let complete = *state == ide_core::agent_changes::ChangeReceiptState::Ready;
                let entry = WorkLogEntry::new(
                    format!("file-receipt:{turn_id}"),
                    format!("file-receipt:{turn_id}"),
                    WorkLogEntryKind::System,
                    if complete {
                        "Changes recorded"
                    } else {
                        "Some change details are unavailable"
                    },
                    if complete {
                        WorkLogStatus::Completed
                    } else {
                        WorkLogStatus::Failed
                    },
                );
                upsert_timeline_work_log(&mut session.timeline, entry.clone());
                persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
                if let Some((receipt, ledger)) =
                    apply_changed_files_summary(session, summary.clone())
                {
                    persist_changed_files_turn(agent_id, receipt, ledger, cx);
                }
                let review = current_generation
                    && matches!(
                        session.status,
                        AgentChatStatus::Idle | AgentChatStatus::PlanReady
                    )
                    && latest_changed_source_turn(&session.timeline).as_deref() == Some(&turn_id);
                if review && self.finished_file_turns.insert((agent_id, turn_id.clone())) {
                    cx.emit(AgentChatEvent::TurnFinished {
                        agent_id,
                        source_turn_id: turn_id,
                    });
                }
                self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
                return;
            }
            _ => {}
        }
        if self.backend_generations.get(&agent_id).copied() != Some(generation) {
            if self.apply_stale_backend_event(agent_id, &event, now) {
                self.publish_change(agent_id, change_categories, cx);
            }
            return;
        }
        // A failed runtime cannot receive another turn. Remove only the
        // controller for the current generation so the next submission starts
        // a fresh backend and resumes from the persisted provider session id.
        let checklist_maintenance_error = matches!(&event, ChatBackendEvent::Error(_))
            && self
                .sessions
                .get(&agent_id)
                .is_some_and(|session| latest_user_is_review_checklist(&session.timeline));
        if matches!(&event, ChatBackendEvent::Error(_)) && !checklist_maintenance_error {
            self.remove_backend_controller(agent_id);
        }
        if self.cancellation_requested.contains(&agent_id) {
            let should_drain_queue = match event {
                ChatBackendEvent::FileChangeActivity(activity) => {
                    // A successful mutation can finish while Stop is in flight.
                    // Keep its original turn anchor without reviving the run.
                    let session = self.ensure_backend_event_session(agent_id, now);
                    upsert_timeline_file_change_activity(&mut session.timeline, activity.clone());
                    persist_timeline_item(
                        agent_id,
                        AgentChatTimelineItem::FileChangeActivity(activity),
                        cx,
                    );
                    false
                }
                ChatBackendEvent::Status(AgentChatStatus::Idle | AgentChatStatus::Failed)
                | ChatBackendEvent::Error(_) => {
                    self.cancellation_requested.remove(&agent_id);
                    let session = self.ensure_backend_event_session(agent_id, now);
                    session.last_activity_at = now;
                    session.set_status(AgentChatStatus::Idle);
                    session.started_running_at = None;
                    session.pending_user_input = None;
                    session.pending_approval = None;
                    session
                        .timeline
                        .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
                    let entry = WorkLogEntry::new(
                        next_local_id(),
                        "agent-chat-cancelled",
                        WorkLogEntryKind::System,
                        "Stopped by user",
                        WorkLogStatus::Completed,
                    );
                    upsert_work_log_entry(&mut session.work_log, entry.clone());
                    // Keep every user stop as the newest visible timeline row.
                    session
                        .timeline
                        .push(AgentChatTimelineItem::WorkLog(entry.clone()));
                    persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
                    session.status == AgentChatStatus::Idle && !session.queued_turns.is_empty()
                }
                ChatBackendEvent::ChatSessionReady { session_id } => {
                    let session = self.ensure_backend_event_session(agent_id, now);
                    session.last_activity_at = now;
                    if is_real_cli_session_id(agent_id, &session_id) {
                        session.chat_session_id = Some(session_id);
                    }
                    session.status == AgentChatStatus::Idle && !session.queued_turns.is_empty()
                }
                ChatBackendEvent::SessionReady { session_id } => {
                    let session = self.ensure_backend_event_session(agent_id, now);
                    session.last_activity_at = now;
                    if is_real_cli_session_id(agent_id, &session_id) {
                        session.cli_session_id = Some(session_id);
                    }
                    session.status == AgentChatStatus::Idle && !session.queued_turns.is_empty()
                }
                ChatBackendEvent::Usage(usage) => {
                    let session = self.ensure_backend_event_session(agent_id, now);
                    session.last_activity_at = now;
                    apply_usage_snapshot(session, usage);
                    false
                }
                _ => {
                    return;
                }
            };
            self.publish_change(agent_id, change_categories, cx);
            if should_drain_queue {
                self.schedule_next_queued_turn(agent_id, cx);
            }
            return;
        }
        let previous_status = self.sessions.get(&agent_id).map(|session| session.status);
        let session = self.ensure_backend_event_session(agent_id, now);
        session.last_activity_at = now;
        match event {
            ChatBackendEvent::ChatSessionReady { session_id } => {
                if is_real_cli_session_id(agent_id, &session_id) {
                    session.chat_session_id = Some(session_id);
                }
            }
            ChatBackendEvent::AssistantChunk { message_id, text } => {
                let message = AgentChatMessage::Assistant {
                    message_id,
                    text,
                    created_at: now,
                };
                let merged_message =
                    append_or_extend_message(&mut session.messages, message.clone());
                append_or_extend_timeline_message(&mut session.timeline, message);
                persist_chat_message(agent_id, merged_message, cx);
            }
            ChatBackendEvent::SessionReady { session_id } => {
                if is_real_cli_session_id(agent_id, &session_id) {
                    session.cli_session_id = Some(session_id);
                }
            }
            ChatBackendEvent::ThoughtChunk { message_id, text } => {
                let message = AgentChatMessage::Thought {
                    message_id,
                    text,
                    created_at: now,
                };
                let merged_message =
                    append_or_extend_message(&mut session.messages, message.clone());
                append_or_extend_timeline_message(&mut session.timeline, message);
                persist_chat_message(agent_id, merged_message, cx);
            }
            ChatBackendEvent::WorkLog(entry) => {
                let entry = entry.redact_sensitive();
                upsert_work_log_entry(&mut session.work_log, entry.clone());
                upsert_timeline_work_log(&mut session.timeline, entry.clone());
                persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            }
            ChatBackendEvent::FileChangeActivity(activity) => {
                upsert_timeline_file_change_activity(&mut session.timeline, activity.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::FileChangeActivity(activity),
                    cx,
                );
            }
            ChatBackendEvent::PendingUserInput(pending) => {
                upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::PendingUserInput(pending.clone()),
                    cx,
                );
                session.pending_user_input = Some(pending);
                session.set_status(AgentChatStatus::WaitingForUser);
                session.started_running_at = None;
            }
            ChatBackendEvent::PendingApproval(pending) => {
                session.pending_approval = Some(pending);
                session.set_status(AgentChatStatus::WaitingForUser);
                session.started_running_at = None;
            }
            ChatBackendEvent::ProposedPlan(plan) => {
                remove_proposed_plan_blocks(&mut session.messages);
                remove_proposed_plan_blocks_from_timeline(&mut session.timeline);
                upsert_timeline_proposed_plan(&mut session.timeline, plan.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::ProposedPlan(plan.clone()),
                    cx,
                );
                session.proposed_plan = Some(plan);
                session.set_status(AgentChatStatus::PlanReady);
                session.interaction_mode = AgentInteractionMode::Plan;
                session.started_running_at = None;
            }
            ChatBackendEvent::CodeReview(review) => {
                remove_code_review_blocks(&mut session.messages);
                remove_code_review_blocks_from_timeline(&mut session.timeline);
                upsert_timeline_code_review(&mut session.timeline, review.clone());
                persist_timeline_item(agent_id, AgentChatTimelineItem::CodeReview(review), cx);
            }
            ChatBackendEvent::Verification(verification) => {
                remove_verification_blocks(&mut session.messages);
                remove_verification_blocks_from_timeline(&mut session.timeline);
                upsert_timeline_verification(&mut session.timeline, verification.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::Verification(verification),
                    cx,
                );
            }
            ChatBackendEvent::ReviewChecklist(mut checklist) => {
                remove_review_checklist_blocks(&mut session.messages);
                remove_review_checklist_blocks_from_timeline(&mut session.timeline);
                if checklist.source_turn_id.is_empty() {
                    if let Some(pending) = session.timeline.iter().rev().find_map(|item| match item
                    {
                        AgentChatTimelineItem::ReviewChecklist(candidate)
                            if candidate.status == ReviewChecklistStatus::Pending =>
                        {
                            Some(candidate)
                        }
                        _ => None,
                    }) {
                        checklist.source_turn_id = pending.source_turn_id.clone();
                        checklist.id = pending.id.clone();
                        checklist.created_at = pending.created_at;
                    } else {
                        return;
                    }
                }
                upsert_timeline_review_checklist(&mut session.timeline, checklist.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::ReviewChecklist(checklist),
                    cx,
                );
            }
            ChatBackendEvent::ReservedEvidence { .. } | ChatBackendEvent::EvidenceOverflow => {
                unreachable!("evidence transport must be consumed by the router")
            }
            ChatBackendEvent::ChangeReceiptPending(_)
            | ChatBackendEvent::ChangeReceiptReady { .. } => {
                unreachable!("handled independently of backend status")
            }
            ChatBackendEvent::ChangedFiles(summary) => {
                if let Some((receipt, ledger)) = apply_changed_files_summary(session, summary) {
                    persist_changed_files_turn(agent_id, receipt, ledger, cx);
                }
            }
            ChatBackendEvent::Usage(usage) => {
                apply_usage_snapshot(session, usage);
            }
            ChatBackendEvent::Compaction(active) => {
                session.set_compacting(active);
            }
            ChatBackendEvent::Status(status) => {
                if status == AgentChatStatus::Idle
                    && session
                        .proposed_plan
                        .as_ref()
                        .is_some_and(|plan| plan.implemented_at.is_none())
                {
                    session.set_status(AgentChatStatus::PlanReady);
                } else {
                    session.set_status(status);
                }
                if !matches!(session.status, AgentChatStatus::Running) {
                    session.started_running_at = None;
                } else if session.started_running_at.is_none() {
                    session.started_running_at = Some(now);
                }
            }
            ChatBackendEvent::Error(error) => {
                if checklist_maintenance_error {
                    if let Some(checklist) =
                        session
                            .timeline
                            .iter_mut()
                            .rev()
                            .find_map(|item| match item {
                                AgentChatTimelineItem::ReviewChecklist(checklist)
                                    if checklist.status == ReviewChecklistStatus::Pending =>
                                {
                                    Some(checklist)
                                }
                                _ => None,
                            })
                    {
                        checklist.status = ReviewChecklistStatus::Failed;
                        persist_timeline_item(
                            agent_id,
                            AgentChatTimelineItem::ReviewChecklist(checklist.clone()),
                            cx,
                        );
                    }
                    session.set_status(AgentChatStatus::Idle);
                    session.started_running_at = None;
                    session.pending_user_input = None;
                    session.pending_approval = None;
                } else {
                    let error_message_id = format!("error-{}", session.messages.len());
                    let message = AgentChatMessage::Assistant {
                        message_id: Some(error_message_id),
                        text: error,
                        created_at: now,
                    };
                    append_or_extend_message(&mut session.messages, message.clone());
                    append_or_extend_timeline_message(&mut session.timeline, message.clone());
                    persist_chat_message(agent_id, message, cx);
                    session.set_status(AgentChatStatus::Failed);
                    session.started_running_at = None;
                    session.pending_user_input = None;
                    session.pending_approval = None;
                    session
                        .timeline
                        .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
                }
            }
        }
        let should_drain_queue =
            session.status == AgentChatStatus::Idle && !session.queued_turns.is_empty();
        let failed_checklist = if previous_status == Some(AgentChatStatus::Running)
            && session.status == AgentChatStatus::Idle
            && latest_user_is_review_checklist(&session.timeline)
        {
            session
                .timeline
                .iter_mut()
                .rev()
                .find_map(|item| match item {
                    AgentChatTimelineItem::ReviewChecklist(checklist)
                        if checklist.status == ReviewChecklistStatus::Pending =>
                    {
                        checklist.status = ReviewChecklistStatus::Failed;
                        Some(checklist.clone())
                    }
                    _ => None,
                })
        } else {
            None
        };
        if let Some(checklist) = failed_checklist {
            persist_timeline_item(
                agent_id,
                AgentChatTimelineItem::ReviewChecklist(checklist),
                cx,
            );
        }
        let work_finished = is_work_completion_transition(previous_status, session.status);
        let finished_source_turn = work_finished
            .then(|| latest_changed_source_turn(&session.timeline))
            .flatten();
        if let Some(source_turn_id) = finished_source_turn
            .filter(|id| self.finished_file_turns.insert((agent_id, id.clone())))
        {
            cx.emit(AgentChatEvent::TurnFinished {
                agent_id,
                source_turn_id,
            });
        }
        if work_finished {
            cx.emit(AgentChatEvent::WorkFinished { agent_id });
        }
        self.publish_change(agent_id, change_categories, cx);
        if should_drain_queue {
            self.schedule_next_queued_turn(agent_id, cx);
        }
    }

    fn next_backend_generation(&mut self, agent_id: Uuid) -> u64 {
        let generation = self
            .backend_generations
            .get(&agent_id)
            .copied()
            .unwrap_or(0)
            .wrapping_add(1);
        self.backend_generations.insert(agent_id, generation);
        generation
    }

    fn ensure_backend_event_session(&mut self, agent_id: Uuid, now: u64) -> &mut AgentChatSession {
        self.sessions
            .entry(agent_id)
            .or_insert_with(|| AgentChatSession {
                agent_id,
                title: String::from("Agent chat"),
                chat_session_id: None,
                cli_session_id: None,
                hidden_from_notifications: false,
                is_compacting: false,
                status: AgentChatStatus::Idle,
                interaction_mode: AgentInteractionMode::Default,
                composer_text: String::new(),
                messages: Vec::new(),
                timeline: Vec::new(),
                queued_turns: Vec::new(),
                work_log: Vec::new(),
                pending_user_input: None,
                pending_approval: None,
                proposed_plan: None,
                changed_files: ChangedFilesSummary::default(),
                usage: None,
                started_running_at: None,
                last_activity_at: now,
            })
    }

    fn apply_backend_identity_event(
        &mut self,
        agent_id: Uuid,
        event: &ChatBackendEvent,
        now: u64,
    ) -> bool {
        match event {
            ChatBackendEvent::ChatSessionReady { session_id }
                if is_real_cli_session_id(agent_id, session_id) =>
            {
                let session = self.ensure_backend_event_session(agent_id, now);
                if session.chat_session_id.as_deref() == Some(session_id.as_str()) {
                    return false;
                }
                session.chat_session_id = Some(session_id.clone());
                session.last_activity_at = now;
                true
            }
            ChatBackendEvent::SessionReady { session_id }
                if is_real_cli_session_id(agent_id, session_id) =>
            {
                let session = self.ensure_backend_event_session(agent_id, now);
                if session.cli_session_id.as_deref() == Some(session_id.as_str()) {
                    return false;
                }
                session.cli_session_id = Some(session_id.clone());
                session.last_activity_at = now;
                true
            }
            _ => false,
        }
    }

    fn apply_stale_backend_event(
        &mut self,
        agent_id: Uuid,
        event: &ChatBackendEvent,
        now: u64,
    ) -> bool {
        self.apply_backend_identity_event(agent_id, event, now)
    }
}

fn is_work_completion_transition(
    previous: Option<AgentChatStatus>,
    current: AgentChatStatus,
) -> bool {
    previous == Some(AgentChatStatus::Running) && current == AgentChatStatus::Idle
}

fn latest_changed_source_turn(timeline: &[AgentChatTimelineItem]) -> Option<String> {
    let latest_user = timeline.iter().rposition(|item| {
        matches!(
            item,
            AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
        )
    })?;
    let user_is_maintenance = matches!(
        timeline.get(latest_user),
        Some(AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }))
            if text.starts_with(REVIEW_CHECKLIST_REQUEST_MARKER)
    );
    if user_is_maintenance {
        return None;
    }
    timeline[latest_user + 1..]
        .iter()
        .rev()
        .find_map(|item| match item {
            AgentChatTimelineItem::ChangedFiles(summary) if !summary.is_empty() => {
                summary.turn_id.clone()
            }
            _ => None,
        })
}

fn latest_user_is_review_checklist(timeline: &[AgentChatTimelineItem]) -> bool {
    timeline.iter().rev().find_map(|item| match item {
        AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => {
            Some(text.starts_with(REVIEW_CHECKLIST_REQUEST_MARKER))
        }
        _ => None,
    }) == Some(true)
}

fn is_real_cli_session_id(agent_id: Uuid, session_id: &str) -> bool {
    !session_id.trim().is_empty() && session_id != agent_id.to_string()
}

fn settle_hard_stopped_session(
    session: &mut AgentChatSession,
    record_user_stop: bool,
) -> Option<WorkLogEntry> {
    session.set_status(AgentChatStatus::Idle);
    session.started_running_at = None;
    session.pending_user_input = None;
    session.pending_approval = None;
    session
        .timeline
        .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
    let entry = record_user_stop.then(|| {
        WorkLogEntry::new(
            next_local_id(),
            "agent-chat-force-stopped",
            WorkLogEntryKind::System,
            "Stopped by user",
            WorkLogStatus::Completed,
        )
        .detail(Some(
            "Queued messages are kept and paused until you send another message.".to_string(),
        ))
    });
    if let Some(entry) = entry.as_ref() {
        upsert_work_log_entry(&mut session.work_log, entry.clone());
        // A stop is a point-in-time outcome. Never merge it into an older
        // stop event or it disappears from the end of the conversation.
        session
            .timeline
            .push(AgentChatTimelineItem::WorkLog(entry.clone()));
    }
    session.last_activity_at = unix_now();
    entry
}

fn apply_changed_files_summary(
    session: &mut AgentChatSession,
    summary: ChangedFilesSummary,
) -> Option<(ChangedFilesSummary, ChangedFilesSummary)> {
    if summary.is_empty() {
        return None;
    }
    if summary.turn_id.is_some()
        && session.timeline.iter().any(|item| {
            let AgentChatTimelineItem::ChangedFiles(previous) = item else {
                return false;
            };
            let stats = |files: &[FileChangeStat]| {
                files
                    .iter()
                    .map(StoredFileChange::from_stat)
                    .collect::<Vec<_>>()
            };
            previous.turn_id == summary.turn_id
                && stats(&previous.files) == stats(&summary.files)
                && stats(&previous.observed_files) == stats(&summary.observed_files)
                && previous.snapshot_id == summary.snapshot_id
        })
    {
        return None;
    }
    let revision = session
        .changed_files
        .ledger_revision
        .max(summary.ledger_revision)
        .saturating_add(1);
    let mut receipt = summary;
    receipt.ledger_revision = revision;
    append_timeline_changed_files(&mut session.timeline, receipt.clone());
    let mut merged = ChangedFilesSummary::default();
    for item in &session.timeline {
        if let AgentChatTimelineItem::ChangedFiles(turn) = item {
            merged.merge_turn(turn);
        }
    }
    merged.ledger_revision = revision;
    session.changed_files = merged;
    Some((receipt, session.changed_files.clone()))
}

fn pending_file_activity_summary(
    timeline: &[AgentChatTimelineItem],
) -> Option<ChangedFilesSummary> {
    let turn_id = timeline.iter().rev().find_map(|item| match item {
        AgentChatTimelineItem::FileChangeActivity(activity) => Some(activity.turn_id.as_str()),
        _ => None,
    })?;
    if timeline.iter().any(|item| {
        matches!(item, AgentChatTimelineItem::ChangedFiles(summary) if summary.turn_id.as_deref() == Some(turn_id))
    }) {
        return None;
    }
    let summary = ChangedFilesSummary::from_activities(
        turn_id,
        timeline.iter().filter_map(|item| match item {
            AgentChatTimelineItem::FileChangeActivity(activity) => Some(activity),
            _ => None,
        }),
    );
    (!summary.is_empty()).then_some(summary)
}

/// Structural half of the idle-retirement guard: nothing in-flight that a
/// backend kill would lose, and at least one provider session id so the next
/// submission can resume. The caller layers policy on top (idle duration,
/// selection, special assistants, provider-specific resume rules).
fn session_safe_to_retire(session: &AgentChatSession) -> bool {
    session.status == AgentChatStatus::Idle
        && session.queued_turns.is_empty()
        && session.pending_user_input.is_none()
        && session.pending_approval.is_none()
        && (session.chat_session_id.is_some() || session.cli_session_id.is_some())
}

#[cfg(test)]
mod retirement_tests {
    use super::*;
    use std::path::PathBuf;

    fn retirable_session() -> AgentChatSession {
        AgentChatSession {
            agent_id: Uuid::new_v4(),
            title: "Chat".to_string(),
            chat_session_id: None,
            cli_session_id: Some("claude-session-1".to_string()),
            hidden_from_notifications: false,
            is_compacting: false,
            status: AgentChatStatus::Idle,
            interaction_mode: AgentInteractionMode::Default,
            composer_text: String::new(),
            messages: Vec::new(),
            timeline: Vec::new(),
            queued_turns: Vec::new(),
            work_log: Vec::new(),
            pending_user_input: None,
            pending_approval: None,
            proposed_plan: None,
            changed_files: ChangedFilesSummary::default(),
            usage: None,
            started_running_at: None,
            last_activity_at: 0,
        }
    }

    #[test]
    fn replaying_a_turn_receipt_does_not_double_the_file_counts() {
        let mut session = retirable_session();
        let receipt = ChangedFilesSummary::attributed(
            "turn-replay",
            vec![FileChangeStat::new("a.rs", 2, 1)],
            vec![],
        );
        assert!(apply_changed_files_summary(&mut session, receipt.clone()).is_some());
        assert!(apply_changed_files_summary(&mut session, receipt).is_none());
        assert_eq!(session.changed_files.files[0].additions, 2);
        assert_eq!(session.changed_files.ledger_revision, 1);
    }

    #[test]
    fn idle_session_with_resume_id_is_retirable() {
        assert!(session_safe_to_retire(&retirable_session()));
        let mut codex = retirable_session();
        codex.cli_session_id = None;
        codex.chat_session_id = Some("codex-thread-1".to_string());
        assert!(session_safe_to_retire(&codex));
    }

    #[cfg(feature = "ui-layout-tests")]
    #[gpui::test]
    fn studio_queue_preserves_active_scope_and_fifo(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext;
        let (store, design, agent_id, request) = studio_request::tests::fixture();
        let active = ide_core::studio::scope_for_request(&design, None, None);
        store.save_scope(agent_id, &active).unwrap();
        let chats = cx.new(|_| AgentChatState::new());
        chats.update(cx, |state, cx| {
            state.ensure_session(agent_id, "Designer", cx).set_status(AgentChatStatus::Running);
            let first = state.queue_turn_with_studio(agent_id, "First".into(), None, vec![],
                AgentInteractionMode::Default, Some(request.clone()), cx);
            let second = state.queue_turn_with_studio(agent_id, "Second".into(), None, vec![],
                AgentInteractionMode::Default, Some(request), cx);
            for status in [AgentChatStatus::Running, AgentChatStatus::Cancelling] {
                state.sessions.get_mut(&agent_id).unwrap().set_status(status);
                assert!(!state.validate_queued_studio_request(agent_id, first, cx));
                assert_eq!(store.scope(agent_id).unwrap().id, active.id);
                assert_eq!(state.sessions[&agent_id].queued_turns.len(), 2);
            }
            state.sessions.get_mut(&agent_id).unwrap().set_status(AgentChatStatus::Idle);
            assert!(state.validate_queued_studio_request(agent_id, first, cx));
            assert_eq!(store.scope(agent_id).unwrap().id, active.id);
            assert_eq!(state.take_next_queued_turn(agent_id).unwrap().id, first);
            assert_eq!(state.take_next_queued_turn(agent_id).unwrap().id, second);
        });
    }

    fn queued_handoff_fixture(kind: &str) -> QueuedChatTurn {
        QueuedChatTurn {
            id: Uuid::new_v4(),
            text: "Prepared teammate context".to_string(),
            display_text: Some("Teammate request".to_string()),
            tags: Vec::new(),
            mode: AgentInteractionMode::Default,
            created_at: 0,
            handoff: Some(QueuedAgentHandoff {
                target_agent_id: Uuid::new_v4(),
                target_title: "SDK agent".to_string(),
                kind: kind.to_string(),
                original_text: "Check the SDK".to_string(),
                references: "File: sdk.rs".to_string(),
            }),
            studio_request: None,
        }
    }

    #[test]
    fn queued_handoff_waits_for_active_work_decisions_and_existing_queue() {
        let mut state = AgentChatState::new();
        let session = retirable_session();
        let id = session.agent_id;
        state.sessions.insert(id, session);
        for status in [
            AgentChatStatus::Running,
            AgentChatStatus::Cancelling,
            AgentChatStatus::WaitingForUser,
            AgentChatStatus::PlanReady,
        ] {
            state.sessions.get_mut(&id).unwrap().set_status(status);
            assert!(state.handoff_must_wait(id), "{status:?}");
        }
        state
            .sessions
            .get_mut(&id)
            .unwrap()
            .set_status(AgentChatStatus::Idle);
        assert!(!state.handoff_must_wait(id));
        state
            .sessions
            .get_mut(&id)
            .unwrap()
            .queued_turns
            .push(queued_handoff_fixture("ask"));
        assert!(state.handoff_must_wait(id));
        assert!(state.has_queued_work(id));
        state.take_next_queued_turn(id);
        state.handoffs_sending.insert(id);
        assert!(state.handoff_must_wait(id));
        assert!(state.has_queued_work(id));
        state.handoffs_sending.remove(&id);
        assert!(!state.handoff_must_wait(id));
    }

    #[test]
    fn queued_handoffs_keep_fifo_order_and_routing_metadata() {
        for kind in ["ask", "delegate"] {
            let mut state = AgentChatState::new();
            let mut session = retirable_session();
            let id = session.agent_id;
            let handoff = queued_handoff_fixture(kind);
            let mut first = queued_handoff_fixture(kind);
            first.handoff = None;
            let mut last = first.clone();
            last.id = Uuid::new_v4();
            session.queued_turns = vec![first.clone(), handoff.clone(), last.clone()];
            state.sessions.insert(id, session);
            assert_eq!(state.take_next_queued_turn(id), Some(first));
            assert_eq!(state.take_next_queued_turn(id), Some(handoff));
            assert_eq!(state.take_next_queued_turn(id), Some(last));
            assert_eq!(state.take_next_queued_turn(id), None);
        }
    }

    #[test]
    fn compaction_preserves_the_running_turn_and_ends_explicitly() {
        let mut session = retirable_session();
        session.set_status(AgentChatStatus::Running);
        session.started_running_at = Some(123);
        session.set_compacting(true);
        assert!(session.is_compacting);
        assert_eq!(session.status, AgentChatStatus::Running);
        assert_eq!(session.started_running_at, Some(123));
        assert!(!session_safe_to_retire(&session));
        session.set_compacting(false);
        assert!(!session.is_compacting);
        assert_eq!(session.status, AgentChatStatus::Running);
        assert_eq!(session.started_running_at, Some(123));
    }

    #[test]
    fn compaction_clears_on_every_turn_transition() {
        for status in [
            AgentChatStatus::Idle,
            AgentChatStatus::Running,
            AgentChatStatus::Cancelling,
            AgentChatStatus::WaitingForUser,
            AgentChatStatus::PlanReady,
            AgentChatStatus::Failed,
        ] {
            let mut session = retirable_session();
            session.set_status(AgentChatStatus::Running);
            session.set_compacting(true);
            session.set_status(status);
            assert!(!session.is_compacting, "status {status:?}");
            if status != AgentChatStatus::Running {
                session.set_compacting(true);
                assert!(!session.is_compacting, "late event for {status:?}");
            }
        }
    }

    #[test]
    fn any_in_flight_state_blocks_retirement() {
        for status in [
            AgentChatStatus::Running,
            AgentChatStatus::Cancelling,
            AgentChatStatus::WaitingForUser,
            AgentChatStatus::PlanReady,
            AgentChatStatus::Failed,
        ] {
            let mut session = retirable_session();
            session.set_status(status);
            assert!(!session_safe_to_retire(&session), "status {status:?}");
        }

        let mut queued = retirable_session();
        queued.queued_turns.push(QueuedChatTurn {
            id: Uuid::new_v4(),
            text: "next".to_string(),
            display_text: None,
            tags: Vec::new(),
            mode: AgentInteractionMode::Default,
            created_at: 0,
            handoff: None,
            studio_request: None,
        });
        assert!(!session_safe_to_retire(&queued));

        let mut asking = retirable_session();
        asking.pending_user_input = Some(PendingUserInput::new("q-1", Vec::new()));
        assert!(!session_safe_to_retire(&asking));

        let mut approving = retirable_session();
        approving.pending_approval = Some(PendingApproval::new(
            "a-1",
            PendingApprovalKind::Command,
            "Allow?",
            None,
        ));
        assert!(!session_safe_to_retire(&approving));
    }

    #[test]
    fn missing_resume_id_blocks_retirement() {
        let mut session = retirable_session();
        session.cli_session_id = None;
        session.chat_session_id = None;
        assert!(!session_safe_to_retire(&session));
    }
    #[test]
    fn lane_exit_stops_without_claiming_the_user_pressed_stop() {
        let mut session = retirable_session();
        session.set_status(AgentChatStatus::Running);

        let entry = settle_hard_stopped_session(&mut session, false);

        assert!(entry.is_none());
        assert_eq!(session.status, AgentChatStatus::Idle);
        assert!(session.timeline.is_empty());
        assert!(session.work_log.is_empty());
    }

    #[test]
    fn explicit_force_stop_keeps_the_user_stop_outcome() {
        let mut session = retirable_session();
        session.set_status(AgentChatStatus::Running);

        let entry = settle_hard_stopped_session(&mut session, true)
            .expect("an explicit stop should produce a timeline outcome");

        assert_eq!(entry.title, "Stopped by user");
        assert!(session.timeline.iter().any(|item| matches!(
            item,
            AgentChatTimelineItem::WorkLog(entry) if entry.title == "Stopped by user"
        )));
    }

    #[test]
    fn repeated_force_stops_preserve_every_queued_message_and_handoff() {
        let mut session = retirable_session();
        let mut message = queued_handoff_fixture("ask");
        message.handoff = None;
        message.text = "Review the attached image: /tmp/reference.png".into();
        message.display_text = Some("Review the attached image".into());
        let queued = vec![message, queued_handoff_fixture("delegate")];
        session.queued_turns = queued.clone();
        session.set_status(AgentChatStatus::Cancelling);

        for _ in 0..2 {
            settle_hard_stopped_session(&mut session, true);
            assert_eq!(session.status, AgentChatStatus::Idle);
            assert_eq!(session.queued_turns, queued);
        }
    }

    #[test]
    fn paused_queue_stays_paused_after_completion_and_new_arrivals() {
        let mut state = AgentChatState::new();
        let mut session = retirable_session();
        let id = session.agent_id;
        let first = queued_handoff_fixture("ask");
        let second = queued_handoff_fixture("delegate");
        session.queued_turns = vec![first.clone()];
        session.set_status(AgentChatStatus::Cancelling);
        state.sessions.insert(id, session);
        state.paused_queues.insert(id);
        assert!(!state.can_drain_queue(id));

        let session = state.sessions.get_mut(&id).unwrap();
        session.set_status(AgentChatStatus::Idle);
        session.queued_turns.push(second.clone());
        assert!(!state.can_drain_queue(id));
        assert_eq!(
            state.session(id).unwrap().queued_turns,
            vec![first.clone(), second.clone()]
        );

        // The next explicit Send releases the pause; FIFO order is unchanged.
        state.paused_queues.remove(&id);
        assert!(state.can_drain_queue(id));
        assert_eq!(state.take_next_queued_turn(id), Some(first));
        assert_eq!(state.take_next_queued_turn(id), Some(second));
    }

    #[test]
    fn queue_dispatch_waits_for_cancellation_and_in_flight_handoffs() {
        let mut state = AgentChatState::new();
        let mut session = retirable_session();
        let id = session.agent_id;
        session.queued_turns.push(queued_handoff_fixture("ask"));
        session.set_status(AgentChatStatus::Cancelling);
        state.sessions.insert(id, session);
        // Sending during cancellation must wait for the stop acknowledgement.
        assert!(!state.can_drain_queue(id));
        state
            .sessions
            .get_mut(&id)
            .unwrap()
            .set_status(AgentChatStatus::Idle);
        state.handoffs_sending.insert(id);
        assert!(!state.can_drain_queue(id));
        state.handoffs_sending.remove(&id);
        assert!(state.can_drain_queue(id));
    }

    #[test]
    fn queued_message_cannot_be_consumed_without_a_backend_after_force_stop() {
        let mut state = AgentChatState::new();
        let mut session = retirable_session();
        let id = session.agent_id;
        let mut message = queued_handoff_fixture("ask");
        message.handoff = None;
        session.queued_turns.push(message.clone());
        state.sessions.insert(id, session);
        assert!(!state.can_drain_queue(id));
        assert_eq!(state.session(id).unwrap().queued_turns, vec![message]);
    }

    #[test]
    fn interrupted_activity_still_closes_the_previous_turn_after_a_steer_message() {
        let mut session = retirable_session();
        session
            .timeline
            .push(AgentChatTimelineItem::FileChangeActivity(
                FileChangeActivity::new(
                    "edit:index",
                    "turn-a",
                    FileChangeStat::new("index.html", 23, 34),
                    false,
                    10,
                ),
            ));
        session
            .timeline
            .push(AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: "continue with the next part".to_string(),
                display_text: None,
                tags: Vec::new(),
                created_at: 11,
            }));

        let summary = pending_file_activity_summary(&session.timeline)
            .expect("the interrupted provider turn should still have a receipt");
        assert_eq!(summary.turn_id.as_deref(), Some("turn-a"));
        assert_eq!(summary.files[0].path, PathBuf::from("index.html"));
        apply_changed_files_summary(&mut session, summary)
            .expect("receipt should update the cumulative ledger");

        assert!(pending_file_activity_summary(&session.timeline).is_none());
        assert!(session.timeline.iter().any(|item| matches!(
            item,
            AgentChatTimelineItem::ChangedFiles(summary)
                if summary.turn_id.as_deref() == Some("turn-a")
        )));
        assert_eq!(
            session.changed_files.files[0].path,
            PathBuf::from("index.html")
        );
    }

    #[test]
    fn completed_file_changing_turn_reports_only_its_targeted_source_turn() {
        let timeline = vec![
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: "implement it".to_string(),
                display_text: None,
                tags: Vec::new(),
                created_at: 1,
            }),
            AgentChatTimelineItem::ChangedFiles(ChangedFilesSummary::attributed(
                "turn-7",
                vec![FileChangeStat::new("src/main.rs", 2, 0)],
                Vec::new(),
            )),
        ];

        assert_eq!(
            latest_changed_source_turn(&timeline).as_deref(),
            Some("turn-7")
        );
    }

    #[test]
    fn remote_work_completion_does_not_require_a_user_timeline_message() {
        let timeline = vec![AgentChatTimelineItem::ChangedFiles(
            ChangedFilesSummary::attributed(
                "remote-turn",
                vec![FileChangeStat::new("mock.html", 15, 0)],
                Vec::new(),
            ),
        )];

        assert_eq!(latest_changed_source_turn(&timeline), None);
        assert!(is_work_completion_transition(
            Some(AgentChatStatus::Running),
            AgentChatStatus::Idle,
        ));
        assert!(!is_work_completion_transition(
            Some(AgentChatStatus::Running),
            AgentChatStatus::WaitingForUser,
        ));
    }

    #[test]
    fn checklist_maintenance_turn_never_recursively_requests_a_checklist() {
        let timeline = vec![
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: format!("{REVIEW_CHECKLIST_REQUEST_MARKER}\nSource turn: turn-7"),
                display_text: None,
                tags: Vec::new(),
                created_at: 1,
            }),
            AgentChatTimelineItem::ChangedFiles(ChangedFilesSummary::attributed(
                "maintenance-turn",
                vec![FileChangeStat::new("unexpected.rs", 1, 0)],
                Vec::new(),
            )),
        ];

        assert_eq!(latest_changed_source_turn(&timeline), None);
    }
}

#[cfg(test)]
mod identity_tests {
    use super::*;

    #[test]
    fn stale_backend_identity_events_are_retained() {
        let agent_id = Uuid::new_v4();
        let mut state = AgentChatState::default();

        assert!(state.apply_stale_backend_event(
            agent_id,
            &ChatBackendEvent::ChatSessionReady {
                session_id: "codex-thread-123".to_string(),
            },
            10,
        ));
        assert!(state.apply_stale_backend_event(
            agent_id,
            &ChatBackendEvent::SessionReady {
                session_id: "claude-session-456".to_string(),
            },
            11,
        ));

        let session = state
            .session(agent_id)
            .expect("identity event creates session");
        assert_eq!(session.chat_session_id.as_deref(), Some("codex-thread-123"));
        assert_eq!(
            session.cli_session_id.as_deref(),
            Some("claude-session-456")
        );
        assert_eq!(session.last_activity_at, 11);
    }

    #[test]
    fn stale_backend_non_identity_events_are_ignored() {
        let agent_id = Uuid::new_v4();
        let mut state = AgentChatState::default();

        assert!(!state.apply_stale_backend_event(
            agent_id,
            &ChatBackendEvent::Status(AgentChatStatus::Running),
            10,
        ));
        assert!(state.session(agent_id).is_none());
    }

    #[test]
    fn placeholder_identity_is_not_persisted() {
        let agent_id = Uuid::new_v4();
        let mut state = AgentChatState::default();

        assert!(!state.apply_stale_backend_event(
            agent_id,
            &ChatBackendEvent::SessionReady {
                session_id: agent_id.to_string(),
            },
            10,
        ));
        assert!(state.session(agent_id).is_none());
    }
}
