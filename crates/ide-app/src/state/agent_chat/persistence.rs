use super::*;

pub fn timeline_item_from_store_event(
    event: &StoredTimelineEvent,
) -> Option<AgentChatTimelineItem> {
    let payload = serde_json::from_str::<StoredTimelinePayload>(&event.payload_json).ok()?;
    payload.into_timeline_item()
}

pub fn persist_timeline_snapshot(
    agent_id: Uuid,
    timeline: &[AgentChatTimelineItem],
) -> anyhow::Result<()> {
    let store = LocalStore::open_default()?;
    for item in timeline {
        if let AgentChatTimelineItem::Message(message) = item {
            if let Some((role, text, created_at, backend_message_id)) =
                stored_message_parts(message)
            {
                store.upsert_chat_message(agent_id, role, text, created_at, backend_message_id)?;
            }
        }
        if let Some((kind, event_key, payload_json, created_at)) = stored_timeline_event_parts(item)
        {
            store.upsert_timeline_event(agent_id, kind, event_key, payload_json, created_at)?;
        }
    }
    Ok(())
}

pub(super) fn persist_chat_message(
    agent_id: Uuid,
    message: AgentChatMessage,
    cx: &mut Context<AgentChatState>,
) {
    let timeline_event =
        stored_timeline_event_parts(&AgentChatTimelineItem::Message(message.clone()));
    let Some((role, text, created_at, backend_message_id)) = stored_message_parts(&message) else {
        return;
    };
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = LocalStore::open_default().and_then(|store| {
                    store.upsert_chat_message(
                        agent_id,
                        role,
                        text,
                        created_at,
                        backend_message_id,
                    )?;
                    if let Some((kind, event_key, payload_json, event_created_at)) = timeline_event
                    {
                        store.upsert_timeline_event(
                            agent_id,
                            kind,
                            event_key,
                            payload_json,
                            event_created_at,
                        )?;
                    }
                    Ok(())
                }) {
                    eprintln!("failed to persist chat message: {error:#}");
                }
            })
            .await;
    })
    .detach();
}

pub(super) fn persist_timeline_item(
    agent_id: Uuid,
    item: AgentChatTimelineItem,
    cx: &mut Context<AgentChatState>,
) {
    let Some((kind, event_key, payload_json, created_at)) = stored_timeline_event_parts(&item)
    else {
        return;
    };
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = LocalStore::open_default().and_then(|store| {
                    store
                        .upsert_timeline_event(agent_id, kind, event_key, payload_json, created_at)
                        .map(|_| ())
                }) {
                    eprintln!("failed to persist chat timeline event: {error:#}");
                }
            })
            .await;
    })
    .detach();
}

pub(super) fn stored_message_parts(
    message: &AgentChatMessage,
) -> Option<(String, String, u64, Option<String>)> {
    match message {
        AgentChatMessage::User {
            text, created_at, ..
        } => Some(("user".to_string(), text.clone(), *created_at, None)),
        AgentChatMessage::Assistant {
            message_id,
            text,
            created_at,
        } => Some((
            "assistant".to_string(),
            text.clone(),
            *created_at,
            message_id.clone(),
        )),
        AgentChatMessage::Thought {
            message_id,
            text,
            created_at,
        } => Some((
            "thought".to_string(),
            text.clone(),
            *created_at,
            message_id.clone(),
        )),
    }
}

pub(super) fn stored_timeline_event_parts(
    item: &AgentChatTimelineItem,
) -> Option<(String, Option<String>, String, u64)> {
    let payload = StoredTimelinePayload::from_timeline_item(item)?;
    let kind = payload.kind().to_string();
    let event_key = payload.event_key();
    let created_at = payload.created_at();
    let payload_json = serde_json::to_string(&payload).ok()?;
    Some((kind, event_key, payload_json, created_at))
}

impl StoredTimelinePayload {
    fn from_timeline_item(item: &AgentChatTimelineItem) -> Option<Self> {
        match item {
            AgentChatTimelineItem::Message(message) => {
                let (role, text, created_at, backend_message_id) = stored_message_parts(message)?;
                let (display_text, tags) = match message {
                    AgentChatMessage::User {
                        display_text, tags, ..
                    } => (display_text.clone(), tags.clone()),
                    _ => (None, Vec::new()),
                };
                Some(Self::Message {
                    role,
                    text,
                    display_text,
                    tags,
                    created_at,
                    backend_message_id,
                })
            }
            AgentChatTimelineItem::WorkLog(entry) => Some(Self::WorkLog {
                id: entry.id.clone(),
                collapse_key: entry.collapse_key.clone(),
                kind: work_log_kind_label(entry.kind).to_string(),
                title: entry.title.clone(),
                detail: entry.detail.clone(),
                status: work_log_status_label(entry.status).to_string(),
                started_at: entry.started_at,
                updated_at: entry.updated_at,
                count: entry.count,
            }),
            AgentChatTimelineItem::PendingUserInput(_) => None,
            AgentChatTimelineItem::ProposedPlan(plan) => Some(Self::ProposedPlan {
                id: plan.id.clone(),
                markdown: plan.markdown.clone(),
                expanded: plan.expanded,
                implemented_at: plan.implemented_at,
            }),
            AgentChatTimelineItem::CodeReview(review) => Some(Self::CodeReview {
                id: review.id.clone(),
                markdown: review.markdown.clone(),
                expanded: review.expanded,
            }),
            AgentChatTimelineItem::Verification(verification) => Some(Self::Verification {
                id: verification.id.clone(),
                markdown: verification.markdown.clone(),
                expanded: verification.expanded,
            }),
            AgentChatTimelineItem::ChangedFiles(summary) => Some(Self::ChangedFiles {
                snapshot_id: summary.snapshot_id,
                commit_sha: summary.commit_sha.clone(),
                files: summary
                    .files
                    .iter()
                    .map(|file| StoredFileChange {
                        path: file.path.to_string_lossy().to_string(),
                        additions: file.additions,
                        deletions: file.deletions,
                    })
                    .collect(),
            }),
            AgentChatTimelineItem::Rejoined(card) => Some(Self::Rejoined {
                id: card.id.clone(),
                branch: card.branch.clone(),
                base: card.base.clone(),
                created_at: card.created_at,
            }),
            AgentChatTimelineItem::RejoinConflict(card) => Some(Self::RejoinConflict {
                id: card.id.clone(),
                branch: card.branch.clone(),
                target: card.target.clone(),
                files: card.files.clone(),
                detail: card.detail.clone(),
                created_at: card.created_at,
                requested_at: card.requested_at,
                dismissed_at: card.dismissed_at,
                resolved_at: card.resolved_at,
            }),
            AgentChatTimelineItem::Memorized(card) => Some(Self::Memorized {
                memory_id: card.memory_id,
                text: card.text.clone(),
                global: card.global,
                created_at: card.created_at,
            }),
            AgentChatTimelineItem::MemoryProposal(card) => {
                let (status, memory_id, accepted_global) = match card.status {
                    MemoryProposalStatus::Pending => ("pending", None, None),
                    MemoryProposalStatus::Accepted { memory_id, global } => {
                        ("accepted", Some(memory_id), Some(global))
                    }
                    MemoryProposalStatus::Dismissed => ("dismissed", None, None),
                };
                Some(Self::MemoryProposal {
                    id: card.id.clone(),
                    text: card.text.clone(),
                    why: card.why.clone(),
                    suggested_global: card.suggested_global,
                    status: status.to_string(),
                    memory_id,
                    accepted_global,
                    source: card.source.clone(),
                    created_at: card.created_at,
                })
            }
            AgentChatTimelineItem::ShipResult(result) => Some(Self::ShipResult {
                id: result.id.clone(),
                action: result.action.clone(),
                repository: result.repository.clone(),
                branch: result.branch.clone(),
                pr_base_branch: result.pr_base_branch.clone(),
                commit_sha: result.commit_sha.clone(),
                pr_url: result.pr_url.clone(),
                pr_title: result.pr_title.clone(),
                pr_body: result.pr_body.clone(),
                created_at: result.created_at,
                task: result.task.clone(),
                suggested_status: result.suggested_status.clone(),
                applied: result
                    .applied
                    .as_ref()
                    .map(|applied| StoredShipTaskApplied {
                        commented: applied.commented,
                        status_name: applied.status_name.clone(),
                        at: applied.at,
                    }),
            }),
        }
    }

    fn into_timeline_item(self) -> Option<AgentChatTimelineItem> {
        match self {
            Self::Message {
                role,
                text,
                display_text,
                tags,
                created_at,
                backend_message_id,
            } => {
                let message = match role.as_str() {
                    "user" => AgentChatMessage::User {
                        text,
                        display_text,
                        tags,
                        created_at,
                    },
                    "thought" => AgentChatMessage::Thought {
                        message_id: backend_message_id,
                        text,
                        created_at,
                    },
                    _ => AgentChatMessage::Assistant {
                        message_id: backend_message_id,
                        text,
                        created_at,
                    },
                };
                Some(AgentChatTimelineItem::Message(message))
            }
            Self::WorkLog {
                id,
                collapse_key,
                kind,
                title,
                detail,
                status,
                started_at,
                updated_at,
                count,
            } => Some(AgentChatTimelineItem::WorkLog(WorkLogEntry {
                id,
                collapse_key,
                kind: parse_work_log_kind(&kind),
                title,
                detail,
                status: parse_work_log_status(&status),
                started_at,
                updated_at,
                count,
            })),
            Self::PendingUserInput { .. } => None,
            Self::ProposedPlan {
                id,
                markdown,
                expanded,
                implemented_at,
            } => {
                let mut plan = ProposedPlan::new(id, markdown);
                plan.expanded = expanded;
                plan.implemented_at = implemented_at;
                Some(AgentChatTimelineItem::ProposedPlan(plan))
            }
            Self::CodeReview {
                id,
                markdown,
                expanded,
            } => {
                let mut review = CodeReview::new(id, markdown);
                review.expanded = expanded;
                Some(AgentChatTimelineItem::CodeReview(review))
            }
            Self::Verification {
                id,
                markdown,
                expanded,
            } => {
                let mut verification = Verification::new(id, markdown);
                verification.expanded = expanded;
                Some(AgentChatTimelineItem::Verification(verification))
            }
            Self::ChangedFiles {
                files,
                snapshot_id,
                commit_sha,
            } => Some(AgentChatTimelineItem::ChangedFiles(ChangedFilesSummary {
                snapshot_id,
                commit_sha,
                files: files
                    .into_iter()
                    .map(|file| FileChangeStat::new(file.path, file.additions, file.deletions))
                    .collect(),
            })),
            Self::ShipResult {
                id,
                action,
                repository,
                branch,
                pr_base_branch,
                commit_sha,
                pr_url,
                pr_title,
                pr_body,
                created_at,
                task,
                suggested_status,
                applied,
            } => Some(AgentChatTimelineItem::ShipResult(ShipResult {
                id,
                action,
                repository,
                branch,
                pr_base_branch,
                commit_sha,
                pr_url,
                pr_title,
                pr_body,
                created_at,
                task,
                suggested_status,
                applied: applied.map(|applied| ShipTaskApplied {
                    commented: applied.commented,
                    status_name: applied.status_name,
                    at: applied.at,
                }),
            })),
            Self::Rejoined {
                id,
                branch,
                base,
                created_at,
            } => Some(AgentChatTimelineItem::Rejoined(RejoinedCard {
                id,
                branch,
                base,
                created_at,
            })),
            Self::RejoinConflict {
                id,
                branch,
                target,
                files,
                detail,
                created_at,
                requested_at,
                dismissed_at,
                resolved_at,
            } => Some(AgentChatTimelineItem::RejoinConflict(RejoinConflictCard {
                id,
                branch,
                target,
                files,
                detail,
                created_at,
                requested_at,
                dismissed_at,
                resolved_at,
            })),
            Self::Memorized {
                memory_id,
                text,
                global,
                created_at,
            } => Some(AgentChatTimelineItem::Memorized(MemorizedCard {
                memory_id,
                text,
                global,
                created_at,
            })),
            Self::MemoryProposal {
                id,
                text,
                why,
                suggested_global,
                status,
                memory_id,
                accepted_global,
                source,
                created_at,
            } => {
                let status = match (status.as_str(), memory_id) {
                    ("pending", _) => MemoryProposalStatus::Pending,
                    ("accepted", Some(memory_id)) => MemoryProposalStatus::Accepted {
                        memory_id,
                        global: accepted_global.unwrap_or(suggested_global),
                    },
                    // "accepted" without a memory id is unrecoverable — degrade
                    // to Dismissed so the text still guards dedupe.
                    _ => MemoryProposalStatus::Dismissed,
                };
                Some(AgentChatTimelineItem::MemoryProposal(MemoryProposalCard {
                    id,
                    text,
                    why,
                    suggested_global,
                    source,
                    status,
                    created_at,
                }))
            }
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Message { .. } => "message",
            Self::WorkLog { .. } => "work_log",
            Self::PendingUserInput { .. } => "pending_user_input",
            Self::ProposedPlan { .. } => "proposed_plan",
            Self::CodeReview { .. } => "code_review",
            Self::Verification { .. } => "verification",
            Self::ChangedFiles { .. } => "changed_files",
            Self::ShipResult { .. } => "ship_result",
            Self::Rejoined { .. } => "rejoined",
            Self::RejoinConflict { .. } => "rejoin_conflict",
            Self::Memorized { .. } => "memorized",
            Self::MemoryProposal { .. } => "memory_proposal",
        }
    }

    fn event_key(&self) -> Option<String> {
        match self {
            Self::Message {
                role,
                text,
                created_at,
                backend_message_id,
                ..
            } => Some(match backend_message_id {
                Some(message_id) => format!("message:{role}:backend:{message_id}"),
                None => format!(
                    "message:{role}:{}:{:016x}",
                    created_at,
                    stable_hash(text.as_bytes())
                ),
            }),
            Self::WorkLog { id, .. } => Some(format!("work_log:{id}")),
            Self::PendingUserInput { request_id, .. } => {
                Some(format!("pending_user_input:{request_id}"))
            }
            Self::ProposedPlan { id, .. } => Some(format!("proposed_plan:{id}")),
            Self::CodeReview { id, .. } => Some(format!("code_review:{id}")),
            Self::Verification { id, .. } => Some(format!("verification:{id}")),
            Self::ChangedFiles { files, .. } => {
                let mut bytes = Vec::new();
                for file in files {
                    bytes.extend_from_slice(file.path.as_bytes());
                    bytes.extend_from_slice(file.additions.to_string().as_bytes());
                    bytes.extend_from_slice(file.deletions.to_string().as_bytes());
                }
                Some(format!("changed_files:{:016x}", stable_hash(&bytes)))
            }
            Self::ShipResult { id, .. } => Some(format!("ship_result:{id}")),
            Self::Rejoined { id, .. } => Some(format!("rejoined:{id}")),
            Self::RejoinConflict { id, .. } => Some(format!("rejoin_conflict:{id}")),
            Self::Memorized { memory_id, .. } => Some(format!("memorized:{memory_id}")),
            Self::MemoryProposal { id, .. } => Some(format!("memory_proposal:{id}")),
        }
    }

    fn created_at(&self) -> u64 {
        match self {
            Self::Message { created_at, .. } => *created_at,
            Self::WorkLog { updated_at, .. } => *updated_at,
            Self::ProposedPlan { implemented_at, .. } => implemented_at.unwrap_or_else(unix_now),
            Self::ShipResult { created_at, .. } => *created_at,
            Self::Rejoined { created_at, .. } => *created_at,
            Self::RejoinConflict { created_at, .. } => *created_at,
            Self::Memorized { created_at, .. } => *created_at,
            Self::MemoryProposal { created_at, .. } => *created_at,
            _ => unix_now(),
        }
    }
}

pub(super) fn work_log_kind_label(kind: WorkLogEntryKind) -> &'static str {
    match kind {
        WorkLogEntryKind::Tool => "tool",
        WorkLogEntryKind::Step => "step",
        WorkLogEntryKind::Plan => "plan",
        WorkLogEntryKind::UserInput => "user_input",
        WorkLogEntryKind::System => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memorized_event_identity_matches_atomic_undo_contract() {
        let memory_id = Uuid::new_v4();
        let item = AgentChatTimelineItem::Memorized(MemorizedCard {
            memory_id,
            text: "Use shared controls.".to_string(),
            global: false,
            created_at: 42,
        });

        let (kind, event_key, _, created_at) =
            stored_timeline_event_parts(&item).expect("memorized event");

        assert_eq!(kind, "memorized");
        assert_eq!(
            event_key.as_deref(),
            Some(format!("memorized:{memory_id}").as_str())
        );
        assert_eq!(created_at, 42);
    }

    #[test]
    fn memory_proposal_round_trips_through_storage() {
        let memory_id = Uuid::new_v4();
        for status in [
            MemoryProposalStatus::Pending,
            MemoryProposalStatus::Accepted {
                memory_id,
                global: true,
            },
            MemoryProposalStatus::Dismissed,
        ] {
            let item = AgentChatTimelineItem::MemoryProposal(MemoryProposalCard {
                id: "mp-test".to_string(),
                text: "Always keep UI copy in sentence case.".to_string(),
                why: "You corrected the plan's title casing.".to_string(),
                suggested_global: false,
                source: "plan_feedback".to_string(),
                status: status.clone(),
                created_at: 42,
            });

            let (kind, event_key, payload_json, created_at) =
                stored_timeline_event_parts(&item).expect("memory proposal event");
            assert_eq!(kind, "memory_proposal");
            assert_eq!(event_key.as_deref(), Some("memory_proposal:mp-test"));
            assert_eq!(created_at, 42);

            let payload =
                serde_json::from_str::<StoredTimelinePayload>(&payload_json).expect("payload");
            let Some(AgentChatTimelineItem::MemoryProposal(restored)) =
                payload.into_timeline_item()
            else {
                panic!("expected a memory proposal back");
            };
            assert_eq!(restored.status, status);
            assert_eq!(restored.text, "Always keep UI copy in sentence case.");
            assert_eq!(restored.suggested_global, false);
        }
    }

    #[test]
    fn accepted_proposal_without_memory_id_degrades_to_dismissed() {
        let payload = StoredTimelinePayload::MemoryProposal {
            id: "mp-broken".to_string(),
            text: "rule".to_string(),
            why: String::new(),
            suggested_global: true,
            status: "accepted".to_string(),
            memory_id: None,
            accepted_global: None,
            source: String::new(),
            created_at: 1,
        };
        let Some(AgentChatTimelineItem::MemoryProposal(card)) = payload.into_timeline_item() else {
            panic!("expected a memory proposal back");
        };
        assert_eq!(card.status, MemoryProposalStatus::Dismissed);
    }
}

pub(super) fn parse_work_log_kind(kind: &str) -> WorkLogEntryKind {
    match kind {
        "tool" => WorkLogEntryKind::Tool,
        "step" => WorkLogEntryKind::Step,
        "plan" => WorkLogEntryKind::Plan,
        "user_input" => WorkLogEntryKind::UserInput,
        _ => WorkLogEntryKind::System,
    }
}

pub(super) fn work_log_status_label(status: WorkLogStatus) -> &'static str {
    match status {
        WorkLogStatus::Pending => "pending",
        WorkLogStatus::InProgress => "in_progress",
        WorkLogStatus::Completed => "completed",
        WorkLogStatus::Failed => "failed",
    }
}

pub(super) fn parse_work_log_status(status: &str) -> WorkLogStatus {
    match status {
        "pending" => WorkLogStatus::Pending,
        "in_progress" => WorkLogStatus::InProgress,
        "failed" => WorkLogStatus::Failed,
        _ => WorkLogStatus::Completed,
    }
}

pub(super) fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub(super) fn next_local_id() -> String {
    format!("local-{}", unix_now())
}
