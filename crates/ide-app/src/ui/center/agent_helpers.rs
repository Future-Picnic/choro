use super::*;

// ---- composer control icons ---------------------------------------------
//
// One source per control, shared by the agent-chat and new-agent composers so
// their glyphs can't drift apart. Access and mode icons change with the selected
// value, so the glyph states the mode before you read the label.

/// The model glyph — a concentric hexagon with a filled core (custom asset).
pub(super) fn composer_model_icon(color: gpui::Hsla) -> gpui::AnyElement {
    gpui::svg()
        .path("agent-icons/model.svg")
        .size(crate::ui::design::icon_sm())
        .text_color(color)
        .into_any_element()
}

/// Reasoning effort — a gauge communicates the selected thinking intensity.
pub(super) fn composer_effort_icon(
    _effort: ide_core::AgentEffort,
    color: gpui::Hsla,
) -> gpui::AnyElement {
    crate::ui::design::indicator::lucide_icon(
        lucide_icons::Icon::Gauge,
        color,
        crate::ui::design::icon_sm(),
    )
    .into_any_element()
}

/// Access mode, as an escalation of trust: locked (asks first) → unlocked (edits
/// freely) → shield off (no guardrails).
pub(super) fn composer_access_icon(
    mode: ide_core::AgentAccessMode,
    color: gpui::Hsla,
) -> gpui::AnyElement {
    let icon = match mode {
        ide_core::AgentAccessMode::AskForApproval => lucide_icons::Icon::Lock,
        ide_core::AgentAccessMode::AutoAcceptEdits => lucide_icons::Icon::LockOpen,
        ide_core::AgentAccessMode::FullAccess => lucide_icons::Icon::ShieldOff,
    };
    crate::ui::design::indicator::lucide_icon(icon, color, crate::ui::design::icon_sm())
        .into_any_element()
}

/// Solo — a fork stepping out on its own lane.
pub(super) fn composer_solo_icon(color: gpui::Hsla) -> gpui::AnyElement {
    crate::ui::design::indicator::solo_icon(color, crate::ui::design::icon_sm()).into_any_element()
}

/// Build vs Plan — a hammer acts, a checklist plans.
pub(super) fn composer_mode_icon(plan: bool, color: gpui::Hsla) -> gpui::AnyElement {
    let icon = if plan {
        lucide_icons::Icon::ListChecks
    } else {
        lucide_icons::Icon::Hammer
    };
    crate::ui::design::indicator::lucide_icon(icon, color, crate::ui::design::icon_sm())
        .into_any_element()
}

pub(super) fn agent_footer_tasks(session: &AgentChatSession) -> Vec<AgentFooterTask> {
    // Reflect the agent's own live to-do list (Codex `update_plan` / ACP plan),
    // which already carries a real per-item status. We deliberately do NOT
    // synthesize a to-do from the proposed-plan markdown — that's a separate
    // approval artifact — and we trust the agent's statuses rather than guessing.
    let Some(plan_entry) = session
        .work_log
        .iter()
        .rev()
        .find(|entry| entry.kind == WorkLogEntryKind::Plan && entry.detail.is_some())
    else {
        return Vec::new();
    };
    let tasks = plan_entry
        .detail
        .as_deref()
        .map(parse_agent_footer_tasks)
        .unwrap_or_default();
    // The agent clears its to-do once every item is done; mirror that so the
    // footer disappears instead of lingering as an all-complete list.
    if !tasks.is_empty()
        && tasks
            .iter()
            .all(|task| task.status == WorkLogStatus::Completed)
    {
        return Vec::new();
    }
    tasks
}

pub(super) fn parse_agent_footer_tasks(detail: &str) -> Vec<AgentFooterTask> {
    detail
        .lines()
        .filter_map(|line| {
            let (status, text) = line.split_once('\t')?;
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            Some(AgentFooterTask {
                text: text.to_string(),
                status: footer_task_status(status),
            })
        })
        .collect()
}

pub(super) fn footer_task_status(status: &str) -> WorkLogStatus {
    match status {
        "done" | "completed" | "complete" | "success" => WorkLogStatus::Completed,
        "in_progress" | "in-progress" | "running" | "active" | "started" => {
            WorkLogStatus::InProgress
        }
        "failed" | "error" => WorkLogStatus::Failed,
        _ => WorkLogStatus::Pending,
    }
}

pub(super) fn current_footer_task_index(tasks: &[AgentFooterTask]) -> Option<usize> {
    tasks
        .iter()
        .position(|task| task.status == WorkLogStatus::InProgress)
        .or_else(|| {
            tasks
                .iter()
                .position(|task| task.status == WorkLogStatus::Pending)
        })
        .or_else(|| tasks.len().checked_sub(1))
}

pub(super) fn message_metadata_row(
    id: (&'static str, usize),
    text: String,
    created_at: u64,
    align_end: bool,
    visible: bool,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let timestamp = chat_message_time_label(created_at);
    let content = h_flex()
        .gap_1p5()
        .items_center()
        .text_size(crate::ui::design::text_ui())
        .text_color(crate::ui::design::t3(cx).opacity(0.78))
        .opacity(if visible { 1. } else { 0. })
        .child(
            Button::new(id)
                .ghost()
                .xsmall()
                .compact()
                .h(px(16.))
                .icon(IconName::Copy)
                .tooltip("Copy message")
                .disabled(!visible)
                .on_click(move |_, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                }),
        )
        .child(timestamp);

    h_flex()
        .w_full()
        .h(px(16.))
        .when(align_end, |row| row.justify_end())
        .when(!align_end, |row| row.justify_start())
        .child(content)
        .into_any_element()
}

pub(super) fn timeline_contains_user_message(
    timeline: &[AgentChatTimelineItem],
    text: &str,
) -> bool {
    timeline.iter().any(|item| {
        matches!(
            item,
            AgentChatTimelineItem::Message(AgentChatMessage::User { text: existing, .. })
                if existing == text
        )
    })
}

pub(super) fn is_agent_stop_work_log(entry: &crate::state::agent_chat::WorkLogEntry) -> bool {
    matches!(
        entry.collapse_key.as_str(),
        "agent-chat-force-stopped" | "agent-chat-cancelled"
    )
}

pub(super) fn agent_chat_rows(
    session: &AgentChatSession,
    include_activity: bool,
    include_saved_resume: bool,
    artifact_filter: &VisualizationArtifactFilter,
) -> Vec<AgentChatRow> {
    let mut rows = Vec::new();

    if session.timeline.is_empty() {
        rows.extend((0..session.messages.len()).map(AgentChatRow::Message));
    } else {
        for (index, message) in session.messages.iter().enumerate() {
            if let AgentChatMessage::User { text, .. } = message {
                if !timeline_contains_user_message(&session.timeline, text) {
                    rows.push(AgentChatRow::Message(index));
                }
            }
        }

        let mut index = 0;
        while index < session.timeline.len() {
            match &session.timeline[index] {
                AgentChatTimelineItem::PendingUserInput(pending)
                    if session
                        .pending_user_input
                        .as_ref()
                        .is_some_and(|active| active.request_id == pending.request_id) =>
                {
                    index += 1;
                }
                AgentChatTimelineItem::WorkLog(entry) if is_agent_stop_work_log(entry) => {
                    // A stop is a user-visible outcome, not another tool step.
                    // Keep it out of the collapsed "Worked on …" group.
                    rows.push(AgentChatRow::TimelineItem(index));
                    index += 1;
                }
                AgentChatTimelineItem::WorkLog(_) => {
                    let start = index;
                    while matches!(
                        session.timeline.get(index),
                        Some(AgentChatTimelineItem::WorkLog(entry))
                            if !is_agent_stop_work_log(entry)
                    ) {
                        index += 1;
                    }
                    rows.push(AgentChatRow::WorkLogGroup { start, end: index });
                }
                AgentChatTimelineItem::ChangedFiles(summary)
                    if !summary
                        .files
                        .iter()
                        .any(|file| !artifact_filter.is_artifact(&file.path)) =>
                {
                    index += 1;
                }
                _ => {
                    rows.push(AgentChatRow::TimelineItem(index));
                    index += 1;
                }
            }
        }
    }

    if include_activity {
        rows.push(AgentChatRow::Activity);
    }
    if rows.is_empty() && include_saved_resume {
        rows.push(AgentChatRow::ResumeSavedSession);
    }

    rows
}

/// Map visual list positions to chronological row positions. Top-down chat
/// reverses complete user turns rather than individual rows, so a prompt still
/// appears before the work and answer it produced.
pub(super) fn agent_chat_display_order(
    rows: &[AgentChatRow],
    session: &AgentChatSession,
    newest_turn_first: bool,
) -> (Vec<usize>, usize) {
    if !newest_turn_first {
        return ((0..rows.len()).collect(), 0);
    }

    let user_starts = rows
        .iter()
        .map(|row| match row {
            AgentChatRow::Message(index) => session
                .messages
                .get(*index)
                .is_some_and(|message| matches!(message, AgentChatMessage::User { .. })),
            AgentChatRow::TimelineItem(index) => session.timeline.get(*index).is_some_and(|item| {
                matches!(
                    item,
                    AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
                )
            }),
            _ => false,
        })
        .collect::<Vec<_>>();
    newest_turn_first_order(&user_starts)
}

fn newest_turn_first_order(user_starts: &[bool]) -> (Vec<usize>, usize) {
    let mut turns: Vec<Vec<usize>> = Vec::new();
    for (index, starts_turn) in user_starts.iter().copied().enumerate() {
        if starts_turn || turns.is_empty() {
            turns.push(vec![index]);
        } else if let Some(turn) = turns.last_mut() {
            turn.push(index);
        }
    }
    let newest_turn_len = turns.last().map_or(0, Vec::len);
    let order = turns.into_iter().rev().flatten().collect();
    (order, newest_turn_len)
}

pub(super) fn agent_changed_file_key(agent_id: Uuid, path: &Path) -> u64 {
    let mut hash = (agent_id.as_u128() as u64) ^ ((agent_id.as_u128() >> 64) as u64);
    for byte in path.to_string_lossy().bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::newest_turn_first_order;

    #[test]
    fn newest_turns_reverse_but_each_turn_keeps_reading_order() {
        let (order, newest_len) =
            newest_turn_first_order(&[true, false, false, true, false, true, false, false]);

        assert_eq!(order, vec![5, 6, 7, 3, 4, 0, 1, 2]);
        assert_eq!(newest_len, 3);
    }
}
