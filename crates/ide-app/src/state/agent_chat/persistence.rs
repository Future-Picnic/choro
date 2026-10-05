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
    let store = LocalStore::open_default().ok()?;
    load_file_ledger_from_store(&store, agent_id).ok()
}

/// Load the durable projection independently of chat paging. Old projections
/// get one recovery pass, persisted back to the DB; current projections are
/// authoritative, including an empty ledger after a revert.
pub(crate) fn load_file_ledger_from_store(
    store: &LocalStore,
    agent_id: Uuid,
) -> anyhow::Result<ChangedFilesSummary> {
    // Receipt and projection reads use separate queries. Reject a capture
    // straddling a write instead of marking newer receipts as already applied
    // to an older projection.
    for _ in 0..3 {
        let ledger = store.load_chat_file_ledger(agent_id)?;
        let receipts = store.load_timeline_events_by_kind(agent_id, "changed_files")?;
        if ledger == store.load_chat_file_ledger(agent_id)? {
            return load_file_ledger_projection(store, agent_id, ledger, receipts);
        }
    }
    anyhow::bail!("Saved files are still changing. Retry after the agent finishes its current edit.")
}

fn load_file_ledger_projection(
    store: &LocalStore,
    agent_id: Uuid,
    ledger: Option<ide_core::local_store::StoredChatFileLedger>,
    receipts: Vec<StoredTimelineEvent>,
) -> anyhow::Result<ChangedFilesSummary> {
    // Fresh Studio/doc assistants can be viewed before an agent row exists.
    // With no projection or receipts there is nothing to recover or persist.
    if ledger.is_none() && receipts.is_empty() {
        return Ok(ChangedFilesSummary::default());
    }
    let needs_recovery = ledger.as_ref().is_none_or(|ledger| ledger.projection_version == 0);
    let mut history = ChangedFilesSummary::default();
    let mut identities = std::collections::BTreeSet::new();
    for event in receipts {
        let Some(AgentChatTimelineItem::ChangedFiles(receipt)) = timeline_item_from_store_event(&event) else {
            anyhow::bail!("Saved file receipt {} is unreadable", event.id);
        };
        if let Some(identity) = receipt.receipt_identity() { identities.insert(identity); }
        if needs_recovery {
            history.merge_turn(&receipt);
        }
    }
    let Some(ledger) = ledger else {
        store.replace_chat_file_ledger(agent_id, history.ledger_revision, &file_ledger_entries(agent_id, &history))?;
        return Ok(history);
    };
    let mut summary = ChangedFilesSummary::default();
    summary.attribution_version = 1;
    summary.ledger_revision = ledger.revision;
    for entry in ledger.entries {
        let mut file = FileChangeStat::new(entry.path, entry.additions, entry.deletions)
            .with_content_hashes(entry.baseline_hash, entry.result_hash)
            .with_content_projection(entry.baseline_content, entry.result_content);
        file.counts_unavailable = entry.counts_unavailable;
        file.prior_segments = serde_json::from_str(&entry.segments_json).unwrap_or_default();
        if entry.observed {
            summary.observed_files.push(file);
        } else {
            summary.files.push(file);
        }
    }
    summary.remove_provider_private_artifacts();
    if !needs_recovery {
        summary.applied_receipts = identities;
        return Ok(summary);
    }
    for mut file in history.files {
        if let Some(saved) = summary.files.iter_mut().find(|saved| saved.path == file.path) {
            // An older truncated projection can also have lost the first
            // baseline for a path that still appears in Files. Do not present
            // its last few edits as the whole conversation's net change.
            if saved.baseline_hash != file.baseline_hash && saved.result_hash == file.result_hash {
                file.result_content = saved.result_content.clone();
                if file.baseline_content.is_none() { file.counts_unavailable = true; }
                *saved = file;
            }
        } else {
            summary.files.push(file);
        }
    }
    for file in history.observed_files {
        if !summary.files.iter().chain(&summary.observed_files).any(|saved| saved.path == file.path) {
            summary.observed_files.push(file);
        }
    }
    summary.applied_receipts = history.applied_receipts;
    summary.ledger_revision = summary.ledger_revision.max(history.ledger_revision);
    summary.files.sort_by(|a, b| a.path.cmp(&b.path));
    summary.observed_files.sort_by(|a, b| a.path.cmp(&b.path));
    store.replace_chat_file_ledger(agent_id, summary.ledger_revision, &file_ledger_entries(agent_id, &summary))?;
    Ok(summary)
}

pub(crate) fn load_latest_plan_from_store(store: &LocalStore, agent_id: Uuid) -> anyhow::Result<Option<ProposedPlan>> {
    let Some(event) = store.load_latest_timeline_event(agent_id, "proposed_plan")? else { return Ok(None) };
    match timeline_item_from_store_event(&event) {
        Some(AgentChatTimelineItem::ProposedPlan(plan)) => Ok(Some(plan)),
        _ => anyhow::bail!("Saved plan {} is unreadable", event.id),
    }
}

fn file_ledger_entries(agent_id: Uuid, ledger: &ChangedFilesSummary) -> Vec<ide_core::local_store::StoredChatFileLedgerEntry> {
    let updated_at = unix_now();
    ledger.files.iter().map(|file| (file, false))
        .chain(ledger.observed_files.iter().map(|file| (file, true)))
        .map(|(file, observed)| ide_core::local_store::StoredChatFileLedgerEntry {
            agent_id, path: file.path.clone(), observed,
            additions: file.additions, deletions: file.deletions,
            counts_unavailable: file.counts_unavailable,
            segments_json: serde_json::to_string(&file.prior_segments.iter().map(FileChangeStat::metadata).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".into()),
            baseline_hash: file.baseline_hash.clone(), result_hash: file.result_hash.clone(),
            baseline_content: file.baseline_content.clone(), result_content: file.result_content.clone(), updated_at,
        }).collect()
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
    let entries = file_ledger_entries(agent_id, &ledger);
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
    pub(super) fn from_stat(file: &FileChangeStat) -> Self {
        Self {
            counts_unavailable: file.counts_unavailable,
            prior_segments: file
                .prior_segments
                .iter()
                .map(FileChangeStat::metadata)
                .collect(),
            path: file.path.to_string_lossy().to_string(),
            additions: file.additions,
            deletions: file.deletions,
            counts_are_projection: file.counts_are_projection,
            clears_projection: file.clears_projection,
            baseline_hash: file.baseline_hash.clone(),
            result_hash: file.result_hash.clone(),
        }
    }

    fn into_stat(self) -> FileChangeStat {
        let mut file = FileChangeStat::new(self.path, self.additions, self.deletions)
            .with_count_projection(self.counts_are_projection)
            .with_cleared_projection(self.clears_projection)
            .with_content_hashes(self.baseline_hash, self.result_hash);
        file.counts_unavailable = self.counts_unavailable;
        file.prior_segments = self.prior_segments;
        file
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
                    search_text_version: TIMELINE_SEARCH_TEXT_VERSION,
                    search_text: searchable_message_text(message)
                        .map(|text| fold_search_text(&text))
                        .unwrap_or_default(),
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
                revision: plan.revision,
            }),
            AgentChatTimelineItem::CodeReview(review) => Some(Self::CodeReview {
                structured: review.structured.clone(),
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
                        counts_unavailable: file.counts_unavailable,
                        prior_segments: file
                            .prior_segments
                            .iter()
                            .map(FileChangeStat::metadata)
                            .collect(),
                        path: file.path.to_string_lossy().to_string(),
                        additions: file.additions,
                        deletions: file.deletions,
                        counts_are_projection: file.counts_are_projection,
                        clears_projection: file.clears_projection,
                        baseline_hash: file.baseline_hash.clone(),
                        result_hash: file.result_hash.clone(),
                    })
                    .collect(),
                observed_files: summary
                    .observed_files
                    .iter()
                    .map(|file| StoredFileChange {
                        counts_unavailable: file.counts_unavailable,
                        prior_segments: file
                            .prior_segments
                            .iter()
                            .map(FileChangeStat::metadata)
                            .collect(),
                        path: file.path.to_string_lossy().to_string(),
                        additions: file.additions,
                        deletions: file.deletions,
                        counts_are_projection: file.counts_are_projection,
                        clears_projection: file.clears_projection,
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
            AgentChatTimelineItem::OrbitUpdate(card) => Some(Self::OrbitUpdate {
                invocation_id: card.invocation_id,
                module_id: card.module_id,
                module_name: card.module_name.clone(),
                inserted: card.inserted,
                updated: card.updated,
                deleted: card.deleted,
                undone: card.undone,
                created_at: card.created_at,
            }),
            AgentChatTimelineItem::AgentSummary(card) => Some(Self::AgentSummary {
                summary_text: card.summary_text.clone(),
                last_summarized_sequence: card.last_summarized_sequence,
                updated_at: card.updated_at,
                edited_by_user: card.edited_by_user,
                expanded: card.expanded,
            }),
            AgentChatTimelineItem::DelegationGroup { run_id, created_at } => {
                Some(Self::DelegationGroup {
                    run_id: *run_id,
                    created_at: *created_at,
                })
            }
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
                search_text_version: _,
                search_text: _,
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
                revision,
            } => {
                let mut plan = ProposedPlan::new(id, markdown);
                plan.expanded = expanded;
                plan.implemented_at = implemented_at;
                plan.revision = revision;
                Some(AgentChatTimelineItem::ProposedPlan(plan))
            }
            Self::CodeReview {
                id,
                markdown,
                expanded,
                structured,
            } => {
                let mut review = CodeReview::new(id, markdown);
                if let Some(mut run) = structured {
                    if run.freshness == ide_core::code_review::ReviewFreshness::Current {
                        run.freshness = ide_core::code_review::ReviewFreshness::Uncertain("Revalidating saved review source and ownership".into());
                    }
                    review = CodeReview::from_run(run);
                }
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
                let restore = StoredFileChange::into_stat;
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
                    applied_receipts: Default::default(),
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
            Self::OrbitUpdate {
                invocation_id,
                module_id,
                module_name,
                inserted,
                updated,
                deleted,
                undone,
                created_at,
            } => Some(AgentChatTimelineItem::OrbitUpdate(OrbitUpdateCard {
                invocation_id,
                module_id,
                module_name,
                inserted,
                updated,
                deleted,
                undone,
                created_at,
            })),
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
            Self::DelegationGroup { run_id, created_at } => {
                Some(AgentChatTimelineItem::DelegationGroup { run_id, created_at })
            }
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
            Self::DelegationGroup { .. } => "delegation_group",
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
            Self::OrbitUpdate { .. } => "orbit_update",
            Self::AgentSummary { .. } => "agent_summary",
            Self::AgentMessage { .. } => "agent_message",
        }
    }

    fn event_key(&self) -> Option<String> {
        match self {
            Self::DelegationGroup { run_id, .. } => Some(format!("delegation_group:{run_id}")),
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
            Self::FileChangeActivity { id, turn_id, .. } => {
                Some(format!("file_change_activity:{turn_id}:{id}"))
            }
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
            Self::OrbitUpdate { invocation_id, .. } => {
                Some(format!("orbit_update:{invocation_id}"))
            }
            Self::AgentSummary { .. } => Some("agent_summary:living".to_string()),
            Self::AgentMessage { id, .. } => Some(format!("agent_message:{id}")),
        }
    }

    fn created_at(&self) -> u64 {
        match self {
            Self::DelegationGroup { created_at, .. } => *created_at,
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
            Self::OrbitUpdate { created_at, .. } => *created_at,
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
    fn fresh_assistant_loads_empty_artifacts_without_creating_a_file_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let project = ide_core::Project::from_path(dir.path().join("project"));
        // Studio's assistant record exists before its first provider turn has
        // registered an agent row. Merely viewing it must remain a read.
        let assistant = ide_core::DocAssistantRecord::new(project.id, PathBuf::from("design.choro"));
        let id = assistant.chat_agent_id;
        assert!(store.load_agents().unwrap().is_empty());
        assert_eq!(load_file_ledger_from_store(&store, id).unwrap(), ChangedFilesSummary::default());
        assert!(load_latest_plan_from_store(&store, id).unwrap().is_none());
        assert!(store.load_chat_file_ledger(id).unwrap().is_none());
        assert!(store.load_agents().unwrap().is_empty());
    }

    #[test]
    fn studio_legacy_receipts_recover_after_registering_the_conversation_parent() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let project = ide_core::Project::from_path(dir.path().join("project"));
        let mut config = ide_core::AppConfig::default();
        config.projects.push(project.clone());
        store.save_workspace_config(&config).unwrap();
        let mut agent = AgentRecord::new(project.id, project.path.clone(), "Studio Agent", "",
            ide_core::AgentKind::Codex, AgentModel::CodexDefault, AgentEffort::Medium,
            ide_core::agents::AgentAccessMode::FullAccess);
        agent.hidden_doc_assistant = true;
        agent.runtime = ide_core::AgentRuntimeKind::Chat;
        let receipt = ChangedFilesSummary {
            files: vec![FileChangeStat::new("src/design.rs", 5, 2)],
            turn_id: Some("studio-legacy-parent-turn".into()),
            attribution_version: 1,
            ledger_revision: 1,
            ..Default::default()
        };
        let (kind, key, json, at) = stored_timeline_event_parts(&AgentChatTimelineItem::ChangedFiles(receipt)).unwrap();
        // Legacy Studio writes allowed timeline events without an agent row,
        // but reconstructing their ledger fails its foreign-key constraint.
        store.upsert_timeline_event(agent.id, kind, key, json, at).unwrap();
        assert!(format!("{:#}", load_file_ledger_from_store(&store, agent.id).unwrap_err()).contains("FOREIGN KEY"));
        store.ensure_assistant_chat_agent(&agent).unwrap();
        let recovered = load_file_ledger_from_store(&store, agent.id).unwrap();
        assert_eq!(recovered.files[0].path, PathBuf::from("src/design.rs"));
        assert_eq!(recovered.files[0].additions, 5);
        store.save_agents(&[]).unwrap();
        assert_eq!(load_file_ledger_from_store(&store, agent.id).unwrap(), recovered);
    }

    #[test]
    fn reopening_recovers_old_files_and_keeps_latest_contents_and_receipt_identities() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let project = ide_core::Project::from_path(dir.path().join("project"));
        let mut agent = AgentRecord::new(project.id, project.path.clone(), "Saved artifacts", "",
            ide_core::AgentKind::Codex, AgentModel::CodexDefault, AgentEffort::Medium, ide_core::agents::AgentAccessMode::FullAccess);
        agent.runtime = ide_core::AgentRuntimeKind::Chat;
        let mut config = ide_core::AppConfig::default();
        config.projects = vec![project];
        store.save_workspace_config(&config).unwrap();
        store.save_agents(std::slice::from_ref(&agent)).unwrap();
        let receipt = |turn: &str, path: &str, before: &str, after: &str, revision| {
            let mut summary = ChangedFilesSummary::attributed(turn,
                vec![FileChangeStat::new(path, 1, 1)
                    .with_content_hashes(Some(before.into()), Some(after.into()))
                    .with_content_projection(Some(before.into()), Some(after.into()))], vec![]);
            summary.ledger_revision = revision;
            summary
        };
        let old = receipt("old", "old.rs", "before\n", "after\n", 1);
        let earlier = receipt("earlier", "recent.rs", "first baseline\n", "original\n", 2);
        let recent = receipt("recent", "recent.rs", "original\n", "final\n", 3);
        for summary in [&old, &earlier, &recent] {
            let (kind, key, payload, at) = stored_timeline_event_parts(&AgentChatTimelineItem::ChangedFiles(summary.clone())).unwrap();
            store.upsert_timeline_event(agent.id, kind, key, payload, at).unwrap();
        }
        // Reproduce the old bug's saved projection: the earlier path is absent.
        let saved = ide_core::local_store::StoredChatFileLedgerEntry {
            agent_id: agent.id, path: "recent.rs".into(), observed: false,
            additions: 1, deletions: 1, counts_unavailable: false, segments_json: "[]".into(),
            baseline_hash: Some("original\n".into()), result_hash: Some("final\n".into()),
            baseline_content: Some("original\n".into()), result_content: Some("final\n".into()), updated_at: 2,
        };
        store.replace_chat_file_ledger(agent.id, 3, &[saved]).unwrap();
        let plan = ProposedPlan::new("plan", "# Latest saved plan");
        let (kind, key, payload, at) = stored_timeline_event_parts(&AgentChatTimelineItem::ProposedPlan(plan.clone())).unwrap();
        store.upsert_timeline_event(agent.id, kind, key, payload, at).unwrap();
        for i in 0..205 {
            store.upsert_timeline_event(agent.id, "message", Some(format!("message:{i}")), "message", 4 + i).unwrap();
        }
        drop(store);
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        assert!(store.load_timeline_events_page(agent.id, None, 200).unwrap().events.iter().all(|event| event.kind == "message"));
        let mut legacy_ledger = store.load_chat_file_ledger(agent.id).unwrap().unwrap();
        legacy_ledger.projection_version = 0;
        let mut restored = load_file_ledger_projection(&store, agent.id, Some(legacy_ledger),
            store.load_timeline_events_by_kind(agent.id, "changed_files").unwrap()).unwrap();
        assert_eq!(restored.conversation_files().count(), 2);
        let recent_file = restored.files.iter().find(|file| file.path == PathBuf::from("recent.rs")).unwrap();
        assert_eq!(recent_file.baseline_hash.as_deref(), Some("first baseline\n"));
        assert!(recent_file.baseline_content.is_none());
        assert!(recent_file.counts_unavailable);
        assert_eq!(recent_file.result_content.as_deref(), Some("final\n"));
        let before_replay = restored.clone();
        restored.merge_turn(&old);
        restored.merge_turn(&recent);
        assert_eq!(restored, before_replay);
        assert_eq!(load_latest_plan_from_store(&store, agent.id).unwrap(), Some(plan));
        assert_eq!(store.load_chat_file_ledger(agent.id).unwrap().unwrap().projection_version, 1);
        assert_eq!(load_file_ledger_from_store(&store, agent.id).unwrap(), restored);
        // A current ledger is the source of truth: historical receipts must
        // never resurrect a deliberately removed path or a reverted file.
        store.replace_chat_file_ledger(agent.id, 4, &[]).unwrap();
        let empty = load_file_ledger_from_store(&store, agent.id).unwrap();
        assert!(empty.files.is_empty());
        assert_eq!(empty.ledger_revision, 4);
        assert!(!empty.applied_receipts.is_empty());
    }

    #[test]
    fn restored_structured_reviews_need_source_validation_before_looking_current() {
        use ide_core::code_review::*;
        let mut run=ReviewRun::new(Uuid::new_v4(),Uuid::new_v4(),"Claude".into(),"model".into(),"effort".into(),review_now());
        run.state=ReviewRunState::Complete;run.freshness=ReviewFreshness::Current;
        run.files.push(ReviewFile {id:"owned".into(),path:"owned.rs".into(),change_kind:"Modified".into(),attributed_ranges:vec![],before_hash:Some("a".repeat(64)),after_hash:Some("b".repeat(64)),diff_pages:1,consumed_pages:[0].into_iter().collect(),status:ReviewFileStatus::Complete,skip_reason:None});
        assert!(run.is_clean());
        let stored=StoredTimelinePayload::CodeReview {id:run.id.to_string(),markdown:String::new(),expanded:false,structured:Some(run)};
        let AgentChatTimelineItem::CodeReview(restored)=stored.into_timeline_item().unwrap() else {panic!("Expected review card");};
        assert!(!restored.is_clean());
        assert!(!restored.expanded);
        assert!(matches!(restored.structured.unwrap().freshness,ReviewFreshness::Uncertain(_)));
        let legacy=StoredTimelinePayload::CodeReview {id:"legacy".into(),markdown:"Existing Markdown review".into(),expanded:true,structured:None};
        let AgentChatTimelineItem::CodeReview(restored)=legacy.into_timeline_item().unwrap() else {panic!("Expected legacy review card");};
        assert_eq!(restored.markdown,"Existing Markdown review");
        assert!(restored.structured.is_none());
    }

    #[test]
    fn disconnected_segments_survive_restore_and_a_later_local_revert() {
        let edit = |id: &str, before: &str, after: &str| {
            FileChangeActivity::new(
                id,
                "turn",
                FileChangeStat::new("shared", 1, 1)
                    .with_content_hashes(Some(before.into()), Some(after.into())),
                false,
                0,
            )
        };
        let summary = ChangedFilesSummary::from_activities(
            "turn",
            &[edit("first", "A", "B"), edit("second", "manual", "C")],
        );
        let (_, _, raw, _) =
            stored_timeline_event_parts(&AgentChatTimelineItem::ChangedFiles(summary)).unwrap();
        let restored = serde_json::from_str::<StoredTimelinePayload>(&raw)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::ChangedFiles(restored) = restored else {
            panic!("receipt");
        };
        assert_eq!(restored.files[0].prior_segments.len(), 1);
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&restored);
        ledger.merge_turn(&ChangedFilesSummary::attributed(
            "later",
            vec![FileChangeStat::new("shared", 1, 1)
                .with_content_hashes(Some("C".into()), Some("manual".into()))],
            vec![],
        ));
        assert_eq!(ledger.files.len(), 1);
        assert_eq!(
            (ledger.files[0].additions, ledger.files[0].deletions),
            (1, 1)
        );
        assert_eq!(
            ledger.files[0].prior_segments[0].result_hash.as_deref(),
            Some("B")
        );
    }

    #[test]
    fn message_payload_persists_folded_visible_search_text() {
        let item = AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
            message_id: Some("turn-1".to_string()),
            text: "The **Maße**.\n\n<code_review>Hidden finding</code_review>".to_string(),
            created_at: 42,
        });

        let (_, _, payload, _) = stored_timeline_event_parts(&item).expect("message event");
        let payload = serde_json::from_str::<serde_json::Value>(&payload).expect("JSON payload");
        assert_eq!(payload["search_text_version"], TIMELINE_SEARCH_TEXT_VERSION);
        assert_eq!(payload["search_text"], "the masse.");
    }

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
    fn restoring_a_large_receipt_keeps_all_files_and_projection_counts() {
        let files = (0..24)
            .map(|i| FileChangeStat::new(format!("direct-{i}.rs"), 1, 0))
            .collect();
        let observed = (0..64)
            .map(|i| FileChangeStat::new(format!("shell-{i}.rs"), 1, 0).as_count_projection())
            .collect();
        let receipt = ChangedFilesSummary::attributed("large-turn", files, observed);
        let (_, _, payload, _) =
            stored_timeline_event_parts(&AgentChatTimelineItem::ChangedFiles(receipt)).unwrap();
        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::ChangedFiles(restored) = restored else {
            panic!("expected file receipt");
        };
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&restored);
        assert_eq!(ledger.files.len(), 24);
        assert_eq!(ledger.observed_files.len(), 64);
        assert_eq!(ledger.conversation_files().count(), 24);
        assert!(ledger
            .observed_files
            .iter()
            .all(|file| file.counts_are_projection));
        assert_eq!(
            ledger.total_additions() + ledger.total_observed_additions(),
            88
        );
    }

    #[test]
    fn restored_unattributed_reverts_cannot_erase_conversation_work() {
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&ChangedFilesSummary::attributed(
            "edit",
            vec![FileChangeStat::new("a.rs", 8, 0)],
            vec![],
        ));
        let receipt = ChangedFilesSummary::attributed(
            "revert",
            vec![],
            vec![FileChangeStat::new("a.rs", 0, 0)
                .as_count_projection()
                .with_cleared_projection(true)],
        );
        let (_, _, payload, _) =
            stored_timeline_event_parts(&AgentChatTimelineItem::ChangedFiles(receipt)).unwrap();
        let AgentChatTimelineItem::ChangedFiles(restored) =
            serde_json::from_str::<StoredTimelinePayload>(&payload)
                .unwrap()
                .into_timeline_item()
                .unwrap()
        else {
            panic!("expected receipt");
        };
        ledger.merge_turn(&restored);
        assert_eq!(ledger.conversation_files().count(), 1);
        assert_eq!(ledger.files[0], FileChangeStat::new("a.rs", 8, 0));
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
            Some("file_change_activity:turn-7:codex:tool-7:index.html")
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
    fn orbit_update_round_trips_with_invocation_identity() {
        let invocation_id = Uuid::new_v4();
        let item = AgentChatTimelineItem::OrbitUpdate(OrbitUpdateCard {
            invocation_id,
            module_id: Uuid::new_v4(),
            module_name: "Analytics".to_string(),
            inserted: 2,
            updated: 1,
            deleted: 0,
            undone: false,
            created_at: 42,
        });
        let (kind, event_key, payload, created_at) =
            stored_timeline_event_parts(&item).expect("Orbit update event");
        assert_eq!(kind, "orbit_update");
        assert_eq!(
            event_key.as_deref(),
            Some(format!("orbit_update:{invocation_id}").as_str())
        );
        assert_eq!(created_at, 42);
        let restored = serde_json::from_str::<StoredTimelinePayload>(&payload)
            .unwrap()
            .into_timeline_item()
            .unwrap();
        let AgentChatTimelineItem::OrbitUpdate(restored) = restored else {
            panic!("expected Orbit update");
        };
        assert_eq!(restored.inserted, 2);
        assert_eq!(restored.module_name, "Analytics");
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
