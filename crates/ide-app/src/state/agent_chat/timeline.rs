use super::*;

pub(super) fn append_or_extend_message(
    messages: &mut Vec<AgentChatMessage>,
    message: AgentChatMessage,
) -> AgentChatMessage {
    match (messages.last_mut(), message) {
        (
            Some(AgentChatMessage::Assistant {
                message_id, text, ..
            }),
            AgentChatMessage::Assistant {
                message_id: next_id,
                text: next,
                ..
            },
        ) if same_stream_id(message_id, &next_id) && !next.is_empty() => {
            text.push_str(&next);
            messages.last().cloned().expect("merged message exists")
        }
        (
            Some(AgentChatMessage::Thought {
                message_id, text, ..
            }),
            AgentChatMessage::Thought {
                message_id: next_id,
                text: next,
                ..
            },
        ) if same_stream_id(message_id, &next_id) && !next.is_empty() => {
            text.push_str(&next);
            messages.last().cloned().expect("merged message exists")
        }
        (_, message) => {
            messages.push(message);
            messages.last().cloned().expect("pushed message exists")
        }
    }
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
    match (timeline.last_mut(), message) {
        (
            Some(AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id,
                text,
                ..
            })),
            AgentChatMessage::Assistant {
                message_id: next_id,
                text: next,
                ..
            },
        ) if same_stream_id(message_id, &next_id) && !next.is_empty() => {
            text.push_str(&next);
            match timeline.last() {
                Some(AgentChatTimelineItem::Message(message)) => message.clone(),
                _ => unreachable!("merged timeline message exists"),
            }
        }
        (
            Some(AgentChatTimelineItem::Message(AgentChatMessage::Thought {
                message_id,
                text,
                ..
            })),
            AgentChatMessage::Thought {
                message_id: next_id,
                text: next,
                ..
            },
        ) if same_stream_id(message_id, &next_id) && !next.is_empty() => {
            text.push_str(&next);
            match timeline.last() {
                Some(AgentChatTimelineItem::Message(message)) => message.clone(),
                _ => unreachable!("merged timeline message exists"),
            }
        }
        (_, message) => {
            timeline.push(AgentChatTimelineItem::Message(message));
            match timeline.last() {
                Some(AgentChatTimelineItem::Message(message)) => message.clone(),
                _ => unreachable!("pushed timeline message exists"),
            }
        }
    }
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
            AgentChatTimelineItem::FileChangeActivity(existing) => existing.id == activity.id,
            _ => false,
        })
    {
        *existing = activity;
    } else {
        timeline.push(AgentChatTimelineItem::FileChangeActivity(activity));
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

pub(super) fn append_timeline_changed_files(
    timeline: &mut Vec<AgentChatTimelineItem>,
    summary: ChangedFilesSummary,
) {
    if timeline
        .iter()
        .rev()
        .take_while(|item| !matches!(item, AgentChatTimelineItem::Message(AgentChatMessage::User { .. })))
        .any(|item| matches!(item, AgentChatTimelineItem::ChangedFiles(existing) if existing == &summary))
    {
        return;
    }
    if let Some(AgentChatTimelineItem::ChangedFiles(existing)) = timeline.last_mut() {
        *existing = summary;
        return;
    }
    timeline.push(AgentChatTimelineItem::ChangedFiles(summary));
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
