//! Sending from a Visual answer, and the composer note that goes with it.
//!
//! The native composer text is the answer's note while the answer is open.
//! It is saved to the answer (debounced per answer) so it survives restarts
//! and new revisions, and restored into an empty composer, never over text.
//! A send is bound to the revision the user saw. It first takes the
//! renderer's current snapshot (bounded wait), then records the receipt, then
//! dispatches through the normal conversation path. While it is in flight,
//! further composer submits are blocked rather than sent without context.
use ide_core::visual_answers::VisualAnswer;

use super::agent_chat_visual_answer::{
    merge_client_state, submission_display, SendTarget, VisualJob, VisualPanelMode,
};
use super::agent_chat_visual_answer_flow::{
    composer_route, note_for_answer, note_to_restore, ComposerRoute, MAX_VISUAL_NOTE_BYTES,
    NOTE_TOO_LONG, REVISION_CHANGED,
};
use super::visual_answer_surface::SNAPSHOT_TIMEOUT;
use super::*;

const NOTE_SAVE_DELAY: Duration = Duration::from_millis(400);
const CHIPS_NOT_SENT: &str = "Remove attachments or mentions from the composer to send them with this visual answer, or send them as a normal message.";

/// A send waiting for the renderer's snapshot of the latest local state.
pub(super) struct PendingVisualSubmit {
    pub token: u64,
    pub target: SendTarget,
    pub action_id: String,
    pub note: String,
    pub window: Option<gpui::AnyWindowHandle>,
}

impl CenterArea {
    /// Enter and the send arrow share this path. With an open answer a plain
    /// note refines it; attachments, chips or no open answer send normally;
    /// while a visual send is in flight nothing is sent (no plain duplicate).
    pub(super) fn submit_agent_chat_composer(
        &mut self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        surface: &AgentChatSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let note = self.visual_composer_note(agent.id, cx);
        let in_flight = self
            .visual_answers
            .get(&agent.id)
            .is_some_and(|entry| entry.send_in_flight());
        let displayed = self
            .visual_answer_displayed(agent.id, cx)
            .and_then(|entry| entry.current_answer());
        match composer_route(displayed, in_flight, note.as_deref()) {
            ComposerRoute::Normal => {
                self.submit_agent_chat_message_for_surface(agent, input, false, surface, window, cx)
            }
            ComposerRoute::Visual { action_id } => {
                self.submit_visual_action(agent.id, action_id, window, cx);
            }
            ComposerRoute::Blocked(message) => {
                if let (Some(message), Some(entry)) =
                    (message, self.visual_answers.get_mut(&agent.id))
                {
                    entry.error = Some(message.into());
                    cx.notify();
                }
            }
        }
    }

    /// Plain composer text, or `None` when the draft has attachments or chips
    /// that only the normal send path understands.
    pub(super) fn visual_composer_note(&self, agent_id: Uuid, cx: &App) -> Option<String> {
        let plain = self
            .agent_chat_attached_files
            .get(&agent_id)
            .is_none_or(|files| files.is_empty())
            && self
                .agent_chat_pasted_text_blocks
                .get(&agent_id)
                .is_none_or(|blocks| blocks.is_empty())
            && self
                .agent_chat_attachment_pastes_pending
                .get(&agent_id)
                .is_none_or(|p| p.is_empty())
            && !self.agent_chat_selected_commands.contains_key(&agent_id)
            && self
                .agent_chat_selected_mentions
                .get(&agent_id)
                .is_none_or(|m| m.is_empty())
            && !self
                .agent_chat_selected_agent_targets
                .contains_key(&agent_id);
        if !plain {
            return None;
        }
        Some(
            self.agent_chat_inputs
                .get(&agent_id)
                .map(|input| input.read(cx).value().to_string())
                .unwrap_or_default(),
        )
    }

    /// Follow composer edits for this conversation (once per conversation).
    pub(super) fn watch_visual_note(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(input) = self.agent_chat_inputs.get(&agent_id).cloned() else {
            return;
        };
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry.note_watched {
            return;
        }
        entry.note_watched = true;
        cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.sync_visual_note(agent_id, cx);
            }
        })
        .detach();
    }

    /// Keep the open answer's draft equal to the composer note. The cached
    /// draft changes at once (and wins over any save already in flight); the
    /// write is debounced per answer.
    fn sync_visual_note(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let note = self.visual_composer_note(agent_id, cx);
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        // Only a choice beside the composer owns its text; next to an
        // informational result the composer is ordinary chat.
        if !entry.takes_composer() || entry.send_in_flight() {
            return;
        }
        // Chips or attachments make this a normal message; keep the saved note.
        let Some(note) = note else { return };
        let Some(answer_id) = entry.current else {
            return;
        };
        let Some(answer) = entry.answers.get_mut(&answer_id) else {
            return;
        };
        match note_for_answer(answer, &note) {
            Ok(None) => {}
            Ok(Some(note)) => {
                answer.interaction.draft = note;
                entry.state_version += 1;
                let had_error = entry.note_error.take().is_some();
                let generation = entry.note_timers.bump(answer_id);
                if had_error {
                    cx.notify();
                }
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(NOTE_SAVE_DELAY).await;
                    this.update(cx, |this, cx| {
                        let due = this
                            .visual_answers
                            .get(&agent_id)
                            .is_some_and(|e| e.note_timers.due(answer_id, generation));
                        if due {
                            this.queue_visual_job(agent_id, VisualJob::Persist(answer_id), cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
            Err(message) => {
                if entry.note_error.is_none() {
                    entry.note_error = Some(message.into());
                    cx.notify();
                }
            }
        }
    }

    /// After a restart (or reopening), a saved note returns to an empty
    /// composer. Applied once, during render where a window is available.
    pub(super) fn restore_visual_note(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.agent_chat_inputs.get(&agent_id).cloned() else {
            return;
        };
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let Some(answer_id) = entry.restore_note.take() else {
            return;
        };
        if !entry.takes_composer() || entry.current != Some(answer_id) {
            return;
        }
        let Some(answer) = entry.answers.get(&answer_id) else {
            return;
        };
        if let Some(note) = note_to_restore(answer, &input.read(cx).value()) {
            input.update(cx, |input, cx| input.set_value(note, window, cx));
        }
    }

    /// Explicit native submit (panel actions and composer refinements).
    /// Selecting an option never sends; this does. The renderer's snapshot is
    /// taken first so the newest selection and view are what get sent.
    pub(super) fn submit_visual_action(
        &mut self,
        agent_id: Uuid,
        action_id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        // A follow-up on an informational result sends just that action; the
        // composer text stays an ordinary, unsent chat draft.
        let note = if self.visual_answer_takes_composer(agent_id, cx) {
            self.visual_composer_note(agent_id, cx)
        } else {
            Some(String::new())
        };
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return false;
        };
        let Some(answer) = entry.current_answer() else {
            return false;
        };
        if entry.send_in_flight() || answer.interaction.submission.is_some() {
            return false;
        }
        let Some(note) = note else {
            entry.error = Some(CHIPS_NOT_SENT.into());
            cx.notify();
            return false;
        };
        if note.len() > MAX_VISUAL_NOTE_BYTES {
            entry.error = Some(NOTE_TOO_LONG.into());
            cx.notify();
            return false;
        }
        let target = SendTarget {
            answer_id: answer.id,
            revision: answer.revision,
            surface: entry.surface,
        };
        entry.sending = Some(target);
        entry.error = None;
        let token = self
            .web_host
            .update(cx, |host, _| host.request_visual_snapshot(target.surface));
        let pending = PendingVisualSubmit {
            token: token.unwrap_or_default(),
            target,
            action_id,
            note,
            window: Some(window.window_handle()),
        };
        match token {
            // No live surface (still loading, hidden): the cached state is current.
            None => self.queue_visual_submit(agent_id, pending, cx),
            Some(token) => {
                if let Some(entry) = self.visual_answers.get_mut(&agent_id) {
                    entry.pending_submit = Some(pending);
                }
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(SNAPSHOT_TIMEOUT).await;
                    this.update(cx, |this, cx| {
                        this.release_visual_submit(agent_id, token, cx)
                    })
                    .ok();
                })
                .detach();
            }
        }
        cx.notify();
        true
    }

    /// Continue a send once its snapshot arrived, or when the bounded wait
    /// ends (then the last state Choro received is used). A send whose
    /// answer changed meanwhile was already cancelled (`cancel_stale_send`).
    pub(super) fn release_visual_submit(
        &mut self,
        agent_id: Uuid,
        token: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry
            .pending_submit
            .as_ref()
            .is_none_or(|p| p.token != token)
        {
            return;
        }
        if let Some(pending) = entry.pending_submit.take() {
            self.queue_visual_submit(agent_id, pending, cx);
        }
    }

    fn queue_visual_submit(
        &mut self,
        agent_id: Uuid,
        pending: PendingVisualSubmit,
        cx: &mut Context<Self>,
    ) {
        let display = self
            .visual_answers
            .get(&agent_id)
            .and_then(|entry| entry.answers.get(&pending.target.answer_id))
            .map(|answer| submission_display(answer, &pending.action_id, &pending.note))
            .unwrap_or_default();
        self.queue_visual_job(
            agent_id,
            VisualJob::Submit {
                target: pending.target,
                action_id: pending.action_id,
                draft: pending.note,
                display,
                window: pending.window,
            },
            cx,
        );
    }

    /// The user confirms an uncertain restored send reached the agent.
    pub(super) fn confirm_visual_submission_sent(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some((answer_id, revision, submission_id)) = self.uncertain_visual_submission(agent_id)
        else {
            return;
        };
        self.queue_visual_job(
            agent_id,
            VisualJob::Finish {
                answer_id,
                revision,
                submission_id,
                accepted: true,
            },
            cx,
        );
    }

    /// The user explicitly asks to send an uncertain restored submission again.
    pub(super) fn resend_visual_submission(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((answer_id, revision, submission_id)) = self.uncertain_visual_submission(agent_id)
        else {
            return;
        };
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry.send_in_flight() {
            return;
        }
        let Some(action_id) = entry
            .answers
            .get(&answer_id)
            .and_then(|a| a.interaction.submission.as_ref())
            .map(|s| s.action_id.clone())
        else {
            return;
        };
        entry.sending = Some(SendTarget {
            answer_id,
            revision,
            surface: entry.surface,
        });
        self.queue_visual_job(
            agent_id,
            VisualJob::Resend {
                answer_id,
                revision,
                submission_id,
                action_id,
                window: window.window_handle(),
            },
            cx,
        );
        cx.notify();
    }

    fn uncertain_visual_submission(&self, agent_id: Uuid) -> Option<(Uuid, u64, Uuid)> {
        let entry = self
            .visual_answers
            .get(&agent_id)
            .filter(|e| e.uncertain_submission())?;
        let answer = entry.current_answer()?;
        Some((
            answer.id,
            answer.revision,
            answer.interaction.submission.as_ref()?.id,
        ))
    }

    /// End a send that did not reach dispatch, with an optional message.
    fn release_visual_send(
        &mut self,
        agent_id: Uuid,
        target: SendTarget,
        error: Option<SharedString>,
    ) {
        if let Some(entry) = self.visual_answers.get_mut(&agent_id) {
            if entry.sending == Some(target) {
                entry.sending = None;
            }
            if error.is_some() {
                entry.error = error;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn run_visual_submit(
        &mut self,
        agent_id: Uuid,
        project_id: Uuid,
        target: SendTarget,
        action_id: String,
        draft: String,
        display: String,
        recover: Option<Uuid>,
        window: Option<gpui::AnyWindowHandle>,
        cx: &mut Context<Self>,
    ) {
        // Only the revision the user saw may be sent. If a newer revision
        // (or another answer) is shown now, cancel visibly; nothing is sent.
        let answer = self
            .visual_answers
            .get(&agent_id)
            .and_then(|entry| entry.current_answer().filter(|a| target.matches(Some(a))))
            .cloned();
        let (Some(answer), Some(window)) = (answer, window) else {
            self.release_visual_send(agent_id, target, Some(REVISION_CHANGED.into()));
            cx.notify();
            return self.run_visual_jobs(agent_id, cx);
        };
        let draft_for_store = recover.is_none().then(|| draft.clone());
        cx.spawn(async move |this, cx| {
            let begun = cx
                .background_executor()
                .spawn(async move {
                    let store = Self::visual_store()?;
                    if let Some(submission_id) = recover {
                        store.finish_visual_submission(
                            project_id,
                            agent_id,
                            target.answer_id,
                            target.revision,
                            submission_id,
                            false,
                        )?;
                    }
                    // Persist the latest client state (selection, view) and then
                    // reserve the receipt with the composer note, before dispatch.
                    let latest =
                        store.read_visual_answer(project_id, agent_id, target.answer_id, None)?;
                    anyhow::ensure!(latest.revision == target.revision, REVISION_CHANGED);
                    let merged = merge_client_state(&latest.interaction, &answer.interaction);
                    let saved = if merged != latest.interaction {
                        store.save_visual_interaction(
                            project_id,
                            agent_id,
                            target.answer_id,
                            target.revision,
                            merged,
                        )?
                    } else {
                        latest
                    };
                    store.begin_visual_submission_with_draft(
                        project_id,
                        agent_id,
                        target.answer_id,
                        target.revision,
                        saved.interaction.revision,
                        &action_id,
                        draft_for_store,
                    )
                })
                .await;
            let delivered = window
                .update(cx, |_, window, cx| {
                    this.update(cx, |this, cx| {
                        this.complete_visual_begin(
                            agent_id, target, begun, draft, display, window, cx,
                        );
                    })
                })
                .is_ok();
            if !delivered {
                // The window closed: release the queue; a recorded receipt
                // stays `sending` and is recovered explicitly after restart.
                this.update(cx, |this, cx| {
                    this.release_visual_send(agent_id, target, None);
                    this.run_visual_jobs(agent_id, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn complete_visual_begin(
        &mut self,
        agent_id: Uuid,
        target: SendTarget,
        begun: anyhow::Result<VisualAnswer>,
        draft: String,
        display: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let pending = match begun {
            Ok(pending) => pending,
            Err(error) => {
                self.release_visual_send(agent_id, target, Some(format!("{error:#}").into()));
                cx.notify();
                return self.run_visual_jobs(agent_id, cx);
            }
        };
        let Some(receipt) = pending.interaction.submission.clone() else {
            self.release_visual_send(agent_id, target, None);
            return self.run_visual_jobs(agent_id, cx);
        };
        let title = pending.document.title.clone();
        self.accept_saved_visual_answer(agent_id, pending);
        let agent = self.agents.read(cx).agent(agent_id).cloned();
        let mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        // The receipt is recorded; dispatch through the normal conversation
        // path so the current mode, permissions and queueing all apply.
        let accepted = agent.is_some_and(|agent| {
            self.dispatch_agent_chat_submission_with_agent(
                &agent,
                receipt.text.clone(),
                Some(if display.trim().is_empty() {
                    "Visual answer follow-up".into()
                } else {
                    display
                }),
                vec![AgentChatMessageTag {
                    kind: AgentChatMessageTagKind::VisualAnswer,
                    label: title,
                    detail: Some(format!("Revision {}", target.revision)),
                }],
                mode,
                cx,
            )
        });
        if accepted {
            if let Some(input) = self.agent_chat_inputs.get(&agent_id).cloned() {
                if input.read(cx).value().trim() == draft.trim() && !draft.trim().is_empty() {
                    input.update(cx, |input, cx| input.set_value("", window, cx));
                }
            }
            self.acknowledge_agent_chat_seen(agent_id, cx);
        }
        self.queue_visual_job(
            agent_id,
            VisualJob::Finish {
                answer_id: target.answer_id,
                revision: target.revision,
                submission_id: receipt.id,
                accepted,
            },
            cx,
        );
        self.run_visual_jobs(agent_id, cx);
    }

    /// Collapse a choice after an accepted send; a rejected send stays open
    /// for retry. An informational result stays readable in its card.
    pub(super) fn settle_visual_send(&mut self, agent_id: Uuid, answer_id: Uuid, accepted: bool) {
        if let Some(entry) = self.visual_answers.get_mut(&agent_id) {
            if entry.sending.is_some_and(|t| t.answer_id == answer_id) {
                entry.sending = None;
            }
            if accepted && entry.current == Some(answer_id) && entry.takes_composer() {
                entry.mode = VisualPanelMode::Collapsed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::agent_chat_visual_answer::VisualAnswerChat;
    use super::*;
    use ide_core::visual_answers::{VisualDocument, VisualInteraction};

    fn answer(id: Uuid, revision: u64) -> VisualAnswer {
        let document: VisualDocument = serde_json::from_value(serde_json::json!({
            "title":"Projects", "summary":"Sample", "kind":"table",
            "openui_lang":"root = Answer([t])\nt = DataTable(\"p\")",
            "datasets":{"p":{"columns":[{"key":"name","label":"Project","kind":"text"}],"rows":[]}},
            "sources":[{"label":"Sample data"}]
        }))
        .unwrap();
        VisualAnswer {
            id,
            project_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            revision,
            document,
            interaction: VisualInteraction::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    fn chat_sending(id: Uuid) -> VisualAnswerChat {
        let target = SendTarget {
            answer_id: id,
            revision: 1,
            surface: 3,
        };
        let mut entry = VisualAnswerChat::default();
        entry.answers.insert(id, answer(id, 1));
        entry.current = Some(id);
        entry.mode = VisualPanelMode::Docked;
        entry.surface = 3;
        entry.sending = Some(target);
        entry.pending_submit = Some(PendingVisualSubmit {
            token: 7,
            target,
            action_id: "choro_refine".into(),
            note: "Only blocked".into(),
            window: None,
        });
        entry
    }

    #[test]
    fn a_revision_during_the_snapshot_wait_cancels_the_send_visibly() {
        let id = Uuid::from_u128(1);
        let mut entry = chat_sending(id);
        assert!(entry.send_in_flight() && entry.sending_current());
        // visual_present publishes revision 2 while the snapshot is awaited.
        entry.answers.insert(id, answer(id, 2));
        entry.cancel_stale_send();
        assert!(
            entry.pending_submit.is_none(),
            "the old note/action is never sent"
        );
        assert!(
            entry.sending.is_none(),
            "the new panel is not stuck sending"
        );
        assert!(!entry.send_in_flight(), "the composer is usable again");
        assert_eq!(
            entry.error.as_ref().map(|e| e.as_ref()),
            Some(REVISION_CHANGED)
        );
    }

    #[test]
    fn a_send_queued_behind_a_save_never_targets_a_newer_revision() {
        let id = Uuid::from_u128(1);
        let mut entry = chat_sending(id);
        // The snapshot arrived: the send is queued behind a Persist job.
        let pending = entry.pending_submit.take().unwrap();
        // A revision lands before the queued send runs.
        entry.answers.insert(id, answer(id, 2));
        entry.cancel_stale_send();
        assert!(
            entry.sending.is_some(),
            "already queued: cancelled when it runs"
        );
        assert!(
            !entry.sending_current(),
            "the new revision does not show Sending…"
        );
        // run_visual_submit's check: the queued target no longer matches.
        assert!(!pending.target.matches(entry.current_answer()));
    }

    #[test]
    fn an_unchanged_answer_keeps_its_pending_send() {
        let id = Uuid::from_u128(1);
        let mut entry = chat_sending(id);
        entry.cancel_stale_send();
        assert!(entry.pending_submit.is_some());
        assert!(entry.error.is_none());
    }
}
