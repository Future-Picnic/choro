#![allow(
    dead_code,
    reason = "retained agent-chat state for planned interaction paths"
)]

mod changed_files;
mod code_review;
mod interactions;
mod pending_approval;
mod pending_user_input;
mod persistence;
mod proposed_plan;
pub(crate) mod protocol;
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

pub(crate) use changed_files::VisualizationArtifactFilter;
pub use changed_files::{ChangedFilesSummary, FileChangeStat};
pub use code_review::{split_code_review, CodeReview, CodeReviewFinding, CodeReviewSeverity};
pub use pending_approval::{PendingApproval, PendingApprovalKind};
pub use pending_user_input::{PendingUserInput, PendingUserInputOption, PendingUserInputQuestion};
pub use proposed_plan::{split_proposed_plan, ProposedPlan};
use protocol::{spawn_chat_backend, ChatBackendCommand, ChatBackendController, ChatBackendEvent};
pub use usage::{ConversationUsage, ModelUsage, UsageTotals};
pub use verification::{split_verification, Verification, VerificationItem, VerificationStatus};
pub use work_log::{WorkLogEntry, WorkLogEntryKind, WorkLogStatus};

use interactions::*;
use persistence::*;
pub use persistence::{persist_timeline_snapshot, timeline_item_from_store_event};
use timeline::*;

pub enum AgentChatEvent {
    Changed,
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
    pub(crate) sessions: HashMap<Uuid, AgentChatSession>,
    controllers: HashMap<Uuid, ChatBackendController>,
    backend_generations: HashMap<Uuid, u64>,
    cancellation_requested: HashSet<Uuid>,
}

#[derive(Clone, Debug)]
pub struct AgentChatSession {
    pub agent_id: Uuid,
    pub title: String,
    pub chat_session_id: Option<String>,
    pub cli_session_id: Option<String>,
    pub hidden_from_notifications: bool,
    pub status: AgentChatStatus,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueuedChatTurn {
    pub id: Uuid,
    pub text: String,
    pub display_text: Option<String>,
    pub tags: Vec<AgentChatMessageTag>,
    pub mode: AgentInteractionMode,
    pub created_at: u64,
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
    Riff,
    Skill,
    Command,
    File,
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
    Message(AgentChatMessage),
    WorkLog(WorkLogEntry),
    PendingUserInput(PendingUserInput),
    ProposedPlan(ProposedPlan),
    CodeReview(CodeReview),
    Verification(Verification),
    ChangedFiles(ChangedFilesSummary),
    ShipResult(ShipResult),
    Rejoined(RejoinedCard),
    RejoinConflict(RejoinConflictCard),
    Memorized(MemorizedCard),
    MemoryProposal(MemoryProposalCard),
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
    Message {
        role: String,
        text: String,
        #[serde(default)]
        display_text: Option<String>,
        #[serde(default)]
        tags: Vec<AgentChatMessageTag>,
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
    ChangedFiles {
        files: Vec<StoredFileChange>,
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

#[derive(Debug, Serialize, Deserialize)]
struct StoredFileChange {
    path: String,
    additions: usize,
    deletions: usize,
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
        let session = self
            .sessions
            .entry(agent_id)
            .or_insert_with(|| AgentChatSession {
                agent_id,
                title: title.into(),
                chat_session_id: None,
                cli_session_id: None,
                hidden_from_notifications: false,
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
        cx.notify();
        session
    }

    pub fn set_interaction_mode(
        &mut self,
        agent_id: Uuid,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.interaction_mode = mode;
            cx.emit(AgentChatEvent::Changed);
            cx.notify();
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
        let (controller, event_rx) = spawn_chat_backend(agent.clone(), initial_mode)?;
        self.controllers.insert(agent.id, controller);
        let agent_id = agent.id;
        // Await the channel instead of polling on a timer: an idle chat costs
        // zero wake-ups, and the task ends when the backend's senders drop.
        cx.spawn(async move |this, cx| loop {
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
        })
        .detach();
        Ok(())
    }

    pub fn send_turn(
        &mut self,
        agent_id: Uuid,
        text: String,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) {
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::SendTurn { text, mode });
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
            session.status = AgentChatStatus::Running;
            session.pending_approval = None;
            session.started_running_at = Some(unix_now());
            session.last_activity_at = unix_now();
        }
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn update_model_effort(
        &mut self,
        agent_id: Uuid,
        model: AgentModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        if let Some(controller) = self.controllers.get(&agent_id) {
            let _ = controller.send(ChatBackendCommand::UpdateModelEffort { model, effort });
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
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn update_title(&mut self, agent_id: Uuid, title: String, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        if session.title == title {
            return;
        }
        session.title = title;
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
        let id = Uuid::new_v4();
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.queued_turns.push(QueuedChatTurn {
                id,
                text,
                display_text,
                tags,
                mode,
                created_at: unix_now(),
            });
        }
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
        id
    }

    pub fn remove_queued_turn(&mut self, agent_id: Uuid, turn_id: Uuid, cx: &mut Context<Self>) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.queued_turns.retain(|turn| turn.id != turn_id);
        }
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn steer_queued_turn(&mut self, agent_id: Uuid, turn_id: Uuid, cx: &mut Context<Self>) {
        let Some(turn) = self.take_queued_turn(agent_id, turn_id) else {
            return;
        };
        self.start_turn(
            agent_id,
            turn.text,
            turn.display_text,
            turn.tags,
            turn.mode,
            cx,
        );
    }

    fn drain_next_queued_turn(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(turn) = self.take_next_queued_turn(agent_id) else {
            return;
        };
        self.start_turn(
            agent_id,
            turn.text,
            turn.display_text,
            turn.tags,
            turn.mode,
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
                let still_idle = state
                    .sessions
                    .get(&agent_id)
                    .is_some_and(|session| session.status == AgentChatStatus::Idle);
                if still_idle {
                    state.drain_next_queued_turn(agent_id, cx);
                }
            })
            .ok();
        })
        .detach();
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
        cx: &mut Context<Self>,
    ) {
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
        self.send_turn(agent_id, text, mode, cx);
    }

    pub fn stop_backend(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        if !self.controllers.contains_key(&agent_id) {
            self.hard_stop_backend(agent_id, true, cx);
            return;
        }
        let force_stop = self.cancellation_requested.contains(&agent_id)
            || self
                .sessions
                .get(&agent_id)
                .is_some_and(|session| session.status == AgentChatStatus::Cancelling);
        if force_stop {
            self.hard_stop_backend(agent_id, true, cx);
            return;
        }
        if let Some(controller) = self.controllers.get(&agent_id) {
            controller.cancel_turn();
        }
        self.cancellation_requested.insert(agent_id);
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.status = AgentChatStatus::Cancelling;
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
                "Click stop again to force-kill the backend process.".to_string(),
            ));
            upsert_work_log_entry(&mut session.work_log, entry.clone());
            upsert_timeline_work_log(&mut session.timeline, entry.clone());
            persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            session.last_activity_at = unix_now();
        }
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
        if !session_safe_to_retire(session) {
            return false;
        }
        // Bump the generation so trailing events from the dying backend cannot
        // flip the session's status or append late output.
        self.next_backend_generation(agent_id);
        // Dropping the controller sends Shutdown; the backend thread exits its
        // run loop and its Drop impl terminates the whole process group.
        self.controllers.remove(&agent_id);
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
        true
    }

    pub fn reset_session(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.hard_stop_backend(agent_id, true, cx);
        self.sessions.remove(&agent_id);
        self.cancellation_requested.remove(&agent_id);
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn force_stop_backend(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.hard_stop_backend(agent_id, true, cx);
    }

    /// Stop a backend because its Solo lane is about to be merged or removed.
    /// This is a lifecycle transition, not a user pressing Stop, so it must not
    /// leave a misleading "Stopped by user" row in the conversation.
    pub fn stop_backend_for_lane_exit(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.hard_stop_backend(agent_id, false, cx);
    }

    /// Stop every app-owned backend process before the application exits.
    pub fn shutdown_all(&mut self, cx: &mut Context<Self>) {
        for (_, controller) in self.controllers.drain() {
            controller.force_shutdown();
        }
        self.cancellation_requested.clear();
        for session in self.sessions.values_mut() {
            session.status = AgentChatStatus::Idle;
            session.started_running_at = None;
            session.pending_user_input = None;
            session.pending_approval = None;
        }
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    fn hard_stop_backend(
        &mut self,
        agent_id: Uuid,
        record_user_stop: bool,
        cx: &mut Context<Self>,
    ) {
        self.next_backend_generation(agent_id);
        if let Some(controller) = self.controllers.remove(&agent_id) {
            controller.force_shutdown();
        }
        self.cancellation_requested.remove(&agent_id);
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            if let Some(entry) = settle_hard_stopped_session(session, record_user_stop) {
                persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            }
        }
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
        session.status = AgentChatStatus::Running;
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
        session.status = AgentChatStatus::Idle;
        session.started_running_at = None;
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
        session.status = AgentChatStatus::Running;
        session.started_running_at = Some(unix_now());
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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
            cx.emit(AgentChatEvent::Changed);
            cx.notify();
        }
    }

    fn apply_backend_event(
        &mut self,
        agent_id: Uuid,
        generation: u64,
        event: ChatBackendEvent,
        cx: &mut Context<Self>,
    ) {
        let now = unix_now();
        if self.backend_generations.get(&agent_id).copied() != Some(generation) {
            if self.apply_stale_backend_event(agent_id, &event, now) {
                cx.emit(AgentChatEvent::Changed);
                cx.notify();
            }
            return;
        }
        // A failed runtime cannot receive another turn. Remove only the
        // controller for the current generation so the next submission starts
        // a fresh backend and resumes from the persisted provider session id.
        if matches!(&event, ChatBackendEvent::Error(_)) {
            self.controllers.remove(&agent_id);
        }
        if self.cancellation_requested.contains(&agent_id) {
            let should_drain_queue = match event {
                ChatBackendEvent::Status(AgentChatStatus::Idle | AgentChatStatus::Failed)
                | ChatBackendEvent::Error(_) => {
                    self.cancellation_requested.remove(&agent_id);
                    let session = self.ensure_backend_event_session(agent_id, now);
                    session.last_activity_at = now;
                    session.status = AgentChatStatus::Idle;
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
            cx.emit(AgentChatEvent::Changed);
            cx.notify();
            if should_drain_queue {
                self.schedule_next_queued_turn(agent_id, cx);
            }
            return;
        }
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
            ChatBackendEvent::PendingUserInput(pending) => {
                upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::PendingUserInput(pending.clone()),
                    cx,
                );
                session.pending_user_input = Some(pending);
                session.status = AgentChatStatus::WaitingForUser;
                session.started_running_at = None;
            }
            ChatBackendEvent::PendingApproval(pending) => {
                session.pending_approval = Some(pending);
                session.status = AgentChatStatus::WaitingForUser;
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
                session.status = AgentChatStatus::PlanReady;
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
            ChatBackendEvent::ChangedFiles(summary) => {
                session.changed_files = summary.clone();
                if !summary.files.is_empty() {
                    append_timeline_changed_files(&mut session.timeline, summary.clone());
                    persist_timeline_item(
                        agent_id,
                        AgentChatTimelineItem::ChangedFiles(summary),
                        cx,
                    );
                }
            }
            ChatBackendEvent::Usage(usage) => {
                apply_usage_snapshot(session, usage);
            }
            ChatBackendEvent::Status(status) => {
                if status == AgentChatStatus::Idle
                    && session
                        .proposed_plan
                        .as_ref()
                        .is_some_and(|plan| plan.implemented_at.is_none())
                {
                    session.status = AgentChatStatus::PlanReady;
                } else {
                    session.status = status;
                }
                if !matches!(session.status, AgentChatStatus::Running) {
                    session.started_running_at = None;
                } else if session.started_running_at.is_none() {
                    session.started_running_at = Some(now);
                }
            }
            ChatBackendEvent::Error(error) => {
                let error_message_id = format!("error-{}", session.messages.len());
                let message = AgentChatMessage::Assistant {
                    message_id: Some(error_message_id),
                    text: error,
                    created_at: now,
                };
                append_or_extend_message(&mut session.messages, message.clone());
                append_or_extend_timeline_message(&mut session.timeline, message.clone());
                persist_chat_message(agent_id, message, cx);
                session.status = AgentChatStatus::Failed;
                session.started_running_at = None;
                session.pending_user_input = None;
                session.pending_approval = None;
                session
                    .timeline
                    .retain(|item| !matches!(item, AgentChatTimelineItem::PendingUserInput(_)));
            }
        }
        let should_drain_queue =
            session.status == AgentChatStatus::Idle && !session.queued_turns.is_empty();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
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

fn is_real_cli_session_id(agent_id: Uuid, session_id: &str) -> bool {
    !session_id.trim().is_empty() && session_id != agent_id.to_string()
}

fn settle_hard_stopped_session(
    session: &mut AgentChatSession,
    record_user_stop: bool,
) -> Option<WorkLogEntry> {
    session.status = AgentChatStatus::Idle;
    session.started_running_at = None;
    session.queued_turns.clear();
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

    fn retirable_session() -> AgentChatSession {
        AgentChatSession {
            agent_id: Uuid::new_v4(),
            title: "Chat".to_string(),
            chat_session_id: None,
            cli_session_id: Some("claude-session-1".to_string()),
            hidden_from_notifications: false,
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
    fn idle_session_with_resume_id_is_retirable() {
        assert!(session_safe_to_retire(&retirable_session()));
        let mut codex = retirable_session();
        codex.cli_session_id = None;
        codex.chat_session_id = Some("codex-thread-1".to_string());
        assert!(session_safe_to_retire(&codex));
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
            session.status = status;
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
        session.status = AgentChatStatus::Running;

        let entry = settle_hard_stopped_session(&mut session, false);

        assert!(entry.is_none());
        assert_eq!(session.status, AgentChatStatus::Idle);
        assert!(session.timeline.is_empty());
        assert!(session.work_log.is_empty());
    }

    #[test]
    fn explicit_force_stop_keeps_the_user_stop_outcome() {
        let mut session = retirable_session();
        session.status = AgentChatStatus::Running;

        let entry = settle_hard_stopped_session(&mut session, true)
            .expect("an explicit stop should produce a timeline outcome");

        assert_eq!(entry.title, "Stopped by user");
        assert!(session.timeline.iter().any(|item| matches!(
            item,
            AgentChatTimelineItem::WorkLog(entry) if entry.title == "Stopped by user"
        )));
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
