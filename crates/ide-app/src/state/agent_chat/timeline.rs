use super::*;

#[cfg(test)]
mod stream_routing_tests {
    use super::*;

    fn reply(id: &str, text: &str) -> AgentChatMessage {
        AgentChatMessage::Assistant {
            message_id: Some(id.into()),
            text: text.into(),
            created_at: 1,
        }
    }

    #[test]
    fn late_receipt_uses_persisted_user_anchor_without_any_file_activity() {
        let user = |text: &str, time| {
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: text.into(),
                display_text: None,
                tags: vec![],
                created_at: time,
            })
        };
        let original = user("original", 1);
        let key = super::super::persistence::stored_timeline_event_parts(&original)
            .unwrap()
            .1
            .unwrap();
        let turn = format!("unique-turn|{key}");
        let mut receipt = ChangedFilesSummary::attributed(
            &turn,
            vec![FileChangeStat::new("file.rs", 1, 0)],
            vec![],
        );
        receipt.attribution_version = 2;
        let marker = AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
            format!("file-receipt:{turn}"),
            format!("file-receipt:{turn}"),
            WorkLogEntryKind::System,
            "Changes recorded",
            WorkLogStatus::Completed,
        ));
        // Persisted writes may finish out of order. Neither activity nor the
        // original in-memory marker is needed to find the source message.
        let mut timeline = vec![
            AgentChatTimelineItem::ChangedFiles(receipt),
            original,
            user("newer", 2),
            marker,
        ];
        place_change_receipts(&mut timeline);
        assert!(
            matches!(&timeline[0], AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) if text == "original")
        );
        assert!(matches!(
            &timeline[2],
            AgentChatTimelineItem::ChangedFiles(_)
        ));
        assert!(
            matches!(&timeline[3], AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) if text == "newer")
        );
    }

    #[test]
    fn interleaved_replies_keep_complete_text_in_messages_and_timeline() {
        let mut messages = Vec::new();
        let mut timeline = Vec::new();
        for chunk in [
            reply("review", "Review "),
            reply("main", "I am "),
            reply("review", "complete."),
            reply("main", "fixing it."),
        ] {
            let stored = append_or_extend_message(&mut messages, chunk.clone());
            let visible = append_or_extend_timeline_message(&mut timeline, chunk);
            assert_eq!(
                stored, visible,
                "persisted text and displayed text must agree"
            );
        }
        assert_eq!(
            messages,
            vec![
                reply("review", "Review complete."),
                reply("main", "I am fixing it.")
            ]
        );
        assert_eq!(timeline.len(), 2);
    }

    #[test]
    fn intervening_tool_event_does_not_split_a_streamed_reply() {
        let mut timeline = Vec::new();
        append_or_extend_timeline_message(&mut timeline, reply("main", "Checking "));
        timeline.push(AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
            "tool",
            "tool",
            WorkLogEntryKind::System,
            "Check",
            WorkLogStatus::Completed,
        )));
        let merged = append_or_extend_timeline_message(&mut timeline, reply("main", "the result."));
        assert_eq!(merged, reply("main", "Checking the result."));
        assert_eq!(timeline.len(), 2);
        assert!(matches!(timeline[1], AgentChatTimelineItem::WorkLog(_)));
    }

    #[test]
    fn reused_ids_do_not_merge_across_user_turns_or_message_roles() {
        let first = reply("reused", "First turn");
        let user = AgentChatMessage::User {
            text: "Next turn".into(),
            display_text: None,
            tags: Vec::new(),
            created_at: 2,
        };
        let thought = AgentChatMessage::Thought {
            message_id: Some("reused".into()),
            text: "Thinking".into(),
            created_at: 3,
        };
        let second = reply("reused", "Second turn");
        let mut messages = Vec::new();
        let mut timeline = Vec::new();
        for message in [first.clone(), user, thought, second.clone()] {
            append_or_extend_message(&mut messages, message.clone());
            append_or_extend_timeline_message(&mut timeline, message);
        }
        assert_eq!(messages.len(), 4);
        assert_eq!(timeline.len(), 4);
        assert_eq!(messages[0], first);
        assert_eq!(messages[3], second);
    }

    #[test]
    fn anonymous_fragments_are_not_guessed_into_one_message() {
        let mut messages = Vec::new();
        let mut timeline = Vec::new();
        for text in ["One", "Two"] {
            let message = AgentChatMessage::Assistant {
                message_id: None,
                text: text.into(),
                created_at: 1,
            };
            append_or_extend_message(&mut messages, message.clone());
            append_or_extend_timeline_message(&mut timeline, message);
        }
        assert_eq!(messages.len(), 2);
        assert_eq!(timeline.len(), 2);
    }
}

/// Match the provider's message identity, not arrival adjacency: concurrent
/// streams and tool events can arrive between two chunks of the same reply.
fn extend_identified_stream(existing: &mut AgentChatMessage, incoming: &AgentChatMessage) -> bool {
    match (existing, incoming) {
        (
            AgentChatMessage::Assistant {
                message_id, text, ..
            },
            AgentChatMessage::Assistant {
                message_id: next_id,
                text: next,
                ..
            },
        )
        | (
            AgentChatMessage::Thought {
                message_id, text, ..
            },
            AgentChatMessage::Thought {
                message_id: next_id,
                text: next,
                ..
            },
        ) if same_stream_id(message_id, next_id) && !next.is_empty() => {
            text.push_str(next);
            true
        }
        _ => false,
    }
}

pub(super) fn append_or_extend_message(
    messages: &mut Vec<AgentChatMessage>,
    message: AgentChatMessage,
) -> AgentChatMessage {
    for existing in messages.iter_mut().rev() {
        // Provider ids are scoped to the current user turn. Unidentified
        // messages and separate roles remain separate, as before.
        if matches!(existing, AgentChatMessage::User { .. }) {
            break;
        }
        if extend_identified_stream(existing, &message) {
            return existing.clone();
        }
    }
    messages.push(message.clone());
    message
}

pub(super) fn upsert_work_log_entry(entries: &mut Vec<WorkLogEntry>, entry: WorkLogEntry) {
    if let Some(existing) = entries
        .iter_mut()
        .rev()
        .find(|existing| existing.collapse_key == entry.collapse_key)
    {
        existing.merge(entry);
    } else {
        entries.push(entry);
    }
}

pub(super) fn append_or_extend_timeline_message(
    timeline: &mut Vec<AgentChatTimelineItem>,
    message: AgentChatMessage,
) -> AgentChatMessage {
    for item in timeline.iter_mut().rev() {
        if let AgentChatTimelineItem::Message(existing) = item {
            if matches!(existing, AgentChatMessage::User { .. }) {
                break;
            }
            if extend_identified_stream(existing, &message) {
                return existing.clone();
            }
        }
    }
    timeline.push(AgentChatTimelineItem::Message(message.clone()));
    message
}

pub(super) fn upsert_timeline_work_log(
    timeline: &mut Vec<AgentChatTimelineItem>,
    entry: WorkLogEntry,
) {
    if let Some(AgentChatTimelineItem::WorkLog(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::WorkLog(existing) => existing.collapse_key == entry.collapse_key,
            _ => false,
        })
    {
        existing.merge(entry);
    } else {
        timeline.push(AgentChatTimelineItem::WorkLog(entry));
    }
}

pub(super) fn upsert_timeline_file_change_activity(
    timeline: &mut Vec<AgentChatTimelineItem>,
    activity: FileChangeActivity,
) {
    if let Some(AgentChatTimelineItem::FileChangeActivity(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::FileChangeActivity(existing) => existing.id == activity.id && existing.turn_id == activity.turn_id,
            _ => false,
        })
    {
        *existing = activity;
    } else {
        let target = receipt_position(timeline, &activity.turn_id);
        timeline.insert(target, AgentChatTimelineItem::FileChangeActivity(activity));
    }
}

pub(super) fn upsert_timeline_pending_user_input(
    timeline: &mut Vec<AgentChatTimelineItem>,
    pending: PendingUserInput,
) {
    if let Some(AgentChatTimelineItem::PendingUserInput(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::PendingUserInput(existing) => {
                existing.request_id == pending.request_id
            }
            _ => false,
        })
    {
        *existing = pending;
    } else {
        timeline.push(AgentChatTimelineItem::PendingUserInput(pending));
    }
}

pub(super) fn upsert_timeline_proposed_plan(
    timeline: &mut Vec<AgentChatTimelineItem>,
    plan: ProposedPlan,
) {
    if let Some(AgentChatTimelineItem::ProposedPlan(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::ProposedPlan(existing) => existing.id == plan.id,
            _ => false,
        })
    {
        *existing = plan;
    } else {
        timeline.push(AgentChatTimelineItem::ProposedPlan(plan));
    }
}

pub(super) fn upsert_timeline_code_review(
    timeline: &mut Vec<AgentChatTimelineItem>,
    review: CodeReview,
) {
    if let Some(AgentChatTimelineItem::CodeReview(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::CodeReview(existing) => existing.id == review.id,
            _ => false,
        })
    {
        *existing = review;
    } else {
        timeline.push(AgentChatTimelineItem::CodeReview(review));
    }
}

pub(super) fn upsert_timeline_verification(
    timeline: &mut Vec<AgentChatTimelineItem>,
    verification: Verification,
) {
    if let Some(AgentChatTimelineItem::Verification(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::Verification(existing) => existing.id == verification.id,
            _ => false,
        })
    {
        *existing = verification;
    } else {
        timeline.push(AgentChatTimelineItem::Verification(verification));
    }
}

pub(super) fn upsert_timeline_review_checklist(
    timeline: &mut Vec<AgentChatTimelineItem>,
    checklist: ReviewChecklist,
) {
    if let Some(AgentChatTimelineItem::ReviewChecklist(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::ReviewChecklist(existing) => {
                existing.source_turn_id == checklist.source_turn_id
            }
            _ => false,
        })
    {
        *existing = checklist;
    } else {
        timeline.push(AgentChatTimelineItem::ReviewChecklist(checklist));
    }
}

pub(super) fn upsert_timeline_orbit_update(
    timeline: &mut Vec<AgentChatTimelineItem>,
    card: OrbitUpdateCard,
) {
    if let Some(AgentChatTimelineItem::OrbitUpdate(existing)) =
        timeline.iter_mut().rev().find(|item| match item {
            AgentChatTimelineItem::OrbitUpdate(existing) => {
                existing.invocation_id == card.invocation_id
            }
            _ => false,
        })
    {
        *existing = card;
    } else {
        timeline.push(AgentChatTimelineItem::OrbitUpdate(card));
    }
}

fn receipt_position(timeline: &[AgentChatTimelineItem], turn_id: &str) -> usize {
    let position = if let Some((_, source)) = turn_id.split_once('|') {
        let position = timeline.iter().position(|item| {
            matches!(
                item,
                AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
            ) && super::persistence::stored_timeline_event_parts(item)
                .and_then(|(_, key, _, _)| key)
                .as_deref()
                == Some(source)
        });
        // An older source outside this loaded page belongs before this page.
        let Some(position) = position else {
            return 0;
        };
        Some(position)
    } else {
        timeline.iter().position(|item| matches!(item, AgentChatTimelineItem::WorkLog(entry) if entry.id == format!("file-receipt:{turn_id}")))
            .or_else(|| timeline.iter().position(|item| matches!(item, AgentChatTimelineItem::FileChangeActivity(a) if a.turn_id == turn_id)))
    };
    position
        .and_then(|p| {
            timeline
                .iter()
                .enumerate()
                .skip(p + 1)
                .find(|(_, item)| {
                    matches!(
                        item,
                        AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
                    )
                })
                .map(|(i, _)| i)
        })
        .unwrap_or(timeline.len())
}

pub(super) fn append_timeline_changed_files(
    timeline: &mut Vec<AgentChatTimelineItem>,
    summary: ChangedFilesSummary,
) {
    if let Some(turn_id) = summary.turn_id.as_deref() {
        if let Some(existing) = timeline.iter_mut().find_map(|item| match item {
            AgentChatTimelineItem::ChangedFiles(existing)
                if existing.turn_id.as_deref() == Some(turn_id) =>
            {
                Some(existing)
            }
            _ => None,
        }) {
            *existing = summary;
            return;
        }
        let target = receipt_position(timeline, turn_id);
        timeline.insert(target, AgentChatTimelineItem::ChangedFiles(summary));
        return;
    }
    timeline.push(AgentChatTimelineItem::ChangedFiles(summary));
}

/// Persistence order follows completion order. Re-anchor delayed receipts
/// when history pages are hydrated, including after an application restart.
pub(crate) fn place_change_receipts(timeline: &mut Vec<AgentChatTimelineItem>) {
    let mut receipts = Vec::new();
    let mut markers = Vec::new();
    for item in std::mem::take(timeline) {
        match item {
            AgentChatTimelineItem::ChangedFiles(summary) if summary.attribution_version >= 2 => {
                receipts.push(summary)
            }
            AgentChatTimelineItem::WorkLog(mut entry) if entry.id.starts_with("file-receipt:") => {
                if entry.status == WorkLogStatus::InProgress {
                    entry.status = WorkLogStatus::Failed;
                    entry.title = "Change recording was interrupted".into();
                }
                markers.push(entry);
            }
            other => timeline.push(other),
        }
    }
    for entry in markers {
        let target = receipt_position(timeline, &entry.id["file-receipt:".len()..]);
        timeline.insert(target, AgentChatTimelineItem::WorkLog(entry));
    }
    for receipt in receipts {
        append_timeline_changed_files(timeline, receipt);
    }
}

pub(super) fn remove_proposed_plan_blocks_from_timeline(timeline: &mut Vec<AgentChatTimelineItem>) {
    timeline.retain_mut(|item| match item {
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant { text, .. }) => {
            *text = strip_tagged_blocks(text, "proposed_plan");
            !text.trim().is_empty()
        }
        AgentChatTimelineItem::ProposedPlan(_) => false,
        _ => true,
    });
}

#[cfg(test)]
mod file_activity_tests {
    use super::*;

    #[test]
    fn delayed_receipts_return_to_the_original_turn_during_hydration() {
        let activity = FileChangeActivity::new(
            "edit",
            "old-turn",
            FileChangeStat::new("shared.rs", 1, 1),
            false,
            1,
        );
        let mut receipt =
            ChangedFilesSummary::attributed("old-turn", vec![activity.file.clone()], vec![]);
        receipt.attribution_version = 2;
        let mut timeline = vec![
            AgentChatTimelineItem::FileChangeActivity(activity),
            AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: "next request".into(),
                display_text: None,
                tags: vec![],
                created_at: 2,
            }),
            AgentChatTimelineItem::ChangedFiles(receipt.clone()),
        ];
        place_change_receipts(&mut timeline);
        assert!(
            matches!(&timeline[1],AgentChatTimelineItem::ChangedFiles(r) if r.turn_id.as_deref()==Some("old-turn"))
        );
        assert!(matches!(
            &timeline[2],
            AgentChatTimelineItem::Message(AgentChatMessage::User { .. })
        ));
        receipt.snapshot_id = Some(uuid::Uuid::new_v4());
        append_timeline_changed_files(&mut timeline, receipt.clone());
        assert_eq!(timeline.len(), 3);
        assert!(
            matches!(&timeline[1],AgentChatTimelineItem::ChangedFiles(r) if r.snapshot_id==receipt.snapshot_id)
        );
    }

    #[test]
    fn streaming_updates_replace_only_the_same_live_file_row() {
        let mut timeline = vec![AgentChatTimelineItem::WorkLog(WorkLogEntry::new(
            "tool",
            "tool",
            WorkLogEntryKind::Tool,
            "Editing",
            WorkLogStatus::InProgress,
        ))];
        upsert_timeline_file_change_activity(
            &mut timeline,
            FileChangeActivity::new(
                "edit:index",
                "turn-a",
                FileChangeStat::new("index.html", 1, 0),
                false,
                10,
            ),
        );
        upsert_timeline_file_change_activity(
            &mut timeline,
            FileChangeActivity::new(
                "edit:index",
                "turn-a",
                FileChangeStat::new("index.html", 23, 34),
                false,
                11,
            ),
        );

        assert_eq!(timeline.len(), 2);
        let AgentChatTimelineItem::FileChangeActivity(activity) = &timeline[1] else {
            panic!("file activity should remain its own timeline row");
        };
        assert_eq!(activity.file.additions, 23);
        assert_eq!(activity.file.deletions, 34);
    }

    #[test]
    fn orbit_completion_is_idempotent_per_invocation() {
        let invocation_id = Uuid::new_v4();
        let module_id = Uuid::new_v4();
        let mut timeline = Vec::new();
        let card = OrbitUpdateCard {
            invocation_id,
            module_id,
            module_name: "Analytics".into(),
            inserted: 1,
            updated: 0,
            deleted: 0,
            undone: false,
            created_at: 10,
        };
        upsert_timeline_orbit_update(&mut timeline, card);
        upsert_timeline_orbit_update(
            &mut timeline,
            OrbitUpdateCard {
                invocation_id,
                module_id,
                module_name: "Analytics".into(),
                inserted: 1,
                updated: 2,
                deleted: 0,
                undone: false,
                created_at: 10,
            },
        );

        assert_eq!(timeline.len(), 1);
        let AgentChatTimelineItem::OrbitUpdate(card) = &timeline[0] else {
            panic!("Orbit completion should remain one timeline card");
        };
        assert_eq!(card.updated, 2);
    }
}

pub(super) fn remove_code_review_blocks(messages: &mut Vec<AgentChatMessage>) {
    messages.retain_mut(|message| match message {
        AgentChatMessage::Assistant { text, .. } => {
            *text = strip_tagged_blocks(text, "code_review");
            !text.trim().is_empty()
        }
        _ => true,
    });
}

pub(super) fn remove_code_review_blocks_from_timeline(timeline: &mut Vec<AgentChatTimelineItem>) {
    timeline.retain_mut(|item| match item {
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant { text, .. }) => {
            *text = strip_tagged_blocks(text, "code_review");
            !text.trim().is_empty()
        }
        AgentChatTimelineItem::CodeReview(_) => false,
        _ => true,
    });
}

pub(super) fn remove_verification_blocks(messages: &mut Vec<AgentChatMessage>) {
    messages.retain_mut(|message| match message {
        AgentChatMessage::Assistant { text, .. } => {
            *text = strip_tagged_blocks(text, "verification");
            !text.trim().is_empty()
        }
        _ => true,
    });
}

pub(super) fn remove_verification_blocks_from_timeline(timeline: &mut Vec<AgentChatTimelineItem>) {
    timeline.retain_mut(|item| match item {
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant { text, .. }) => {
            *text = strip_tagged_blocks(text, "verification");
            !text.trim().is_empty()
        }
        _ => true,
    });
}

pub(super) fn remove_review_checklist_blocks(messages: &mut Vec<AgentChatMessage>) {
    messages.retain_mut(|message| match message {
        AgentChatMessage::Assistant { text, .. } => {
            *text = strip_tagged_blocks(text, "review_checklist");
            !text.trim().is_empty()
        }
        _ => true,
    });
}

pub(super) fn remove_review_checklist_blocks_from_timeline(
    timeline: &mut Vec<AgentChatTimelineItem>,
) {
    timeline.retain_mut(|item| match item {
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant { text, .. }) => {
            *text = strip_tagged_blocks(text, "review_checklist");
            !text.trim().is_empty()
        }
        _ => true,
    });
}

pub(super) fn same_stream_id(left: &Option<String>, right: &Option<String>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        _ => false,
    }
}
