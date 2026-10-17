//! Composer Visual answers: per-conversation state, bounded publication
//! refresh, renderer IPC, Preview arbitration and native submission.
//!
//! The agent publishes with the `visual_present` MCP tool; the document is
//! stored in LocalStore. Choro detects publication with a cheap stamp query
//! when the conversation changes or a turn finishes, never by polling history.
use std::sync::Arc;

use ide_core::visual_answers::{
    VisualActionKind, VisualAnswer, VisualAnswerHead, VisualInteraction, VisualSubmissionStatus,
    MAX_VISUAL_STATE_BYTES,
};

pub(super) use super::agent_chat_visual_answer_flow::SendTarget;
pub(super) use super::agent_chat_visual_answer_placement::{visual_slot, VisualPlacement, VisualSlot};
use super::agent_chat_visual_answer_flow::{
    NoteTimers, OutgoingSurface, OutgoingSurfaces, REVISION_CHANGED,
};
use super::visual_answer_surface::{
    self as answer_surface, VisualAnswerMessage, VisualAnswerMode, VisualAnswerSurface,
    VisualRendererState,
};
use super::*;

/// While a conversation streams, check for publications at most this often.
const REFRESH_THROTTLE: Duration = Duration::from_millis(900);
/// Older answers keep their timeline card (with a stored title) but their
/// documents are only loaded when the user opens them.
const MAX_PRELOADED_ANSWERS: usize = 12;
pub(super) const CLOSE_PREVIEW_NOTICE: &str = "Close Preview to open this visual answer.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum VisualPanelMode {
    #[default]
    Collapsed,
    Docked,
    Expanded,
}

pub(super) enum VisualJob {
    Persist(Uuid),
    /// Bound to the revision and surface the user saw when sending.
    Submit {
        target: SendTarget,
        action_id: String,
        draft: String,
        display: String,
        window: Option<gpui::AnyWindowHandle>,
    },
    Finish {
        answer_id: Uuid,
        revision: u64,
        submission_id: Uuid,
        accepted: bool,
    },
    /// User-confirmed recovery of an uncertain send: clear it, then send again.
    Resend {
        answer_id: Uuid,
        revision: u64,
        submission_id: Uuid,
        action_id: String,
        window: gpui::AnyWindowHandle,
    },
}

#[derive(Default)]
pub(super) struct VisualAnswerChat {
    pub heads: Vec<VisualAnswerHead>,
    pub answers: HashMap<Uuid, VisualAnswer>,
    pub current: Option<Uuid>,
    pub mode: VisualPanelMode,
    /// Host generation of the open surface; renderer IPC must echo it.
    pub surface: u64,
    /// Shown beside the answer the user tried to open (e.g. while Preview is open).
    pub notice: Option<(Uuid, SharedString)>,
    pub render_errors: Vec<String>,
    pub error: Option<SharedString>,
    /// A send started in this app session (snapshot wait, reservation or
    /// dispatch), bound to its revision. Not an uncertain restored receipt.
    pub(super) sending: Option<SendTarget>,
    /// Bumped whenever local client state changes, so a rebuilt surface
    /// bootstraps from it rather than from an earlier snapshot.
    pub state_version: u64,
    /// A send waiting for the renderer's latest snapshot.
    pub(super) pending_submit: Option<super::agent_chat_visual_answer_submit::PendingVisualSubmit>,
    /// Shown when the composer note can't be kept with the answer.
    pub note_error: Option<SharedString>,
    pub(super) note_timers: NoteTimers,
    pub(super) note_watched: bool,
    /// Replaced surfaces in this chat whose final snapshot may still arrive.
    pub(super) outgoing: OutgoingSurfaces,
    /// Answer whose saved note should return to an empty composer.
    pub(super) restore_note: Option<Uuid>,
    /// Measured height of this conversation's transcript list. An inline
    /// result must fit inside it to be shown.
    pub(super) transcript_height: Option<f32>,
    /// Content height the renderer reported for (answer, revision) while
    /// docked or inline; the panel is fitted to it instead of a fixed slot.
    pub(super) content_height: Option<(Uuid, u64, f32)>,
    stamp: Option<(u64, i64)>,
    initialized: bool,
    refreshing: bool,
    refresh_again: bool,
    last_refresh: Option<Instant>,
    pub(super) jobs: VecDeque<VisualJob>,
    pub(super) working: bool,
    pub(super) dirty: HashSet<Uuid>,
}

impl VisualAnswerChat {
    pub fn current_answer(&self) -> Option<&VisualAnswer> {
        self.answers.get(&self.current?)
    }

    pub fn is_open(&self) -> bool {
        self.mode != VisualPanelMode::Collapsed && self.current_answer().is_some()
    }

    /// Where the open answer's content is shown, if it is open.
    pub fn slot(&self) -> Option<VisualSlot> {
        let answer = self.current_answer()?;
        visual_slot(VisualPlacement::of(&answer.document), self.mode)
    }

    /// The open answer waits for the user's choice beside the composer, so
    /// composer notes and Enter belong to it rather than to ordinary chat.
    pub fn takes_composer(&self) -> bool {
        self.is_open()
            && self
                .current_answer()
                .is_some_and(|answer| VisualPlacement::of(&answer.document) == VisualPlacement::Composer)
    }

    /// Any send in flight for this chat: composer submits are blocked.
    pub fn send_in_flight(&self) -> bool {
        self.sending.is_some() || self.pending_submit.is_some()
    }

    /// The shown answer revision is the one being sent.
    pub fn sending_current(&self) -> bool {
        self.sending
            .is_some_and(|target| target.matches(self.current_answer()))
    }

    /// The current answer changed (new revision or another answer) while a
    /// send waited for its snapshot: cancel it visibly; nothing was sent.
    /// A send already queued is cancelled when it runs (`SendTarget::matches`).
    pub fn cancel_stale_send(&mut self) {
        let current = self.current.and_then(|id| self.answers.get(&id));
        if self
            .pending_submit
            .as_ref()
            .is_some_and(|pending| !pending.target.matches(current))
        {
            self.pending_submit = None;
            self.sending = None;
            self.error = Some(REVISION_CHANGED.into());
        }
    }

    /// Remember the shown surface before another replaces it in this chat.
    pub fn retire_current_surface(&mut self) {
        if let Some(answer) = self.current_answer() {
            let outgoing = OutgoingSurface {
                answer_id: answer.id,
                revision: answer.revision,
                surface: self.surface,
            };
            self.outgoing.register(outgoing);
        }
    }

    /// A restored `sending` record whose outcome this session cannot know.
    pub fn uncertain_submission(&self) -> bool {
        !self.sending_current()
            && self.current_answer().is_some_and(|answer| {
                answer
                    .interaction
                    .submission
                    .as_ref()
                    .is_some_and(|s| s.status == VisualSubmissionStatus::Sending)
            })
    }
}

/// Newer revisions always win; at the same revision the higher interaction
/// revision wins. A stale read or late IPC can never replace newer state.
pub(super) fn newer_snapshot(cached: Option<&VisualAnswer>, loaded: &VisualAnswer) -> bool {
    cached.is_none_or(|cached| {
        loaded.revision > cached.revision
            || (loaded.revision == cached.revision
                && loaded.interaction.revision > cached.interaction.revision)
    })
}

/// The answer to bring forward after a refresh: a new answer or one whose
/// latest revision increased (any position, not only the newest head). When
/// several changed, the most recently created wins.
pub(super) fn published_since(
    previous: &[VisualAnswerHead],
    latest: &[VisualAnswerHead],
) -> Option<Uuid> {
    latest
        .iter()
        .filter(|head| {
            previous
                .iter()
                .find(|old| old.id == head.id)
                .is_none_or(|old| head.revision > old.revision)
        })
        // Heads are in publication order (latest publication last), so the
        // last changed head is the most recent `visual_present` call.
        .last()
        .map(|head| head.id)
}

/// Combine the store's latest record with local client edits made while a
/// write was in flight. A recorded decision (receipt) is never changed locally.
pub(super) fn merge_client_state(
    saved: &VisualInteraction,
    local: &VisualInteraction,
) -> VisualInteraction {
    let decided = saved.submission.is_some();
    VisualInteraction {
        selected_option_id: if decided {
            saved.selected_option_id.clone()
        } else {
            local.selected_option_id.clone()
        },
        draft: if decided {
            saved.draft.clone()
        } else {
            local.draft.clone()
        },
        fields: local.fields.clone(),
        scroll_top: local.scroll_top,
        collapsed: local.collapsed,
        detached: local.detached,
        ..saved.clone()
    }
}

/// Accepts renderer state only when it is valid for this revision.
pub(super) fn apply_renderer_state(
    answer: &VisualAnswer,
    state: VisualRendererState,
) -> Option<VisualInteraction> {
    if !state.fields.is_object()
        || !state.scroll_top.is_finite()
        || state.scroll_top < 0.0
        || serde_json::to_vec(&state.fields).ok()?.len() > MAX_VISUAL_STATE_BYTES - 4 * 1024
    {
        return None;
    }
    let decided = answer.interaction.submission.is_some();
    let selected = if decided {
        answer.interaction.selected_option_id.clone()
    } else {
        state
            .selected_option_id
            .filter(|id| answer.document.options.iter().any(|o| &o.id == id))
    };
    Some(VisualInteraction {
        selected_option_id: selected,
        fields: state.fields,
        scroll_top: state.scroll_top,
        ..answer.interaction.clone()
    })
}

/// What the user sees in their own transcript for a visual-answer send.
pub(super) fn submission_display(answer: &VisualAnswer, action_id: &str, notes: &str) -> String {
    let notes = notes.trim();
    let action = answer.document.actions.iter().find(|a| a.id == action_id);
    let lead = match action.map(|a| a.kind) {
        Some(VisualActionKind::Select) => answer
            .interaction
            .selected_option_id
            .as_ref()
            .and_then(|id| answer.document.options.iter().find(|o| &o.id == id))
            .map(|option| format!("Use this style: {}", option.title)),
        Some(VisualActionKind::FollowUp) => action.map(|a| a.label.clone()),
        _ => None,
    };
    match (lead, notes.is_empty()) {
        (Some(lead), true) => lead,
        (Some(lead), false) => format!("{lead}\n\n{notes}"),
        (None, _) => notes.to_string(),
    }
}

pub(super) fn kind_label(answer: &VisualAnswer) -> &'static str {
    use ide_core::visual_answers::VisualKind;
    match answer.document.kind {
        VisualKind::Table => "Table",
        VisualKind::LineChart => "Line chart",
        VisualKind::BarChart => "Bar chart",
        VisualKind::Summary => "Summary",
        VisualKind::DesignOptions => "Design options",
    }
}

struct RefreshResult {
    stamp: (u64, i64),
    heads: Vec<VisualAnswerHead>,
    loaded: Vec<VisualAnswer>,
}

impl CenterArea {
    pub(super) fn visual_store() -> anyhow::Result<ide_core::local_store::LocalStore> {
        ide_core::local_store::LocalStore::open_default()
    }

    pub(super) fn visual_answer_chat(&self, agent_id: Uuid) -> Option<&VisualAnswerChat> {
        self.visual_answers.get(&agent_id)
    }

    /// First render of a conversation loads its answers once.
    pub(super) fn ensure_visual_answers(&mut self, agent: &AgentRecord, cx: &mut Context<Self>) {
        if !ide_core::visual_answers::VISUAL_ANSWERS_READY {
            return;
        }
        if !self.visual_answers.contains_key(&agent.id) {
            self.visual_answers
                .insert(agent.id, VisualAnswerChat::default());
            self.refresh_visual_answers(agent.id, true, cx);
        }
        self.watch_visual_note(agent.id, cx);
    }

    /// Event-driven refresh: called on conversation changes (throttled) and
    /// at turn end (forced). Only conversations already shown are tracked.
    pub(super) fn refresh_visual_answers(
        &mut self,
        agent_id: Uuid,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(project_id) = self.agents.read(cx).agent(agent_id).map(|a| a.project_id.0) else {
            return;
        };
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry.refreshing {
            entry.refresh_again |= force;
            return;
        }
        if !force
            && entry
                .last_refresh
                .is_some_and(|at| at.elapsed() < REFRESH_THROTTLE)
        {
            return;
        }
        entry.refreshing = true;
        entry.last_refresh = Some(Instant::now());
        let known_stamp = entry.stamp;
        let cached: HashMap<Uuid, u64> = entry
            .answers
            .iter()
            .map(|(id, a)| (*id, a.revision))
            .collect();
        let current = entry.current;
        let previous = entry.heads.clone();
        let initialized = entry.initialized;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = Self::visual_store()?;
                    let stamp = store.visual_answer_stamp(project_id, agent_id)?;
                    if Some(stamp) == known_stamp {
                        return Ok(None);
                    }
                    let heads = store.visual_answer_heads(project_id, agent_id)?;
                    let recent = heads.len().saturating_sub(MAX_PRELOADED_ANSWERS);
                    // Whatever this refresh will bring forward must be loaded,
                    // even an older answer outside the preload window.
                    let published = initialized
                        .then(|| published_since(&previous, &heads))
                        .flatten();
                    let mut loaded = Vec::new();
                    for (index, head) in heads.iter().enumerate() {
                        let wanted = index >= recent
                            || Some(head.id) == current
                            || Some(head.id) == published;
                        if wanted && cached.get(&head.id) != Some(&head.revision) {
                            loaded.push(
                                store.read_visual_answer(project_id, agent_id, head.id, None)?,
                            );
                        }
                    }
                    Ok::<_, anyhow::Error>(Some(RefreshResult {
                        stamp,
                        heads,
                        loaded,
                    }))
                })
                .await;
            this.update(cx, |this, cx| {
                let again = this.visual_answers.get_mut(&agent_id).map(|entry| {
                    entry.refreshing = false;
                    std::mem::take(&mut entry.refresh_again)
                });
                match result {
                    Ok(Some(result)) => this.apply_visual_refresh(agent_id, result, cx),
                    Ok(None) => {}
                    Err(error) => eprintln!("visual answer refresh failed: {error:#}"),
                }
                if again == Some(true) {
                    this.refresh_visual_answers(agent_id, true, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_visual_refresh(
        &mut self,
        agent_id: Uuid,
        result: RefreshResult,
        cx: &mut Context<Self>,
    ) {
        let preview_blocks = self.visual_answer_preview_blocks(agent_id, cx);
        let next_surface = self.next_visual_surface();
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let published = published_since(&entry.heads, &result.heads).filter(|id| {
            result.loaded.iter().any(|a| a.id == *id) || entry.answers.contains_key(id)
        });
        if entry.initialized && published.is_some() {
            // Before the cache moves to the new revision: the outgoing
            // surface's final snapshot still belongs to the shown one.
            entry.retire_current_surface();
        }
        for answer in result.loaded {
            if newer_snapshot(entry.answers.get(&answer.id), &answer) {
                entry.answers.insert(answer.id, answer);
            }
        }
        entry.stamp = Some(result.stamp);
        entry.heads = result.heads;
        let mut persist_collapse = None;
        if !entry.initialized {
            entry.initialized = true;
            entry.current = entry.heads.last().map(|head| head.id);
            // Restart restores an open answer docked (never expanded).
            entry.mode = match entry.current_answer() {
                Some(answer) if !answer.interaction.collapsed => VisualPanelMode::Docked,
                _ => VisualPanelMode::Collapsed,
            };
            if entry.mode == VisualPanelMode::Docked {
                entry.restore_note = entry.current;
            }
            entry.surface = next_surface;
        } else if let Some(id) = published {
            // An explicit `visual_present` publication or revision (including
            // a revision of an older answer) opens: a result in its transcript
            // card, a choice beside the composer. Preview keeps the shared
            // surface; the answer then waits collapsed.
            entry.current = Some(id);
            entry.render_errors.clear();
            entry.error = None;
            entry.surface = next_surface;
            entry.mode = VisualPanelMode::Docked;
            entry.cancel_stale_send();
        }
        if preview_blocks && entry.mode != VisualPanelMode::Collapsed {
            entry.mode = VisualPanelMode::Collapsed;
            persist_collapse = entry.current;
        }
        let cards: Vec<(Uuid, String, u64)> = entry
            .heads
            .iter()
            .map(|head| {
                let title = entry
                    .answers
                    .get(&head.id)
                    .map(|a| a.document.title.clone())
                    .unwrap_or_else(|| "Visual answer".into());
                (head.id, title, head.created_at)
            })
            .collect();
        if let Some(id) = persist_collapse {
            self.set_visual_collapsed(agent_id, id, true, cx);
        }
        self.ensure_visual_answer_cards(agent_id, cards, cx);
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    fn next_visual_surface(&mut self) -> u64 {
        self.visual_answer_surface_seq += 1;
        self.visual_answer_surface_seq
    }

    /// Every publication gets one transcript card, persisted with the chat.
    fn ensure_visual_answer_cards(
        &mut self,
        agent_id: Uuid,
        cards: Vec<(Uuid, String, u64)>,
        cx: &mut Context<Self>,
    ) {
        let title = self
            .agents
            .read(cx)
            .agent(agent_id)
            .map(|a| a.title.clone())
            .unwrap_or_default();
        self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(agent_id, title, cx);
            let mut added = Vec::new();
            for (answer_id, title, created_at) in cards {
                let exists = session.timeline.iter().any(|item| {
                    matches!(item, AgentChatTimelineItem::VisualAnswer { answer_id: id, .. } if *id == answer_id)
                });
                if !exists {
                    let item = AgentChatTimelineItem::VisualAnswer { answer_id, title, created_at };
                    session.timeline.push(item.clone());
                    added.push(item);
                }
            }
            if added.is_empty() {
                return;
            }
            for item in added {
                crate::state::agent_chat::persist_timeline_item(agent_id, item, cx);
            }
            chats.publish_change(agent_id, crate::state::agent_chat::ChatChangeCategories::CONVERSATION, cx);
        });
    }

    /// Preview owns the shared surface while it is open for this project.
    pub(super) fn visual_answer_preview_blocks(&self, agent_id: Uuid, cx: &App) -> bool {
        self.agents
            .read(cx)
            .agent(agent_id)
            .is_some_and(|agent| self.is_project_preview_open(agent.project_id))
    }

    /// Native approvals, structured questions and plan decisions take
    /// precedence; the answer keeps its state and returns afterwards.
    pub(super) fn visual_answer_suppressed(&self, agent_id: Uuid, cx: &App) -> bool {
        self.agent_chats
            .read(cx)
            .session(agent_id)
            .is_some_and(|session| {
                session.pending_approval.is_some()
                    || session.pending_user_input.is_some()
                    || session
                        .proposed_plan
                        .as_ref()
                        .is_some_and(|plan| plan.implemented_at.is_none())
            })
    }

    /// The open answer of this conversation, if any is shown. Native decisions
    /// replace a composer or expanded panel; a result in its transcript card
    /// stays readable beside them.
    pub(super) fn visual_answer_displayed(
        &self,
        agent_id: Uuid,
        cx: &App,
    ) -> Option<&VisualAnswerChat> {
        self.visual_answers
            .get(&agent_id)
            .filter(|entry| entry.is_open())
            .filter(|entry| {
                entry.slot() == Some(VisualSlot::TranscriptCard)
                    || !self.visual_answer_suppressed(agent_id, cx)
            })
    }

    pub(super) fn visual_answer_slot(&self, agent_id: Uuid, cx: &App) -> Option<VisualSlot> {
        self.visual_answer_displayed(agent_id, cx)?.slot()
    }

    pub(super) fn visual_answer_expanded(&self, agent_id: Uuid, cx: &App) -> bool {
        self.visual_answer_slot(agent_id, cx) == Some(VisualSlot::Expanded)
    }

    /// The open answer is a choice waiting beside the composer (docked or
    /// expanded). Only then do composer notes and Enter belong to it.
    pub(super) fn visual_answer_takes_composer(&self, agent_id: Uuid, cx: &App) -> bool {
        self.visual_answer_displayed(agent_id, cx)
            .is_some_and(VisualAnswerChat::takes_composer)
    }

    /// Intent for the shared web host. Called after Preview has been given
    /// precedence, so this never displaces a live Preview.
    pub(super) fn visual_answer_intent(
        &mut self,
        agent: &AgentRecord,
        cx: &App,
    ) -> Option<web_preview::WebPreviewIntent> {
        let theme = answer_surface::theme_json(cx);
        let entry = self.visual_answer_displayed(agent.id, cx)?;
        let inline = entry.slot() == Some(VisualSlot::TranscriptCard);
        let answer = entry.current_answer()?;
        let mode = if entry.mode == VisualPanelMode::Expanded {
            VisualAnswerMode::Expanded
        } else {
            VisualAnswerMode::Docked
        };
        let (answer_id, revision, surface) = (answer.id, answer.revision, entry.surface);
        // A surface rebuilt later (after a dialog, chat switch or restart of
        // the view) must start from the latest local state, mode and theme.
        let key = format!(
            "{}:{answer_id}:{revision}:{surface}:{}:{mode:?}:{theme}",
            agent.id, entry.state_version
        );
        let bootstrap = match &self.visual_answer_bootstrap {
            Some((cached, bootstrap)) if *cached == key => bootstrap.clone(),
            _ => {
                let bootstrap: Arc<str> =
                    answer_surface::bootstrap_json(surface, answer, &theme, mode).into();
                self.visual_answer_bootstrap = Some((key, bootstrap.clone()));
                bootstrap
            }
        };
        Some(web_preview::WebPreviewIntent::VisualAnswer(
            VisualAnswerSurface {
                agent_id: agent.id,
                answer_id,
                revision,
                surface,
                bootstrap,
                mode,
                theme: theme.into(),
                inline,
            },
        ))
    }

    /// The transcript list's laid-out height. A change re-sizes an open inline
    /// result on the next frame; unchanged heights do nothing (no idle redraw).
    pub(super) fn record_transcript_height(
        &mut self,
        agent_id: Uuid,
        height: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry
            .transcript_height
            .is_some_and(|known| (known - height).abs() < 1.0)
        {
            return;
        }
        entry.transcript_height = Some(height);
        let inline = entry.slot() == Some(VisualSlot::TranscriptCard);
        if inline || self.owns_chat_visualization(agent_id) {
            self.remeasure_agent_chat_list(agent_id);
            cx.notify();
        }
    }

    /// Opening Preview while an answer is shown saves and collapses it first.
    pub(super) fn collapse_visual_answers_for_preview(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) {
        let open: Vec<(Uuid, Uuid, u64)> = self
            .visual_answers
            .iter()
            .filter(|(_, entry)| entry.mode != VisualPanelMode::Collapsed)
            .filter_map(|(agent_id, entry)| Some((*agent_id, entry.current?, entry.surface)))
            .filter(|(agent_id, ..)| {
                self.agents
                    .read(cx)
                    .agent(*agent_id)
                    .is_some_and(|a| a.project_id == project)
            })
            .collect();
        for (agent_id, answer_id, surface) in open {
            self.web_host
                .update(cx, |host, _| host.flush_visual_answer(surface));
            if let Some(entry) = self.visual_answers.get_mut(&agent_id) {
                entry.mode = VisualPanelMode::Collapsed;
            }
            self.set_visual_collapsed(agent_id, answer_id, true, cx);
        }
    }

    pub(super) fn open_visual_answer(
        &mut self,
        agent_id: Uuid,
        answer_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let preview_blocks = self.visual_answer_preview_blocks(agent_id, cx);
        let next_surface = self.next_visual_surface();
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if preview_blocks {
            // Never close, reload or hide the live page on the user's behalf.
            entry.notice = Some((answer_id, CLOSE_PREVIEW_NOTICE.into()));
            cx.notify();
            return;
        }
        entry.notice = None;
        entry.error = None;
        entry.render_errors.clear();
        let reopened = entry.current == Some(answer_id) && entry.mode != VisualPanelMode::Collapsed;
        if !reopened {
            if entry.mode != VisualPanelMode::Collapsed {
                // A → B in this chat: A's final snapshot arrives after the switch.
                entry.retire_current_surface();
            }
            entry.current = Some(answer_id);
            entry.mode = VisualPanelMode::Docked;
            entry.surface = next_surface;
            // Its saved note returns if the composer is empty.
            entry.restore_note = Some(answer_id);
            entry.cancel_stale_send();
        }
        let loaded = self
            .visual_answers
            .get(&agent_id)
            .is_some_and(|entry| entry.answers.contains_key(&answer_id));
        if loaded {
            self.set_visual_collapsed(agent_id, answer_id, false, cx);
        } else {
            self.load_visual_answer(agent_id, answer_id, cx);
        }
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    fn load_visual_answer(&mut self, agent_id: Uuid, answer_id: Uuid, cx: &mut Context<Self>) {
        let Some(project_id) = self.agents.read(cx).agent(agent_id).map(|a| a.project_id.0) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    Self::visual_store()?.read_visual_answer(project_id, agent_id, answer_id, None)
                })
                .await;
            this.update(cx, |this, cx| {
                match loaded {
                    Ok(answer) => {
                        if let Some(entry) = this.visual_answers.get_mut(&agent_id) {
                            if newer_snapshot(entry.answers.get(&answer_id), &answer) {
                                entry.answers.insert(answer_id, answer);
                            }
                        }
                        this.set_visual_collapsed(agent_id, answer_id, false, cx);
                    }
                    Err(error) => {
                        if let Some(entry) = this.visual_answers.get_mut(&agent_id) {
                            entry.error = Some(
                                format!("This visual answer could not be loaded: {error}").into(),
                            );
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn collapse_visual_answer(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let surface = entry.surface;
        entry.mode = VisualPanelMode::Collapsed;
        entry.notice = None;
        let current = entry.current;
        self.web_host
            .update(cx, |host, _| host.flush_visual_answer(surface));
        if let Some(answer_id) = current {
            self.set_visual_collapsed(agent_id, answer_id, true, cx);
        }
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    pub(super) fn set_visual_panel_expanded(
        &mut self,
        agent_id: Uuid,
        expanded: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry.mode == VisualPanelMode::Collapsed {
            return;
        }
        entry.mode = if expanded {
            VisualPanelMode::Expanded
        } else {
            VisualPanelMode::Docked
        };
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    /// Tab from the composer moves into the open answer's content.
    pub(super) fn focus_visual_answer_content(
        &mut self,
        agent_id: Uuid,
        first: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(surface) = self
            .visual_answer_displayed(agent_id, cx)
            .map(|entry| entry.surface)
        else {
            return false;
        };
        self.web_host
            .update(cx, |host, _| host.focus_visual_answer(surface, first))
    }

    /// Opening an answer also re-attaches it; collapsing keeps it as it was.
    fn set_visual_collapsed(
        &mut self,
        agent_id: Uuid,
        answer_id: Uuid,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        let detached = collapsed
            && self
                .visual_answers
                .get(&agent_id)
                .and_then(|entry| entry.answers.get(&answer_id))
                .is_some_and(|answer| answer.interaction.detached);
        self.set_visual_attachment(agent_id, answer_id, collapsed, detached, cx);
    }

    /// Persist where the user left this revision: shown, collapsed in the
    /// composer band, or detached to the transcript. One coalesced write.
    pub(super) fn set_visual_attachment(
        &mut self,
        agent_id: Uuid,
        answer_id: Uuid,
        collapsed: bool,
        detached: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let Some(answer) = entry.answers.get_mut(&answer_id) else {
            return;
        };
        if (answer.interaction.collapsed, answer.interaction.detached) != (collapsed, detached) {
            answer.interaction.collapsed = collapsed;
            answer.interaction.detached = detached;
            self.queue_visual_job(agent_id, VisualJob::Persist(answer_id), cx);
        }
    }

    /// Drain renderer IPC. Each message is checked against the conversation's
    /// current answer, revision and surface generation before it is applied.
    pub(super) fn process_visual_answer_messages(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let messages = self
            .web_host
            .update(cx, |host, _| host.take_visual_answer_messages());
        for envelope in messages {
            // A late message from an earlier surface or a superseded revision
            // (for example a final snapshot) never touches newer state.
            let current = self
                .visual_answers
                .get(&envelope.agent_id)
                .is_some_and(|entry| {
                    entry.current == Some(envelope.answer_id)
                        && entry.surface == envelope.surface
                        && entry
                            .current_answer()
                            .is_some_and(|answer| answer.revision == envelope.revision)
                });
            if !current {
                // The one exception: the final snapshot of a surface replaced
                // within this chat (A → B), applied only to its own answer at
                // its own revision, never to anything newer.
                let outgoing = self
                    .visual_answers
                    .get_mut(&envelope.agent_id)
                    .and_then(|entry| {
                        let cached = entry.answers.get(&envelope.answer_id).cloned();
                        entry
                            .outgoing
                            .take_final_snapshot(&envelope, cached.as_ref())
                    });
                if let (Some(answer_id), VisualAnswerMessage::Snapshot { state, .. }) =
                    (outgoing, envelope.message)
                {
                    self.apply_visual_state(envelope.agent_id, answer_id, state, cx);
                }
                continue;
            }
            match envelope.message {
                VisualAnswerMessage::Ready => {}
                VisualAnswerMessage::Invalid(errors) => {
                    if let Some(entry) = self.visual_answers.get_mut(&envelope.agent_id) {
                        entry.render_errors = errors;
                    }
                    cx.notify();
                }
                VisualAnswerMessage::State(state) => {
                    self.apply_visual_state(envelope.agent_id, envelope.answer_id, state, cx);
                }
                VisualAnswerMessage::Snapshot { token, state } => {
                    // A final snapshot (outgoing surface) or the one a send
                    // is waiting for; either way it is the newest state.
                    self.apply_visual_state(envelope.agent_id, envelope.answer_id, state, cx);
                    self.release_visual_submit(envelope.agent_id, token, cx);
                }
                VisualAnswerMessage::Escape => {
                    // WebKit owned the keyboard, so this is the press's only route.
                    self.composer_escape_from_outside_input(envelope.agent_id, window, cx);
                }
                VisualAnswerMessage::Size(height) => {
                    self.record_visual_content_height(envelope.agent_id, height, cx);
                }
                VisualAnswerMessage::FocusExit { .. } => {
                    // Both directions lead to the panel's native composer, the
                    // one text control that is always present beside the content.
                    self.focus_agent_composer(envelope.agent_id, window, cx);
                }
            }
        }
    }

    /// An outgoing answer surface holds the shared view until its snapshot
    /// arrives; redraw at the deadline so the next surface is built even if
    /// the renderer never replies.
    pub(super) fn schedule_visual_handoff_wake(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.web_host.read(cx).visual_handoff_deadline() else {
            return;
        };
        if self.visual_handoff_wake == Some(deadline) {
            return;
        }
        self.visual_handoff_wake = Some(deadline);
        cx.spawn(async move |this, cx| {
            let wait = deadline.saturating_duration_since(Instant::now());
            cx.background_executor().timer(wait).await;
            this.update(cx, |this, cx| {
                // Release the hidden outgoing surface at its deadline even if
                // nothing else will be placed (e.g. collapsed, no replacement).
                this.web_host
                    .update(cx, |host, _| host.expire_visual_handoff(Instant::now()));
                if this.visual_handoff_wake == Some(deadline) {
                    this.visual_handoff_wake = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::visual_answers::{VisualDocument, VisualSubmission, IMPLICIT_REFINE_ACTION};

    fn head(id: Uuid, revision: u64, created_at: u64) -> VisualAnswerHead {
        VisualAnswerHead {
            id,
            revision,
            created_at,
        }
    }

    #[test]
    fn a_revision_of_an_older_answer_is_brought_forward() {
        let (old, newer) = (Uuid::new_v4(), Uuid::new_v4());
        let before = [head(old, 1, 10), head(newer, 1, 20)];
        assert_eq!(published_since(&before, &before), None, "nothing changed");
        // The older answer is revised. Heads are in publication order, so it
        // moves last, but detection compares revisions, not position.
        let revised = [head(newer, 1, 20), head(old, 2, 10)];
        assert_eq!(published_since(&before, &revised), Some(old));
        let revised_in_place = [head(old, 2, 10), head(newer, 1, 20)];
        assert_eq!(published_since(&before, &revised_in_place), Some(old));
        let fresh = Uuid::new_v4();
        let added = [head(old, 1, 10), head(newer, 1, 20), head(fresh, 1, 30)];
        assert_eq!(published_since(&before, &added), Some(fresh));
        // Two publications between refreshes: the latest publication wins,
        // even when it revises the older answer.
        let both = [head(newer, 1, 20), head(fresh, 1, 30), head(old, 2, 10)];
        assert_eq!(published_since(&before, &both), Some(old));
    }

    fn answer() -> VisualAnswer {
        let document: VisualDocument = serde_json::from_value(serde_json::json!({
            "title":"Directions", "summary":"Two sample styles", "kind":"design_options",
            "openui_lang":"root = Answer([choices])\nchoices = DesignOptions()",
            "options":[{"id":"solid","title":"Solid","description":"","html":"<b>a</b>"},
                       {"id":"tonal","title":"Tonal","description":"","html":"<b>b</b>"}],
            "actions":[{"id":"use","label":"Use this style","kind":"select"},
                       {"id":"more","label":"Show darker versions","kind":"follow_up"}]
        }))
        .unwrap();
        VisualAnswer {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            revision: 2,
            document,
            interaction: VisualInteraction {
                revision: 5,
                ..Default::default()
            },
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn stale_reads_never_replace_newer_answer_state() {
        let cached = answer();
        let mut stale = cached.clone();
        stale.interaction.revision = 4;
        assert!(!newer_snapshot(Some(&cached), &stale));
        let mut same = cached.clone();
        same.interaction.revision = 5;
        assert!(
            !newer_snapshot(Some(&cached), &same),
            "local unsaved edits win ties"
        );
        let mut revised = cached.clone();
        revised.revision = 3;
        revised.interaction.revision = 0;
        assert!(newer_snapshot(Some(&cached), &revised));
        assert!(newer_snapshot(None, &stale));
    }

    #[test]
    fn renderer_state_cannot_select_unknown_options_or_change_a_decision() {
        let mut answer = answer();
        let state = |id: &str| VisualRendererState {
            selected_option_id: Some(id.into()),
            fields: serde_json::json!({"view":{}}),
            scroll_top: 10.0,
        };
        assert_eq!(
            apply_renderer_state(&answer, state("tonal"))
                .unwrap()
                .selected_option_id
                .as_deref(),
            Some("tonal")
        );
        assert_eq!(
            apply_renderer_state(&answer, state("invented"))
                .unwrap()
                .selected_option_id,
            None
        );
        let mut bad = state("tonal");
        bad.scroll_top = f64::NAN;
        assert!(apply_renderer_state(&answer, bad).is_none());
        let mut huge = state("tonal");
        huge.fields = serde_json::json!({"x": "y".repeat(MAX_VISUAL_STATE_BYTES)});
        assert!(apply_renderer_state(&answer, huge).is_none());
        answer.interaction.selected_option_id = Some("solid".into());
        answer.interaction.submission = Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: "use".into(),
            text: "Use the Solid direction.".into(),
            status: VisualSubmissionStatus::Sent,
        });
        let after = apply_renderer_state(&answer, state("tonal")).unwrap();
        assert_eq!(after.selected_option_id.as_deref(), Some("solid"));
        assert!(after.submission.is_some(), "the receipt is kept");
        assert_eq!(
            after.scroll_top, 10.0,
            "a submitted answer stays inspectable"
        );
    }

    #[test]
    fn merging_keeps_local_view_but_never_a_local_decision_change() {
        let mut saved = answer().interaction;
        saved.revision = 9;
        let local = VisualInteraction {
            selected_option_id: Some("tonal".into()),
            draft: "rounder".into(),
            fields: serde_json::json!({"view":{"s:t":{"search":"a"}}}),
            scroll_top: 3.0,
            collapsed: true,
            ..Default::default()
        };
        let merged = merge_client_state(&saved, &local);
        assert_eq!(merged.revision, 9);
        assert_eq!(merged.selected_option_id.as_deref(), Some("tonal"));
        assert!(merged.collapsed);
        saved.submission = Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: "use".into(),
            text: "sent".into(),
            status: VisualSubmissionStatus::Sending,
        });
        let merged = merge_client_state(&saved, &local);
        assert_eq!(merged.selected_option_id, None);
        assert_eq!(merged.draft, "");
        assert_eq!(merged.fields, local.fields);
    }

    #[test]
    fn transcript_receipts_read_as_the_users_decision() {
        let mut answer = answer();
        answer.interaction.selected_option_id = Some("tonal".into());
        assert_eq!(
            submission_display(&answer, "use", "  "),
            "Use this style: Tonal"
        );
        assert_eq!(
            submission_display(&answer, "use", "Rounder"),
            "Use this style: Tonal\n\nRounder"
        );
        assert_eq!(
            submission_display(&answer, "more", ""),
            "Show darker versions"
        );
        assert_eq!(
            submission_display(&answer, IMPLICIT_REFINE_ACTION, "Quieter"),
            "Quieter"
        );
    }
}
