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

/// A cheap fingerprint of everything that can change a row's rendered height
/// while the row count stays the same — chiefly an answer whose text grows token
/// by token, and a work-log group that gains steps. `ListState` caches a
/// measured height per row and only invalidates it on a splice, so without this
/// a row that grows in place keeps the height it had when it first appeared:
/// prose merely overflows, but a markdown table ends up hundreds of pixels
/// taller than its slot and paints over whatever follows.
///
/// Deliberately length-based rather than a content hash. This runs for every row
/// on every frame, and streaming growth is monotonic, so a length notices it
/// without walking the text.
fn agent_chat_message_fingerprint_value(
    message: &AgentChatMessage,
    active_reveal: Option<&agent_chat_reveal::ActiveReveal>,
) -> u64 {
    match message {
        AgentChatMessage::User {
            text,
            display_text,
            tags,
            ..
        } => (text.len() + display_text.as_ref().map_or(0, String::len) + tags.len()) as u64,
        AgentChatMessage::Assistant {
            message_id,
            text,
            created_at,
        } => active_reveal
            .filter(|reveal| {
                reveal.key.0.as_ref() == message_id.as_ref() && reveal.key.1 == *created_at
            })
            .map_or_else(
                || (text.len() as u64).wrapping_mul(2),
                |reveal| {
                    // The stored answer can already be complete while the
                    // paced renderer is still revealing it. Fingerprint what
                    // is actually visible so ListState remeasures the row as
                    // the reveal grows, plus one state bit for the final live
                    // -> Markdown renderer transition.
                    let visible = agent_chat_reveal::char_byte_offset(text, reveal.revealed);
                    (visible as u64)
                        .wrapping_mul(2)
                        .wrapping_add(u64::from(reveal.is_animating()))
                },
            ),
        AgentChatMessage::Thought { text, .. } => text.len() as u64,
    }
}

pub(super) fn agent_chat_row_fingerprint(
    row: &AgentChatRow,
    session: &AgentChatSession,
    active_reveal: Option<&agent_chat_reveal::ActiveReveal>,
) -> u64 {
    fn mix(kind: u64, index: usize, value: u64) -> u64 {
        kind.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (index as u64).wrapping_mul(1_000_003) ^ value
    }

    match row {
        AgentChatRow::Message(index) => mix(
            1,
            *index,
            session.messages.get(*index).map_or(0, |message| {
                agent_chat_message_fingerprint_value(message, active_reveal)
            }),
        ),
        AgentChatRow::TimelineItem(index) => match session.timeline.get(*index) {
            Some(AgentChatTimelineItem::Message(message)) => mix(
                2,
                *index,
                agent_chat_message_fingerprint_value(message, active_reveal),
            ),
            // Cards only change height through an explicit user action (expand,
            // collapse, apply), and every one of those paths already calls
            // `remeasure_agent_chat_list`.
            _ => mix(3, *index, 0),
        },
        AgentChatRow::WorkLogGroup { start, end } => mix(4, *start, *end as u64),
        AgentChatRow::ResumeSavedSession => mix(5, 0, 0),
        AgentChatRow::Activity => mix(6, 0, 0),
    }
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
    use super::*;

    #[test]
    fn newest_turns_reverse_but_each_turn_keeps_reading_order() {
        let (order, newest_len) =
            newest_turn_first_order(&[true, false, false, true, false, true, false, false]);

        assert_eq!(order, vec![5, 6, 7, 3, 4, 0, 1, 2]);
        assert_eq!(newest_len, 3);
    }

    #[test]
    fn assistant_fingerprint_tracks_visible_reveal_and_final_transition() {
        let message = AgentChatMessage::Assistant {
            message_id: Some("m1".to_string()),
            text: "one two three".to_string(),
            created_at: 7,
        };
        let partial = agent_chat_reveal::ActiveReveal {
            agent: Uuid::nil(),
            key: (Some("m1".to_string()), 7),
            revealed: 3,
            full: 13,
            warmth: 1.0,
        };
        let complete_but_warm = agent_chat_reveal::ActiveReveal {
            revealed: 13,
            full: 13,
            warmth: 0.5,
            ..partial.clone()
        };
        let finalized = agent_chat_reveal::ActiveReveal {
            warmth: 0.0,
            ..complete_but_warm.clone()
        };

        let partial_value = agent_chat_message_fingerprint_value(&message, Some(&partial));
        let complete_value =
            agent_chat_message_fingerprint_value(&message, Some(&complete_but_warm));
        let finalized_value = agent_chat_message_fingerprint_value(&message, Some(&finalized));

        assert_ne!(partial_value, complete_value);
        assert_ne!(complete_value, finalized_value);
        assert_eq!(
            finalized_value,
            agent_chat_message_fingerprint_value(&message, None)
        );
    }
}
