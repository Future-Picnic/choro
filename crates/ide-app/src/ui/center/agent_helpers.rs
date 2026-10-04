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

pub(super) fn is_noise_work_log(entry: &crate::state::agent_chat::WorkLogEntry) -> bool {
    let title = entry.title.trim();
    (entry.kind == WorkLogEntryKind::Step
        && (title.eq_ignore_ascii_case("reasoning") || title.eq_ignore_ascii_case("thinking")))
        || (entry.kind == WorkLogEntryKind::Tool && title.eq_ignore_ascii_case("file change"))
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
        let has_ship_result = session
            .timeline
            .iter()
            .any(|item| matches!(item, AgentChatTimelineItem::ShipResult(_)));
        for (index, message) in session.messages.iter().enumerate() {
            if let AgentChatMessage::User { text, .. } = message {
                if !timeline_contains_user_message(&session.timeline, text) {
                    rows.push(AgentChatRow::Message(index));
                }
            }
        }

        let mut index = 0;
        let mut hidden_review_checklist_turn = false;
        let mut hidden_background_summary_turn = false;
        while index < session.timeline.len() {
            match &session.timeline[index] {
                AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. })
                    if super::agent_chat_brain::is_background_summary_request(text) =>
                {
                    hidden_review_checklist_turn = false;
                    hidden_background_summary_turn = true;
                    index += 1;
                }
                AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. })
                    if text.starts_with(REVIEW_CHECKLIST_REQUEST_MARKER) =>
                {
                    hidden_background_summary_turn = false;
                    hidden_review_checklist_turn = true;
                    index += 1;
                }
                AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => {
                    hidden_background_summary_turn = false;
                    hidden_review_checklist_turn = false;
                    // The received card is the visible prompt for a teammate
                    // turn. A second, empty prompt row would split the card
                    // from its answer in newest-turn-first layout.
                    if !super::agent_chat_brain::is_agent_request_submission(text)
                        && !super::agent_chat_brain::is_teammate_result_submission(text)
                    {
                        rows.push(AgentChatRow::TimelineItem(index));
                    }
                    index += 1;
                }
                AgentChatTimelineItem::AgentMessage(_) => {
                    // These arrive independently of background checklist and
                    // summary turns; their receipts must never be suppressed.
                    rows.push(AgentChatRow::TimelineItem(index));
                    index += 1;
                }
                AgentChatTimelineItem::ReviewChecklist(_) => {
                    rows.push(AgentChatRow::TimelineItem(index));
                    index += 1;
                }
                AgentChatTimelineItem::CodeReview(review) if review.structured.is_some() => {
                    // Independent reviews are started from the composer, not
                    // by a new coding turn. Their progress and outcomes must
                    // survive the preceding hidden checklist-maintenance turn.
                    rows.push(AgentChatRow::TimelineItem(index));
                    index += 1;
                }
                AgentChatTimelineItem::AgentSummary(_) if hidden_background_summary_turn => {
                    // A completed conversation keeps its Brain summary as the
                    // durable completion signal. Ship already has a stronger
                    // completion card, so avoid stacking both outcomes.
                    if !has_ship_result {
                        rows.push(AgentChatRow::TimelineItem(index));
                    }
                    index += 1;
                }
                _ if hidden_review_checklist_turn => {
                    index += 1;
                }
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant { .. })
                | AgentChatTimelineItem::Message(AgentChatMessage::Thought { .. })
                | AgentChatTimelineItem::WorkLog(_)
                | AgentChatTimelineItem::FileChangeActivity(_)
                    if hidden_background_summary_turn =>
                {
                    index += 1;
                }
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
                AgentChatTimelineItem::WorkLog(_)
                | AgentChatTimelineItem::FileChangeActivity(_) => {
                    let start = index;
                    while let Some(item) = session.timeline.get(index) {
                        match item {
                            AgentChatTimelineItem::WorkLog(entry)
                                if !is_agent_stop_work_log(entry) =>
                            {
                                index += 1;
                            }
                            AgentChatTimelineItem::FileChangeActivity(_) => index += 1,
                            _ => break,
                        }
                    }
                    let has_visible_activity = session.timeline[start..index].iter().any(|item| {
                        matches!(
                            item,
                            AgentChatTimelineItem::WorkLog(entry) if !is_noise_work_log(entry)
                        ) || matches!(
                            item,
                            AgentChatTimelineItem::FileChangeActivity(activity)
                                if !activity.observed && !activity.file.clears_projection && !artifact_filter.is_artifact(&activity.file.path)
                        )
                    });
                    if has_visible_activity {
                        rows.push(AgentChatRow::ActivityGroup { start, end: index });
                    }
                }
                AgentChatTimelineItem::ChangedFiles(summary)
                    if !summary
                        .conversation_files()
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

/// Whether the chat should show its working indicator.
///
/// `session.status` is the primary signal, but it goes stale in exactly the
/// cases the user notices: the machine slept, or the backend stream dropped
/// while the CLI kept working. The agents sidebar has always fallen back to the
/// transcript's own freshness, which is why it kept spinning while the chat
/// showed nothing. In the idle-but-still-writing window the chat now trusts the
/// same evidence. A settled turn (waiting on the user, a ready plan, a failure)
/// is a deliberate stop and never spins.
pub(super) fn chat_shows_activity(
    status: AgentChatStatus,
    agent_finished: bool,
    transcript_is_fresh: bool,
) -> bool {
    if agent_finished {
        return false;
    }
    match status {
        AgentChatStatus::Running | AgentChatStatus::Cancelling => true,
        AgentChatStatus::Idle => transcript_is_fresh,
        AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady | AgentChatStatus::Failed => {
            false
        }
    }
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
            Some(AgentChatTimelineItem::ReviewChecklist(checklist)) => {
                let status = match checklist.status {
                    ReviewChecklistStatus::Pending => 0,
                    ReviewChecklistStatus::Ready => 1,
                    ReviewChecklistStatus::Failed => 2,
                };
                let geometry = status
                    + (checklist.items.len() as u64).wrapping_mul(8)
                    + u64::from(checklist.expanded).wrapping_mul(4);
                mix(7, *index, geometry)
            }
            Some(AgentChatTimelineItem::CodeReview(review)) if review.structured.is_some() => {
                // Reports can add findings, limitations or a terminal notice
                // without changing the row count or any user action. Remeasure
                // this one card so new content is not clipped by its old height.
                let revision = review.structured.as_ref().unwrap().revision;
                mix(8, *index, revision.wrapping_mul(2) + u64::from(review.expanded))
            }
            // Cards only change height through an explicit user action (expand,
            // collapse, apply), and every one of those paths already calls
            // `remeasure_agent_chat_list`.
            _ => mix(3, *index, 0),
        },
        AgentChatRow::ActivityGroup { start, end } => mix(4, *start, *end as u64),
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
            AgentChatRow::TimelineItem(index) => {
                session.timeline.get(*index).is_some_and(|item| match item {
                    AgentChatTimelineItem::Message(AgentChatMessage::User { .. }) => true,
                    AgentChatTimelineItem::AgentMessage(card) => card.target_agent_id.is_none(),
                    _ => false,
                })
            }
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
    use crate::state::agent_chat::{
        ChangedFilesSummary, FileChangeActivity, FileChangeStat, WorkLogEntry,
    };

    fn session_with_timeline(timeline: Vec<AgentChatTimelineItem>) -> AgentChatSession {
        AgentChatSession {
            agent_id: Uuid::nil(),
            title: "Agent".to_string(),
            chat_session_id: None,
            cli_session_id: None,
            hidden_from_notifications: false,
            is_compacting: false,
            status: AgentChatStatus::Idle,
            interaction_mode: AgentInteractionMode::Default,
            composer_text: String::new(),
            messages: Vec::new(),
            timeline,
            queued_turns: Vec::new(),
            work_log: Vec::new(),
            pending_user_input: None,
            pending_approval: None,
            proposed_plan: None,
            latest_plan: None,
            changed_files: ChangedFilesSummary::default(),
            usage: None,
            started_running_at: None,
            last_activity_at: 0,
        }
    }

    #[test]
    fn live_file_changes_stay_in_one_collapsed_activity_group() {
        let work = |id: &str| {
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                id,
                id,
                WorkLogEntryKind::Tool,
                "Tool",
                WorkLogStatus::Completed,
            ))
        };
        let session = session_with_timeline(vec![
            work("before"),
            AgentChatTimelineItem::FileChangeActivity(FileChangeActivity::new(
                "edit:index",
                "turn-a",
                FileChangeStat::new("index.html", 23, 34),
                false,
                10,
            )),
            work("after"),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        let rows = agent_chat_rows(&session, false, false, &filter);

        assert!(matches!(
            rows.as_slice(),
            [AgentChatRow::ActivityGroup { start: 0, end: 3 }]
        ));
    }

    #[test]
    fn user_messages_still_separate_activity_groups() {
        let work = |id: &str| {
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                id,
                id,
                WorkLogEntryKind::Tool,
                "Tool",
                WorkLogStatus::Completed,
            ))
        };
        let session = session_with_timeline(vec![
            work("before"),
            AgentChatTimelineItem::FileChangeActivity(FileChangeActivity::new(
                "edit:index",
                "turn-a",
                FileChangeStat::new("index.html", 23, 34),
                false,
                10,
            )),
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: "next turn".to_string(),
                display_text: None,
                tags: Vec::new(),
                created_at: 11,
            }),
            work("after"),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        let rows = agent_chat_rows(&session, false, false, &filter);

        assert!(matches!(
            rows.as_slice(),
            [
                AgentChatRow::ActivityGroup { start: 0, end: 2 },
                AgentChatRow::TimelineItem(2),
                AgentChatRow::ActivityGroup { start: 3, end: 4 }
            ]
        ));
    }

    #[test]
    fn assistant_messages_also_separate_activity_groups() {
        let work = |id: &str| {
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                id,
                id,
                WorkLogEntryKind::Command,
                "Command",
                WorkLogStatus::Completed,
            ))
        };
        let session = session_with_timeline(vec![
            work("before"),
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id: Some("message-1".to_string()),
                text: "Progress update".to_string(),
                created_at: 11,
            }),
            work("after"),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        let rows = agent_chat_rows(&session, false, false, &filter);

        assert!(matches!(
            rows.as_slice(),
            [
                AgentChatRow::ActivityGroup { start: 0, end: 1 },
                AgentChatRow::TimelineItem(1),
                AgentChatRow::ActivityGroup { start: 2, end: 3 }
            ]
        ));
    }

    #[test]
    fn generic_reasoning_and_duplicate_file_change_steps_are_hidden() {
        let session = session_with_timeline(vec![
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                "reasoning-1",
                "reasoning-1",
                WorkLogEntryKind::Step,
                "Reasoning",
                WorkLogStatus::Completed,
            )),
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                "file-change-1",
                "file-change-1",
                WorkLogEntryKind::Tool,
                "File change",
                WorkLogStatus::Completed,
            )),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        assert!(agent_chat_rows(&session, false, false, &filter).is_empty());
    }

    #[test]
    fn newest_turns_reverse_but_each_turn_keeps_reading_order() {
        let (order, newest_len) =
            newest_turn_first_order(&[true, false, false, true, false, true, false, false]);

        assert_eq!(order, vec![5, 6, 7, 3, 4, 0, 1, 2]);
        assert_eq!(newest_len, 3);
    }

    fn teammate_card(kind: &str, outgoing: bool) -> AgentChatTimelineItem {
        AgentChatTimelineItem::AgentMessage(crate::state::agent_chat::AgentMessageCard {
            id: Uuid::new_v4(),
            source_agent_id: Uuid::new_v4(),
            source_title: "Teammate".to_string(),
            target_agent_id: outgoing.then(Uuid::new_v4),
            target_title: outgoing.then(|| "Recipient".to_string()),
            text: "Context from the teammate".to_string(),
            kind: kind.to_string(),
            created_at: 2,
        })
    }

    fn user_row(text: &str) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Message(AgentChatMessage::User {
            text: text.to_string(),
            display_text: None,
            tags: Vec::new(),
            created_at: 1,
        })
    }

    #[test]
    fn received_teammate_cards_stay_with_their_answer_in_both_layouts() {
        for (kind, prompt) in [
            ("ask", "<choro-agent-request>\nRequest id: question"),
            ("delegate", "<choro-agent-request>\nRequest id: task"),
            ("reply", "<!-- choro:teammate-result -->\nResult"),
        ] {
            let session = session_with_timeline(vec![
                user_row("Original user request"),
                teammate_card("ask", true),
                teammate_card(kind, false),
                user_row(prompt),
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id: None,
                    text: "Using the teammate's context".to_string(),
                    created_at: 3,
                }),
            ]);
            let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));
            let rows = agent_chat_rows(&session, false, false, &filter);
            assert!(matches!(
                rows.as_slice(),
                [
                    AgentChatRow::TimelineItem(0),
                    AgentChatRow::TimelineItem(1),
                    AgentChatRow::TimelineItem(2),
                    AgentChatRow::TimelineItem(4),
                ]
            ));
            assert_eq!(
                agent_chat_display_order(&rows, &session, false).0,
                vec![0, 1, 2, 3]
            );
            assert_eq!(
                agent_chat_display_order(&rows, &session, true),
                (vec![2, 3, 0, 1], 2)
            );
        }
    }

    #[test]
    fn teammate_receipts_are_visible_during_hidden_checklist_turns() {
        for outgoing in [false, true] {
            let session = session_with_timeline(vec![
                user_row(REVIEW_CHECKLIST_REQUEST_MARKER),
                teammate_card("reply", outgoing),
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id: None,
                    text: "Hidden checklist maintenance".to_string(),
                    created_at: 3,
                }),
                user_row("<!-- choro:teammate-result -->\nResult"),
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id: None,
                    text: "Visible continuation".to_string(),
                    created_at: 4,
                }),
            ]);
            let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));
            assert!(matches!(
                agent_chat_rows(&session, false, false, &filter).as_slice(),
                [AgentChatRow::TimelineItem(1), AgentChatRow::TimelineItem(4)]
            ));
        }
    }

    #[test]
    fn independent_review_outcomes_survive_hidden_checklist_turns() {
        use ide_core::code_review::{ReviewRun, ReviewRunState};
        for state in [ReviewRunState::Running, ReviewRunState::Complete,
            ReviewRunState::Partial, ReviewRunState::Failed, ReviewRunState::Cancelled] {
            let mut run = ReviewRun::new(Uuid::new_v4(), Uuid::new_v4(),
                "Codex".into(), "model".into(), "High".into(), 1);
            run.state = state;
            let session = session_with_timeline(vec![
                user_row(REVIEW_CHECKLIST_REQUEST_MARKER),
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id: None, text: "Hidden maintenance output".into(), created_at: 2,
                }),
                AgentChatTimelineItem::CodeReview(
                    crate::state::agent_chat::CodeReview::from_run(run)),
            ]);
            let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));
            assert!(matches!(agent_chat_rows(&session, false, false, &filter).as_slice(),
                [AgentChatRow::TimelineItem(2)]), "{state:?}");
        }
    }

    #[test]
    fn structured_review_updates_remeasure_the_card_without_new_rows() {
        use ide_core::code_review::ReviewRun;
        let run = ReviewRun::new(Uuid::new_v4(), Uuid::new_v4(),
            "Codex".into(), "model".into(), "High".into(), 1);
        let mut session = session_with_timeline(vec![AgentChatTimelineItem::CodeReview(
            crate::state::agent_chat::CodeReview::from_run(run))]);
        let row = AgentChatRow::TimelineItem(0);
        let before = agent_chat_row_fingerprint(&row, &session, None);
        let AgentChatTimelineItem::CodeReview(card) = &mut session.timeline[0] else { unreachable!() };
        let run = card.structured.as_mut().unwrap();
        run.revision += 1;
        run.limitations.push("Provider startup failed".into());
        let after = agent_chat_row_fingerprint(&row, &session, None);
        assert_ne!(before, after);
        assert_eq!(session.timeline.len(), 1);
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

    #[test]
    fn checklist_checkbox_updates_do_not_remeasure_the_virtualized_row() {
        let checklist =
            ReviewChecklist::ready("turn-1", "- Open Settings — The option is visible", 1);
        let mut session =
            session_with_timeline(vec![AgentChatTimelineItem::ReviewChecklist(checklist)]);
        let row = AgentChatRow::TimelineItem(0);
        let before = agent_chat_row_fingerprint(&row, &session, None);

        let AgentChatTimelineItem::ReviewChecklist(checklist) = &mut session.timeline[0] else {
            unreachable!();
        };
        checklist.items[0].checked = true;
        let checked = agent_chat_row_fingerprint(&row, &session, None);
        assert_eq!(before, checked);

        let AgentChatTimelineItem::ReviewChecklist(checklist) = &mut session.timeline[0] else {
            unreachable!();
        };
        checklist.expanded = false;
        assert_ne!(checked, agent_chat_row_fingerprint(&row, &session, None));
    }

    #[test]
    fn checklist_maintenance_transcript_is_hidden_while_the_card_remains_visible() {
        let session = session_with_timeline(vec![
            AgentChatTimelineItem::ReviewChecklist(ReviewChecklist::pending("turn-1", 1)),
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: format!("{REVIEW_CHECKLIST_REQUEST_MARKER}\nSource turn: turn-1"),
                display_text: None,
                tags: Vec::new(),
                created_at: 2,
            }),
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id: Some("maintenance-output".to_string()),
                text: "<review_checklist>partial".to_string(),
                created_at: 3,
            }),
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: "next request".to_string(),
                display_text: None,
                tags: Vec::new(),
                created_at: 4,
            }),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        assert!(matches!(
            agent_chat_rows(&session, false, false, &filter).as_slice(),
            [AgentChatRow::TimelineItem(0), AgentChatRow::TimelineItem(3)]
        ));
    }

    #[test]
    fn completed_background_summary_card_remains_visible() {
        let session = session_with_timeline(vec![
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: format!(
                    "{}\n{}\nUpdate the summary.",
                    super::super::agent_chat_brain::SUMMARY_REQUEST_MARKER,
                    super::super::agent_chat_brain::BACKGROUND_SUMMARY_REQUEST_MARKER,
                ),
                display_text: Some("Update Choro Brain summary".to_string()),
                tags: Vec::new(),
                created_at: 1,
            }),
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                "summary-save",
                "summary-save",
                WorkLogEntryKind::Tool,
                "Saved Brain summary",
                WorkLogStatus::Completed,
            )),
            AgentChatTimelineItem::AgentSummary(crate::state::agent_chat::AgentSummaryCard {
                summary_text: "Completed the sidebar improvement.".to_string(),
                last_summarized_sequence: 4,
                updated_at: 2,
                edited_by_user: false,
                expanded: false,
            }),
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id: Some("summary-response".to_string()),
                text: "Saved the Brain summary.".to_string(),
                created_at: 3,
            }),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        assert!(matches!(
            agent_chat_rows(&session, false, false, &filter).as_slice(),
            [AgentChatRow::TimelineItem(2)]
        ));
    }

    #[test]
    fn background_brain_transcript_is_hidden_while_ship_stays_last() {
        let session = session_with_timeline(vec![
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: format!(
                    "{}\n{}\nUpdate the summary.",
                    super::super::agent_chat_brain::SUMMARY_REQUEST_MARKER,
                    super::super::agent_chat_brain::BACKGROUND_SUMMARY_REQUEST_MARKER,
                ),
                display_text: Some("Update Choro Brain summary".to_string()),
                tags: Vec::new(),
                created_at: 1,
            }),
            AgentChatTimelineItem::ShipResult(crate::state::agent_chat::ShipResult {
                id: "feature:abc123".to_string(),
                action: "Commit + push + PR".to_string(),
                repository: None,
                branch: "feature".to_string(),
                pr_base_branch: Some("dev".to_string()),
                commit_sha: "abc123".to_string(),
                pr_url: Some("https://github.com/acme/app/pull/1".to_string()),
                pr_title: Some("Ship the fix".to_string()),
                pr_body: None,
                created_at: 2,
                task: None,
                suggested_status: None,
                applied: None,
            }),
            AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
                "summary-save",
                "summary-save",
                WorkLogEntryKind::Tool,
                "Saved Brain summary",
                WorkLogStatus::Completed,
            )),
            AgentChatTimelineItem::AgentSummary(crate::state::agent_chat::AgentSummaryCard {
                summary_text: "Shipped the sidebar improvement.".to_string(),
                last_summarized_sequence: 4,
                updated_at: 2,
                edited_by_user: false,
                expanded: false,
            }),
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id: Some("summary-response".to_string()),
                text: "Saved the Brain summary.".to_string(),
                created_at: 3,
            }),
        ]);
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        assert!(matches!(
            agent_chat_rows(&session, false, false, &filter).as_slice(),
            [AgentChatRow::TimelineItem(1)]
        ));
    }
}
