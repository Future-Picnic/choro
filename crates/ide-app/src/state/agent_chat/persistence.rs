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

pub fn load_persisted_file_ledger(agent_id: Uuid) -> Option<ChangedFilesSummary> {
    let ledger = LocalStore::open_default()
        .ok()?
        .load_chat_file_ledger(agent_id)
        .ok()??;
    let mut summary = ChangedFilesSummary::default();
    summary.attribution_version = 1;
    summary.ledger_revision = ledger.revision;
    for entry in ledger.entries {
        let file = FileChangeStat::new(entry.path, entry.additions, entry.deletions)
            .with_content_hashes(entry.baseline_hash, entry.result_hash)
            .with_content_projection(entry.baseline_content, entry.result_content);
        if entry.observed {
            summary.observed_files.push(file);
        } else {
            summary.files.push(file);
        }
    }
    Some(summary)
}

pub(super) fn persist_changed_files_turn(
    agent_id: Uuid,
    receipt: ChangedFilesSummary,
    ledger: ChangedFilesSummary,
    cx: &mut Context<AgentChatState>,
) {
    let Some((kind, event_key, payload_json, created_at)) =
        stored_timeline_event_parts(&AgentChatTimelineItem::ChangedFiles(receipt))
    else {
        return;
    };
    let revision = ledger.ledger_revision;
    let updated_at = unix_now();
    let entries = ledger
        .files
        .iter()
        .map(|file| (file, false))
        .chain(ledger.observed_files.iter().map(|file| (file, true)))
        .map(
            |(file, observed)| ide_core::local_store::StoredChatFileLedgerEntry {
                agent_id,
                path: file.path.clone(),
                observed,
                additions: file.additions,
                deletions: file.deletions,
                baseline_hash: file.baseline_hash.clone(),
                result_hash: file.result_hash.clone(),
                baseline_content: file.baseline_content.clone(),
                result_content: file.result_content.clone(),
                updated_at,
            },
        )
        .collect::<Vec<_>>();
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = LocalStore::open_default().and_then(|store| {
                    store.persist_timeline_event_and_chat_file_ledger(
                        agent_id,
                        kind,
                        event_key,
                        payload_json,
                        created_at,
                        revision,
                        &entries,
                    )
                }) {
                    eprintln!("failed to persist changed-files turn: {error:#}");
                }
            })
            .await;
    })
    .detach();
}

pub(super) fn persist_chat_message(
    agent_id: Uuid,
    message: AgentChatMessage,
    cx: &mut Context<AgentChatState>,
) {
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .spawn(async move {
                let timeline_event =
                    stored_timeline_event_parts(&AgentChatTimelineItem::Message(message.clone()));
                let Some((role, text, created_at, backend_message_id)) =
                    stored_message_parts(&message)
                else {
                    return;
                };
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

pub(crate) fn persist_timeline_item(
    agent_id: Uuid,
    item: AgentChatTimelineItem,
    cx: &mut Context<AgentChatState>,
) {
    cx.spawn(async move |_, cx| {
        cx.background_executor()
            .spawn(async move {
                let Some((kind, event_key, payload_json, created_at)) =
                    stored_timeline_event_parts(&item)
                else {
                    return;
                };
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

impl StoredFileChange {
    fn from_stat(file: &FileChangeStat) -> Self {
        Self {
            path: file.path.to_string_lossy().to_string(),
            additions: file.additions,
            deletions: file.deletions,
            counts_are_projection: file.counts_are_projection,
            baseline_hash: file.baseline_hash.clone(),
            result_hash: file.result_hash.clone(),
        }
    }

    fn into_stat(self) -> FileChangeStat {
        FileChangeStat::new(self.path, self.additions, self.deletions)
            .with_count_projection(self.counts_are_projection)
            .with_content_hashes(self.baseline_hash, self.result_hash)
    }
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
            AgentChatTimelineItem::FileChangeActivity(activity) => Some(Self::FileChangeActivity {
                id: activity.id.clone(),
                turn_id: activity.turn_id.clone(),
                file: StoredFileChange::from_stat(&activity.file),
                observed: activity.observed,
                updated_at: activity.updated_at,
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
            AgentChatTimelineItem::ReviewChecklist(checklist) => Some(Self::ReviewChecklist {
                id: checklist.id.clone(),
                source_turn_id: checklist.source_turn_id.clone(),
                status: match checklist.status {
                    ReviewChecklistStatus::Pending => "pending",
                    ReviewChecklistStatus::Ready => "ready",
                    ReviewChecklistStatus::Failed => "failed",
                }
                .to_string(),
                items: checklist
                    .items
                    .iter()
                    .map(|item| StoredReviewChecklistItem {
                        id: item.id.clone(),
                        flow: item.flow.clone(),
                        action: item.action.clone(),
                        expected: item.expected.clone(),
                        checked: item.checked,
                    })
                    .collect(),
                expanded: checklist.expanded,
                created_at: checklist.created_at,
            }),
            AgentChatTimelineItem::ChangedFiles(summary) => Some(Self::ChangedFiles {
                snapshot_id: summary.snapshot_id,
                commit_sha: summary.commit_sha.clone(),
                turn_id: summary.turn_id.clone(),
                attribution_version: summary.attribution_version,
                ledger_revision: summary.ledger_revision,
                files: summary
                    .files
                    .iter()
                    .map(|file| StoredFileChange {
                        path: file.path.to_string_lossy().to_string(),
                        additions: file.additions,
                        deletions: file.deletions,
                        counts_are_projection: file.counts_are_projection,
                        baseline_hash: file.baseline_hash.clone(),
                        result_hash: file.result_hash.clone(),
                    })
                    .collect(),
                observed_files: summary
                    .observed_files
                    .iter()
                    .map(|file| StoredFileChange {
                        path: file.path.to_string_lossy().to_string(),
                        additions: file.additions,
                        deletions: file.deletions,
                        counts_are_projection: file.counts_are_projection,
                        baseline_hash: file.baseline_hash.clone(),
                        result_hash: file.result_hash.clone(),
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
            AgentChatTimelineItem::AgentSummary(card) => Some(Self::AgentSummary {
                summary_text: card.summary_text.clone(),
                last_summarized_sequence: card.last_summarized_sequence,
                updated_at: card.updated_at,
                edited_by_user: card.edited_by_user,
                expanded: card.expanded,
            }),
            AgentChatTimelineItem::AgentMessage(card) => Some(Self::AgentMessage {
                id: card.id,
                source_agent_id: card.source_agent_id,
                source_title: card.source_title.clone(),
                target_agent_id: card.target_agent_id,
                target_title: card.target_title.clone(),
                text: card.text.clone(),
                kind: card.kind.clone(),
                created_at: card.created_at,
            }),
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
            Self::FileChangeActivity {
                id,
                turn_id,
                file,
                observed,
                updated_at,
            } => Some(AgentChatTimelineItem::FileChangeActivity(
                FileChangeActivity::new(id, turn_id, file.into_stat(), observed, updated_at),
            )),
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
            Self::ReviewChecklist {
                id,
                source_turn_id,
                status,
                items,
                expanded,
                created_at,
            } => Some(AgentChatTimelineItem::ReviewChecklist(ReviewChecklist {
                id,
                source_turn_id,
                status: match status.as_str() {
                    "ready" => ReviewChecklistStatus::Ready,
                    "failed" => ReviewChecklistStatus::Failed,
                    _ => ReviewChecklistStatus::Pending,
                },
                items: items
                    .into_iter()
                    .map(|item| ReviewChecklistItem {
                        id: item.id,
                        flow: item.flow,
                        action: item.action,
                        expected: item.expected,
                        checked: item.checked,
                    })
                    .collect(),
                expanded,
                created_at,
            })),
            Self::ChangedFiles {
                files,
                observed_files,
                turn_id,
                attribution_version,
                ledger_revision,
                snapshot_id,
                commit_sha,
            } => {
                let restore = |file: StoredFileChange| {
                    FileChangeStat::new(file.path, file.additions, file.deletions)
                        .with_count_projection(file.counts_are_projection)
                        .with_content_hashes(file.baseline_hash, file.result_hash)
                };
                let mut files = files.into_iter().map(restore).collect::<Vec<_>>();
                let mut observed_files =
                    observed_files.into_iter().map(restore).collect::<Vec<_>>();
                // Events written before action attribution existed came from a
                // whole-worktree diff. Preserve them for historical feedback,
                // but never call them exact edits after hydration.
                if attribution_version == 0 {
                    observed_files.append(&mut files);
                    for file in &mut observed_files {
                        file.counts_are_projection = true;
                    }
                }
                Some(AgentChatTimelineItem::ChangedFiles(ChangedFilesSummary {
                    files,
                    observed_files,
                    turn_id,
                    attribution_version,
                    ledger_revision,
                    snapshot_id,
                    commit_sha,
                }))
            }
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
            Self::AgentSummary {
                summary_text,
                last_summarized_sequence,
                updated_at,
                edited_by_user,
                expanded,
            } => Some(AgentChatTimelineItem::AgentSummary(AgentSummaryCard {
                summary_text,
                last_summarized_sequence,
                updated_at,
                edited_by_user,
                expanded,
            })),
            Self::AgentMessage {
                id,
                source_agent_id,
                source_title,
                target_agent_id,
                target_title,
                text,
                kind,
                created_at,
            } => Some(AgentChatTimelineItem::AgentMessage(AgentMessageCard {
                id,
                source_agent_id,
                source_title,
                target_agent_id,
                target_title,
                text,
                kind,
                created_at,
            })),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Message { .. } => "message",
            Self::WorkLog { .. } => "work_log",
            Self::FileChangeActivity { .. } => "file_change_activity",
            Self::PendingUserInput { .. } => "pending_user_input",
            Self::ProposedPlan { .. } => "proposed_plan",
            Self::CodeReview { .. } => "code_review",
            Self::Verification { .. } => "verification",
            Self::ReviewChecklist { .. } => "review_checklist",
            Self::ChangedFiles { .. } => "changed_files",
            Self::ShipResult { .. } => "ship_result",
            Self::Rejoined { .. } => "rejoined",
            Self::RejoinConflict { .. } => "rejoin_conflict",
            Self::Memorized { .. } => "memorized",
            Self::MemoryProposal { .. } => "memory_proposal",
            Self::AgentSummary { .. } => "agent_summary",
            Self::AgentMessage { .. } => "agent_message",
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
            Self::FileChangeActivity { id, .. } => Some(format!("file_change_activity:{id}")),
            Self::PendingUserInput { request_id, .. } => {
                Some(format!("pending_user_input:{request_id}"))
            }
            Self::ProposedPlan { id, .. } => Some(format!("proposed_plan:{id}")),
            Self::CodeReview { id, .. } => Some(format!("code_review:{id}")),
            Self::Verification { id, .. } => Some(format!("verification:{id}")),
            Self::ReviewChecklist { source_turn_id, .. } => {
                Some(format!("review_checklist:turn:{source_turn_id}"))
            }
            Self::ChangedFiles {
                turn_id,
                files,
                observed_files,
                ..
            } => {
                if let Some(turn_id) = turn_id {
                    return Some(format!("changed_files:turn:{turn_id}"));
                }
                let mut bytes = Vec::new();
                for file in files.iter().chain(observed_files) {
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
            Self::AgentSummary { .. } => Some("agent_summary:living".to_string()),
            Self::AgentMessage { id, .. } => Some(format!("agent_message:{id}")),
        }
    }

    fn created_at(&self) -> u64 {
        match self {
            Self::Message { created_at, .. } => *created_at,
            Self::WorkLog { updated_at, .. } => *updated_at,
            Self::FileChangeActivity { updated_at, .. } => *updated_at,
            Self::ReviewChecklist { created_at, .. } => *created_at,
            Self::ProposedPlan { implemented_at, .. } => implemented_at.unwrap_or_else(unix_now),
            Self::ShipResult { created_at, .. } => *created_at,
            Self::Rejoined { created_at, .. } => *created_at,
            Self::RejoinConflict { created_at, .. } => *created_at,
            Self::Memorized { created_at, .. } => *created_at,
            Self::MemoryProposal { created_at, .. } => *created_at,
            Self::AgentSummary { updated_at, .. } => *updated_at,
            Self::AgentMessage { created_at, .. } => *created_at,
            _ => unix_now(),
        }
    }
}

pub(super) fn work_log_kind_label(kind: WorkLogEntryKind) -> &'static str {
    match kind {
        WorkLogEntryKind::Tool => "tool",
        WorkLogEntryKind::Command => "command",
        WorkLogEntryKind::Step => "step",
        WorkLogEntryKind::Plan => "plan",
        WorkLogEntryKind::UserInput => "user_input",
        WorkLogEntryKind::System => "system",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn attributed_file_receipt_round_trips_with_observed_changes_separate() {
        let mut summary = ChangedFilesSummary::attributed(
            "turn-42",
            vec![FileChangeStat::new("src/exact.rs", 3, 1)
                .with_content_hashes(Some("before".into()), Some("after".into()))],
            vec![FileChangeStat::new("generated.css", 8, 0).as_count_projection()],
        );
        summary.ledger_revision = 7;
        let item = AgentChatTimelineItem::ChangedFiles(summary);
        let (_, event_key, payload, _) =
            stored_timeline_event_parts(&item).expect("changed files event");
        assert_eq!(event_key.as_deref(), Some("changed_files:turn:turn-42"));

        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::ChangedFiles(restored) = restored else {
            panic!("expected changed files");
        };
        assert_eq!(restored.turn_id.as_deref(), Some("turn-42"));
        assert_eq!(restored.attribution_version, 1);
        assert_eq!(restored.ledger_revision, 7);
        assert_eq!(restored.files[0].path, PathBuf::from("src/exact.rs"));
        assert_eq!(restored.files[0].baseline_hash.as_deref(), Some("before"));
        assert_eq!(
            restored.observed_files[0].path,
            PathBuf::from("generated.css")
        );
        assert!(restored.observed_files[0].counts_are_projection);
    }

    #[test]
    fn review_checklist_round_trips_checked_state_with_stable_turn_identity() {
        let mut checklist = ReviewChecklist::ready(
            "turn-42",
            "## Settings\n- Open Settings — The automatic option is selected\n- Toggle it off — No checklist is generated",
            55,
        );
        checklist.items[0].checked = true;
        checklist.expanded = false;
        let item = AgentChatTimelineItem::ReviewChecklist(checklist);

        let (kind, event_key, payload, created_at) =
            stored_timeline_event_parts(&item).expect("review checklist event");
        assert_eq!(kind, "review_checklist");
        assert_eq!(event_key.as_deref(), Some("review_checklist:turn:turn-42"));
        assert_eq!(created_at, 55);

        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::ReviewChecklist(restored) = restored else {
            panic!("expected review checklist");
        };
        assert_eq!(restored.source_turn_id, "turn-42");
        assert_eq!(restored.items[0].flow.as_deref(), Some("Settings"));
        assert!(restored.items[0].checked);
        assert!(!restored.expanded);
    }

    #[test]
    fn older_review_checklist_items_default_to_no_flow() {
        let payload = r#"{
            "type":"review_checklist",
            "id":"review-checklist-turn-1",
            "source_turn_id":"turn-1",
            "status":"ready",
            "items":[{
                "id":"check-1",
                "action":"Open Settings",
                "expected":"Settings opens",
                "checked":false
            }],
            "expanded":true,
            "created_at":1
        }"#;

        let restored = serde_json::from_str::<StoredTimelinePayload>(payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::ReviewChecklist(restored) = restored else {
            panic!("expected review checklist");
        };
        assert_eq!(restored.items[0].flow, None);
    }

    #[test]
    fn live_file_change_activity_round_trips_with_stable_identity() {
        let item = AgentChatTimelineItem::FileChangeActivity(FileChangeActivity::new(
            "codex:tool-7:index.html",
            "turn-7",
            FileChangeStat::new("index.html", 23, 34),
            false,
            42,
        ));
        let (kind, event_key, payload, created_at) =
            stored_timeline_event_parts(&item).expect("file activity event");

        assert_eq!(kind, "file_change_activity");
        assert_eq!(
            event_key.as_deref(),
            Some("file_change_activity:codex:tool-7:index.html")
        );
        assert_eq!(created_at, 42);

        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::FileChangeActivity(restored) = restored else {
            panic!("expected live file activity");
        };
        assert_eq!(restored.turn_id, "turn-7");
        assert_eq!(restored.file.path, PathBuf::from("index.html"));
        assert_eq!(restored.file.additions, 23);
        assert_eq!(restored.file.deletions, 34);
    }

    #[test]
    fn pre_ledger_changed_files_restore_as_legacy_observations() {
        let payload = r#"{
            "type":"changed_files",
            "files":[{"path":".agents/skills/generated.md","additions":20,"deletions":0}],
            "snapshot_id":null,
            "commit_sha":null
        }"#;
        let restored = serde_json::from_str::<StoredTimelinePayload>(payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::ChangedFiles(restored) = restored else {
            panic!("expected changed files");
        };
        assert!(restored.files.is_empty());
        assert_eq!(restored.attribution_version, 0);
        assert_eq!(
            restored.observed_files[0].path,
            PathBuf::from(".agents/skills/generated.md")
        );
    }

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
    fn living_summary_uses_one_stable_timeline_identity() {
        let item = AgentChatTimelineItem::AgentSummary(AgentSummaryCard {
            summary_text: "Implemented indexed recall.".to_string(),
            last_summarized_sequence: 42,
            updated_at: 100,
            edited_by_user: true,
            expanded: false,
        });

        let (kind, event_key, payload, created_at) =
            stored_timeline_event_parts(&item).expect("summary event");
        assert_eq!(kind, "agent_summary");
        assert_eq!(event_key.as_deref(), Some("agent_summary:living"));
        assert_eq!(created_at, 100);
        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        assert!(matches!(
            restored,
            AgentChatTimelineItem::AgentSummary(AgentSummaryCard {
                last_summarized_sequence: 42,
                edited_by_user: true,
                ..
            })
        ));
    }

    #[test]
    fn incoming_agent_message_round_trips_with_source_identity() {
        let source_agent_id = Uuid::new_v4();
        let id = Uuid::new_v4();
        let item = AgentChatTimelineItem::AgentMessage(AgentMessageCard {
            id,
            source_agent_id,
            source_title: "API lane".to_string(),
            target_agent_id: None,
            target_title: None,
            text: "I am changing routes.rs".to_string(),
            kind: "collision".to_string(),
            created_at: 77,
        });
        let (kind, event_key, payload, created_at) =
            stored_timeline_event_parts(&item).expect("agent message event");
        assert_eq!(kind, "agent_message");
        assert_eq!(
            event_key.as_deref(),
            Some(format!("agent_message:{id}").as_str())
        );
        assert_eq!(created_at, 77);
        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        assert!(matches!(
            restored,
            AgentChatTimelineItem::AgentMessage(AgentMessageCard {
                id: restored_id,
                source_agent_id: restored_source,
                ..
            }) if restored_id == id && restored_source == source_agent_id
        ));
    }

    #[test]
    fn outgoing_agent_message_round_trips_with_stable_target_identity() {
        let source_agent_id = Uuid::new_v4();
        let target_agent_id = Uuid::new_v4();
        let id = Uuid::new_v4();
        let item = AgentChatTimelineItem::AgentMessage(AgentMessageCard {
            id,
            source_agent_id,
            source_title: "Current lane".to_string(),
            target_agent_id: Some(target_agent_id),
            target_title: Some("Storage lane".to_string()),
            text: "Please verify the migration.".to_string(),
            kind: "user".to_string(),
            created_at: 88,
        });
        let (_, _, payload, _) =
            stored_timeline_event_parts(&item).expect("outgoing agent message event");
        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        assert!(matches!(
            restored,
            AgentChatTimelineItem::AgentMessage(AgentMessageCard {
                target_agent_id: Some(restored_target),
                target_title: Some(restored_title),
                ..
            }) if restored_target == target_agent_id && restored_title == "Storage lane"
        ));
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
        "command" => WorkLogEntryKind::Command,
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
