#![allow(dead_code, reason = "retained agent interaction update paths")]

use super::*;

impl AgentChatState {
    pub fn has_review_checklist_for_turn(&self, agent_id: Uuid, source_turn_id: &str) -> bool {
        self.sessions.get(&agent_id).is_some_and(|session| {
            session.timeline.iter().any(|item| {
                matches!(
                    item,
                    AgentChatTimelineItem::ReviewChecklist(checklist)
                        if checklist.source_turn_id == source_turn_id
                )
            })
        })
    }

    pub fn begin_review_checklist(
        &mut self,
        agent_id: Uuid,
        source_turn_id: String,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return false;
        };
        if session.timeline.iter().any(|item| {
            matches!(
                item,
                AgentChatTimelineItem::ReviewChecklist(checklist)
                    if checklist.source_turn_id == source_turn_id
            )
        }) {
            return false;
        }
        if let Some(previous) = session
            .timeline
            .iter_mut()
            .rev()
            .find_map(|item| match item {
                AgentChatTimelineItem::ReviewChecklist(checklist) if checklist.expanded => {
                    Some(checklist)
                }
                _ => None,
            })
        {
            previous.expanded = false;
            persist_timeline_item(
                agent_id,
                AgentChatTimelineItem::ReviewChecklist(previous.clone()),
                cx,
            );
        }
        let checklist = ReviewChecklist::pending(source_turn_id, unix_now());
        session
            .timeline
            .push(AgentChatTimelineItem::ReviewChecklist(checklist.clone()));
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::ReviewChecklist(checklist),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
        true
    }

    pub fn retry_review_checklist(
        &mut self,
        agent_id: Uuid,
        source_turn_id: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return false;
        };
        let Some(checklist) = session
            .timeline
            .iter_mut()
            .rev()
            .find_map(|item| match item {
                AgentChatTimelineItem::ReviewChecklist(checklist)
                    if checklist.source_turn_id == source_turn_id
                        && checklist.status == ReviewChecklistStatus::Failed =>
                {
                    Some(checklist)
                }
                _ => None,
            })
        else {
            return false;
        };
        checklist.status = ReviewChecklistStatus::Pending;
        checklist.items.clear();
        checklist.expanded = true;
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::ReviewChecklist(checklist.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
        true
    }

    pub fn fail_review_checklist(
        &mut self,
        agent_id: Uuid,
        source_turn_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(checklist) = session
            .timeline
            .iter_mut()
            .rev()
            .find_map(|item| match item {
                AgentChatTimelineItem::ReviewChecklist(checklist)
                    if checklist.source_turn_id == source_turn_id
                        && checklist.status == ReviewChecklistStatus::Pending =>
                {
                    Some(checklist)
                }
                _ => None,
            })
        else {
            return;
        };
        checklist.status = ReviewChecklistStatus::Failed;
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::ReviewChecklist(checklist.clone()),
            cx,
        );
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn toggle_review_checklist_item(
        &mut self,
        agent_id: Uuid,
        checklist_id: &str,
        item_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(checklist) = session.timeline.iter_mut().find_map(|item| match item {
            AgentChatTimelineItem::ReviewChecklist(checklist) if checklist.id == checklist_id => {
                Some(checklist)
            }
            _ => None,
        }) else {
            return;
        };
        let Some(item) = checklist.items.iter_mut().find(|item| item.id == item_id) else {
            return;
        };
        item.checked = !item.checked;
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::ReviewChecklist(checklist.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn toggle_review_checklist_expanded(
        &mut self,
        agent_id: Uuid,
        checklist_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(checklist) = session.timeline.iter_mut().find_map(|item| match item {
            AgentChatTimelineItem::ReviewChecklist(checklist) if checklist.id == checklist_id => {
                Some(checklist)
            }
            _ => None,
        }) else {
            return;
        };
        checklist.expanded = !checklist.expanded;
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::ReviewChecklist(checklist.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn select_pending_user_input_option(
        &mut self,
        agent_id: Uuid,
        option_label: &str,
        auto_advance_single_select: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(pending) = session.pending_user_input.as_mut() else {
            return;
        };

        let is_multi_select = pending
            .active_question()
            .is_some_and(|question| question.multi_select);
        pending.select_option(option_label);
        if auto_advance_single_select && !is_multi_select && !pending.progress().is_last_question {
            pending.next();
        }
        upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::PendingUserInput(pending.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn previous_pending_user_input_question(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(pending) = session.pending_user_input.as_mut() else {
            return;
        };

        pending.previous();
        upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::PendingUserInput(pending.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn next_pending_user_input_question(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(pending) = session.pending_user_input.as_mut() else {
            return;
        };

        pending.next();
        upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::PendingUserInput(pending.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn set_pending_user_input_custom_answer(
        &mut self,
        agent_id: Uuid,
        answer: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(pending) = session.pending_user_input.as_mut() else {
            return;
        };

        pending.set_custom_answer(answer);
        upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::PendingUserInput(pending.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn toggle_proposed_plan_expanded(
        &mut self,
        agent_id: Uuid,
        plan_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let mut expanded = None;
        let mut persist_plan = None;
        if let Some(plan) = session
            .proposed_plan
            .as_mut()
            .filter(|plan| plan.id == plan_id)
        {
            plan.expanded = !plan.expanded;
            expanded = Some(plan.expanded);
            persist_plan = Some(plan.clone());
        }
        let Some(expanded) = expanded else {
            return;
        };
        for item in &mut session.timeline {
            if let AgentChatTimelineItem::ProposedPlan(plan) = item {
                if plan.id == plan_id {
                    plan.expanded = expanded;
                    persist_plan = Some(plan.clone());
                }
            }
        }
        if let Some(plan) = persist_plan {
            persist_timeline_item(agent_id, AgentChatTimelineItem::ProposedPlan(plan), cx);
        }
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn toggle_code_review_expanded(
        &mut self,
        agent_id: Uuid,
        review_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let mut changed = false;
        let mut persist_review = None;
        for item in &mut session.timeline {
            if let AgentChatTimelineItem::CodeReview(review) = item {
                if review.id == review_id {
                    review.expanded = !review.expanded;
                    persist_review = Some(review.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
        if let Some(review) = persist_review {
            persist_timeline_item(agent_id, AgentChatTimelineItem::CodeReview(review), cx);
        }
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn toggle_verification_expanded(
        &mut self,
        agent_id: Uuid,
        verification_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let mut changed = false;
        let mut persist_verification = None;
        for item in &mut session.timeline {
            if let AgentChatTimelineItem::Verification(verification) = item {
                if verification.id == verification_id {
                    verification.expanded = !verification.expanded;
                    persist_verification = Some(verification.clone());
                    changed = true;
                }
            }
        }
        if !changed {
            return;
        }
        if let Some(verification) = persist_verification {
            persist_timeline_item(
                agent_id,
                AgentChatTimelineItem::Verification(verification),
                cx,
            );
        }
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn toggle_agent_summary_expanded(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let Some(card) = session.timeline.iter_mut().find_map(|item| match item {
            AgentChatTimelineItem::AgentSummary(card) => Some(card),
            _ => None,
        }) else {
            return;
        };
        card.expanded = !card.expanded;
        persist_timeline_item(
            agent_id,
            AgentChatTimelineItem::AgentSummary(card.clone()),
            cx,
        );
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    /// The requirement titles a verification fix turn should target: the
    /// clearly Missed items with no fix requested yet. Empty if the
    /// verification is gone or everything was met.
    pub fn verification_fix_lines(&self, agent_id: Uuid, verification_id: &str) -> Vec<String> {
        let Some(session) = self.sessions.get(&agent_id) else {
            return Vec::new();
        };
        for item in session.timeline.iter().rev() {
            if let AgentChatTimelineItem::Verification(verification) = item {
                if verification.id == verification_id {
                    return verification
                        .items_to_fix()
                        .iter()
                        .map(|item| item.summary_line())
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Mark the missed items a fix turn targets as fix-requested, so they show
    /// as sent and can't be requested again.
    pub fn mark_verification_items_fix_requested(
        &mut self,
        agent_id: Uuid,
        verification_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let mut changed = false;
        for item in &mut session.timeline {
            if let AgentChatTimelineItem::Verification(verification) = item {
                if verification.id == verification_id {
                    for entry in &mut verification.items {
                        if entry.status == VerificationStatus::Missed && !entry.fix_requested {
                            entry.fix_requested = true;
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            return;
        }
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    /// The `path:line — title` lines for the findings a fix turn should target:
    /// the ticked ones (`only_selected`) or all of them. Empty if the review or
    /// the selection is gone.
    pub fn code_review_fix_lines(
        &self,
        agent_id: Uuid,
        review_id: &str,
        only_selected: bool,
    ) -> Vec<String> {
        let Some(session) = self.sessions.get(&agent_id) else {
            return Vec::new();
        };
        for item in session.timeline.iter().rev() {
            if let AgentChatTimelineItem::CodeReview(review) = item {
                if review.id == review_id {
                    return review
                        .findings_to_fix(only_selected)
                        .iter()
                        .map(|finding| finding.summary_line())
                        .collect();
                }
            }
        }
        Vec::new()
    }

    /// Toggle whether a single review finding is ticked for "Fix selected".
    pub fn toggle_code_review_finding_selected(
        &mut self,
        agent_id: Uuid,
        review_id: &str,
        finding_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let mut changed = false;
        for item in &mut session.timeline {
            if let AgentChatTimelineItem::CodeReview(review) = item {
                if review.id == review_id {
                    if let Some(finding) = review.findings.get_mut(finding_index) {
                        if !finding.fix_requested {
                            finding.selected = !finding.selected;
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            return;
        }
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    /// Mark the findings a fix turn targets (the ticked ones, or all pending)
    /// as fix-requested and clear their selection, so they show as done and
    /// can't be fixed again.
    pub fn mark_code_review_findings_fix_requested(
        &mut self,
        agent_id: Uuid,
        review_id: &str,
        only_selected: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let mut changed = false;
        for item in &mut session.timeline {
            if let AgentChatTimelineItem::CodeReview(review) = item {
                if review.id == review_id {
                    for finding in &mut review.findings {
                        if !finding.fix_requested && (!only_selected || finding.selected) {
                            finding.fix_requested = true;
                            finding.selected = false;
                            changed = true;
                        }
                    }
                }
            }
        }
        if !changed {
            return;
        }
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn resolve_proposed_plan_submission(
        &mut self,
        agent_id: Uuid,
        draft_text: &str,
        cx: &mut Context<Self>,
    ) -> Option<(String, AgentInteractionMode)> {
        let session = self.sessions.get_mut(&agent_id)?;
        let plan = session.proposed_plan.as_mut()?;
        let submission = plan.resolve_submission(draft_text);
        if submission.refine_in_plan_mode {
            session.proposed_plan = None;
            remove_proposed_plan_blocks_from_timeline(&mut session.timeline);
            session.interaction_mode = AgentInteractionMode::Plan;
        } else {
            plan.mark_implemented();
            persist_timeline_item(
                agent_id,
                AgentChatTimelineItem::ProposedPlan(plan.clone()),
                cx,
            );
            session.interaction_mode = AgentInteractionMode::Default;
        }
        session.last_activity_at = unix_now();
        let mode = session.interaction_mode;
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
        Some((submission.text, mode))
    }

    pub fn dismiss_proposed_plan(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        session.proposed_plan = None;
        session.status = AgentChatStatus::Idle;
        session.interaction_mode = AgentInteractionMode::Default;
        session.started_running_at = None;
        session.last_activity_at = unix_now();
        cx.emit(AgentChatEvent::Changed);
        cx.notify();
    }

    pub fn replace_pending_user_input(
        &mut self,
        agent_id: Uuid,
        pending: Option<PendingUserInput>,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            session.pending_user_input = pending;
            if let Some(pending) = session.pending_user_input.clone() {
                upsert_timeline_pending_user_input(&mut session.timeline, pending.clone());
                persist_timeline_item(
                    agent_id,
                    AgentChatTimelineItem::PendingUserInput(pending),
                    cx,
                );
            }
            session.status = if session.pending_user_input.is_some() {
                AgentChatStatus::WaitingForUser
            } else {
                AgentChatStatus::Idle
            };
            session.last_activity_at = unix_now();
            if session.status != AgentChatStatus::Running {
                session.started_running_at = None;
            }
            cx.emit(AgentChatEvent::Changed);
            cx.notify();
        }
    }

    pub fn set_proposed_plan(
        &mut self,
        agent_id: Uuid,
        plan: ProposedPlan,
        cx: &mut Context<Self>,
    ) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            upsert_timeline_proposed_plan(&mut session.timeline, plan.clone());
            persist_timeline_item(
                agent_id,
                AgentChatTimelineItem::ProposedPlan(plan.clone()),
                cx,
            );
            session.proposed_plan = Some(plan);
            session.status = AgentChatStatus::PlanReady;
            session.started_running_at = None;
            session.last_activity_at = unix_now();
            cx.emit(AgentChatEvent::Changed);
            cx.notify();
        }
    }

    pub fn upsert_work_log(&mut self, agent_id: Uuid, entry: WorkLogEntry, cx: &mut Context<Self>) {
        if let Some(session) = self.sessions.get_mut(&agent_id) {
            upsert_work_log_entry(&mut session.work_log, entry.clone());
            upsert_timeline_work_log(&mut session.timeline, entry.clone());
            persist_timeline_item(agent_id, AgentChatTimelineItem::WorkLog(entry), cx);
            session.last_activity_at = unix_now();
            cx.emit(AgentChatEvent::Changed);
            cx.notify();
        }
    }
}

pub(super) fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

pub(super) fn remove_proposed_plan_blocks(messages: &mut Vec<AgentChatMessage>) {
    messages.retain_mut(|message| match message {
        AgentChatMessage::Assistant { text, .. } => {
            *text = strip_tagged_blocks(text, "proposed_plan");
            !text.trim().is_empty()
        }
        _ => true,
    });
}

pub(super) fn strip_tagged_blocks(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut output = String::new();
    let mut rest = text;

    while let Some(start) = rest.find(&open) {
        output.push_str(&rest[..start]);
        let after_open = &rest[start + open.len()..];
        let Some(end) = after_open.find(&close) else {
            break;
        };
        rest = &after_open[end + close.len()..];
    }

    output.push_str(rest);
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assistant_chunks_with_same_stream_id_merge() {
        let mut messages = Vec::new();
        append_or_extend_message(
            &mut messages,
            AgentChatMessage::Assistant {
                message_id: Some("item-1".into()),
                text: "Hel".into(),
                created_at: 1,
            },
        );
        let merged = append_or_extend_message(
            &mut messages,
            AgentChatMessage::Assistant {
                message_id: Some("item-1".into()),
                text: "lo".into(),
                created_at: 2,
            },
        );

        assert_eq!(messages.len(), 1);
        assert!(matches!(
            &merged,
            AgentChatMessage::Assistant { text, .. } if text == "Hello"
        ));
        assert!(matches!(
            &messages[0],
            AgentChatMessage::Assistant { text, .. } if text == "Hello"
        ));
    }

    #[test]
    fn assistant_chunks_with_different_stream_ids_stay_separate() {
        let mut messages = Vec::new();
        append_or_extend_message(
            &mut messages,
            AgentChatMessage::Assistant {
                message_id: Some("item-1".into()),
                text: "First".into(),
                created_at: 1,
            },
        );
        append_or_extend_message(
            &mut messages,
            AgentChatMessage::Assistant {
                message_id: Some("item-2".into()),
                text: "Second".into(),
                created_at: 2,
            },
        );

        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn assistant_chunks_without_stream_ids_stay_separate() {
        let mut messages = Vec::new();
        append_or_extend_message(
            &mut messages,
            AgentChatMessage::Assistant {
                message_id: None,
                text: "First".into(),
                created_at: 1,
            },
        );
        append_or_extend_message(
            &mut messages,
            AgentChatMessage::Assistant {
                message_id: None,
                text: "Second".into(),
                created_at: 2,
            },
        );

        assert_eq!(messages.len(), 2);
    }
}
