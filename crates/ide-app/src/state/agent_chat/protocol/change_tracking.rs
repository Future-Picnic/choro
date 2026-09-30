use super::*;
use ide_core::agent_changes::{
    now_micros, ChangeKey, ChangeReceipt, ChangeReceiptState, ChangeTracker, EvidenceKind,
    MutationEvidence,
};
use std::collections::HashMap;

pub(super) fn file_payload_bytes(file: &FileChangeStat) -> usize {
    file.prior_segments
        .iter()
        .map(file_payload_bytes)
        .sum::<usize>()
        + file.path.as_os_str().len()
        + 512
        + file.baseline_content.as_ref().map_or(0, String::len)
        + file.result_content.as_ref().map_or(0, String::len)
        + file.baseline_hash.as_ref().map_or(0, String::len)
        + file.result_hash.as_ref().map_or(0, String::len)
        + file.raw_patch.as_ref().map_or(0, |(kind, s)| {
            kind.as_ref().map_or(0, String::len) + s.len()
        })
        + file.attributed_diff.as_ref().map_or(0, |d| {
            d.hunks
                .iter()
                .map(|h| h.header.len() + h.lines.iter().map(|l| l.text.len() + 64).sum::<usize>())
                .sum()
        })
}

pub(super) fn route(
    agent: AgentRecord,
    input: async_channel::Receiver<ChatBackendEvent>,
    output: EventSender,
    timing: Arc<ide_core::agent_changes::DispatchTiming>,
) {
    #[cfg(not(test))]
    ide_core::agent_changes::observer::refresh(agent.runtime_path());
    route_with_tracker(
        agent,
        input,
        output,
        ChangeTracker::global().clone(),
        timing,
    );
}

pub(super) fn route_with_tracker(
    agent: AgentRecord,
    input: async_channel::Receiver<ChatBackendEvent>,
    output: EventSender,
    tracker: Arc<ChangeTracker>,
    timing: Arc<ide_core::agent_changes::DispatchTiming>,
) {
    let agent = Arc::new(agent);
    thread::Builder::new()
        .name("choro-change-router".into())
        .spawn(move || {
            let initial_agent = agent.clone();
            tracker.enqueue(0, move |store| {
                if let Ok(store) = store {
                    let _ = ide_core::agent_changes::refresh_pending_paths(
                        &store,
                        initial_agent.id,
                        initial_agent.runtime_path(),
                    );
                }
            });
            let generation = uuid::Uuid::new_v4().to_string();
            let mut active: HashMap<String, Arc<AtomicBool>> = HashMap::new();
            let mut overflow = false;
            while let Ok(event) = input.recv_blocking() {
                let (event, reservation) = match event {
                    ChatBackendEvent::ReservedEvidence { event, reservation } => {
                        (*event, Some(reservation))
                    }
                    other => (other, None),
                };
                match event {
                    ChatBackendEvent::EvidenceOverflow => {
                        overflow = true;
                        for failed in active.values() {
                            failed.store(true, Ordering::Relaxed);
                        }
                    }
                    ChatBackendEvent::FileChangeActivity(mut activity) => {
                        let Some(path) = ide_core::agent_changes::relative_path(
                            agent.runtime_path(),
                            &activity.file.path,
                        ) else {
                            continue;
                        };
                        activity.file.path = path;
                        if ChangedFilesSummary::is_provider_private_artifact(&activity.file.path)
                            || super::super::changed_files::VisualizationArtifactFilter::new(
                                agent.id,
                                agent.runtime_path(),
                            )
                            .is_artifact(&activity.file.path)
                        {
                            continue;
                        }
                        if active.len() >= ide_core::agent_changes::MAX_PENDING_EVENTS
                            && !active.contains_key(&activity.turn_id)
                        {
                            overflow = true;
                            continue;
                        }
                        let failed = active
                            .entry(activity.turn_id.clone())
                            .or_insert_with(|| Arc::new(AtomicBool::new(overflow)))
                            .clone();
                        let captured_at = now_micros();
                        let worker_failed = failed.clone();
                        let worker_agent = agent.clone();
                        let worker_generation = generation.clone();
                        let worker_output = output.clone();
                        let bytes = if reservation.is_some() {
                            0
                        } else {
                            file_payload_bytes(&activity.file)
                                + activity.id.len()
                                + activity.turn_id.len()
                        };
                        if !tracker.enqueue(bytes, move |store| {
                            let _reservation = reservation;
                            let result = store.and_then(|store| {
                                let previous = store.load_change_receipt(
                                    worker_agent.id,
                                    &worker_generation,
                                    &activity.turn_id,
                                )?;
                                let late = previous
                                    .as_ref()
                                    .is_some_and(|r| r.state != ChangeReceiptState::Pending);
                                if previous.as_ref().is_some_and(|r| {
                                    matches!(
                                        r.state,
                                        ChangeReceiptState::Partial | ChangeReceiptState::Failed
                                    )
                                }) {
                                    worker_failed.store(true, Ordering::Relaxed);
                                }
                                let mut file = activity.file;
                                if let Some((kind, diff)) = file.raw_patch.take() {
                                    file = render_patch_change(
                                        &json!({"path":file.path,"kind":{"type":kind},"diff":diff}),
                                    )
                                    .unwrap_or(file);
                                }
                                use sha2::{Digest, Sha256};
                                if file.baseline_hash.is_none() {
                                    file.baseline_hash = file
                                        .baseline_content
                                        .as_ref()
                                        .map(|s| format!("{:x}", Sha256::digest(s.as_bytes())));
                                }
                                if file.result_hash.is_none() {
                                    file.result_hash = file
                                        .result_content
                                        .as_ref()
                                        .map(|s| format!("{:x}", Sha256::digest(s.as_bytes())));
                                }
                                let mut evidence = MutationEvidence {
                                    key: ChangeKey {
                                        project_id: worker_agent.project_id.0,
                                        root: worker_agent.runtime_path().to_path_buf(),
                                        agent_id: worker_agent.id,
                                        generation: worker_generation.clone(),
                                        turn_id: activity.turn_id.clone(),
                                        action_id: activity.id.clone(),
                                        path: file.path.clone(),
                                    },
                                    kind: if activity.observed {
                                        EvidenceKind::Observation
                                    } else if file.attributed_diff.is_some() {
                                        EvidenceKind::Patch
                                    } else {
                                        EvidenceKind::Contents
                                    },
                                    confirmed: !activity.observed,
                                    additions: (!file.counts_unavailable).then_some(file.additions),
                                    deletions: (!file.counts_unavailable).then_some(file.deletions),
                                    before_hash: file.baseline_hash.clone(),
                                    after_hash: file.result_hash.clone(),
                                    before: file.baseline_content.take(),
                                    after: file.result_content.take(),
                                    patch: file.attributed_diff.take(),
                                    captured_at,
                                };
                                evidence.bound();
                                file.counts_unavailable |= evidence.additions.is_none();
                                ide_core::agent_changes::trace("capture", json!({"payload_bytes":file_payload_bytes(&file) + evidence.before.as_ref().map_or(0, String::len) + evidence.after.as_ref().map_or(0, String::len),"confirmed":evidence.confirmed}));
                                if !store.insert_mutation_evidence(&evidence)? {
                                    return Ok(());
                                }
                                store.save_change_receipt(&ChangeReceipt {
                                    agent_id: worker_agent.id,
                                    generation: worker_generation.clone(),
                                    turn_id: activity.turn_id.clone(),
                                    state: ChangeReceiptState::Pending,
                                    updated_at: now_micros(),
                                })?;
                                ide_core::agent_changes::refresh_pending_paths(
                                    &store,
                                    worker_agent.id,
                                    worker_agent.runtime_path(),
                                )?;
                                let _ = worker_output.tx.send_blocking(
                                    ChatBackendEvent::ChangeReceiptPending(
                                        activity.turn_id.clone(),
                                    ),
                                );
                                let _ = worker_output.send_blocking(
                                    ChatBackendEvent::FileChangeActivity(FileChangeActivity::new(
                                        activity.id.clone(),
                                        activity.turn_id.clone(),
                                        file.metadata(),
                                        activity.observed,
                                        activity.updated_at,
                                    )),
                                );
                                if late {
                                    complete(
                                        Ok(store),
                                        worker_agent.clone(),
                                        worker_generation.clone(),
                                        ChangedFilesSummary::attributed(
                                            activity.turn_id,
                                            vec![],
                                            vec![],
                                        ),
                                        worker_failed.clone(),
                                        worker_output.clone(),
                                    );
                                }
                                Ok(())
                            });
                            if let Err(error) = result {
                                worker_failed.store(true, Ordering::Relaxed);
                                eprintln!("file evidence persistence: {error:#}");
                            }
                        }) {
                            failed.store(true, Ordering::Relaxed);
                        }
                    }
                    ChatBackendEvent::ChangedFiles(summary) => {
                        let id = summary.turn_id.unwrap_or_default();
                        let failed = active
                            .remove(&id)
                            .unwrap_or_else(|| Arc::new(AtomicBool::new(overflow)));
                        if overflow {
                            failed.store(true, Ordering::Relaxed);
                        }
                        finish(&agent, &generation, id, failed, output.clone(), &tracker);
                    }
                    other => {
                        let timer = match &other {
                            ChatBackendEvent::Status(AgentChatStatus::Running) => {
                                Some((&timing.prompt, "prompt_to_running"))
                            }
                            ChatBackendEvent::Status(AgentChatStatus::Cancelling) => {
                                Some((&timing.interrupt, "interrupt_dispatch"))
                            }
                            _ => None,
                        };
                        if let Some((timer, label)) = timer {
                            let started = timer.swap(0, Ordering::Relaxed);
                            if started > 0 {
                                ide_core::agent_changes::trace(
                                    label,
                                    json!({"elapsed_us":now_micros().saturating_sub(started)}),
                                );
                            }
                        }
                        let _ = output.tx.send_blocking(other);
                    }
                }
            }
            for (id, failed) in active {
                finish(&agent, &generation, id, failed, output.clone(), &tracker);
            }
        })
        .expect("start file evidence router");
}

fn finish(
    agent: &Arc<AgentRecord>,
    generation: &str,
    id: String,
    failed: Arc<AtomicBool>,
    output: EventSender,
    tracker: &Arc<ChangeTracker>,
) {
    let receipt = ChangeReceipt {
        agent_id: agent.id,
        generation: generation.to_owned(),
        turn_id: id.clone(),
        state: ChangeReceiptState::Partial,
        updated_at: now_micros(),
    };
    let fallback = ChangedFilesSummary::attributed(id.clone(), vec![], vec![]);
    let (agent, generation, worker_output) = (agent.clone(), generation.to_owned(), output.clone());
    if !tracker.enqueue(id.len() + 256, move |store| {
        if let Ok(store) = &store {
            // Repeated terminal notifications never reopen a completed receipt.
            if store
                .load_change_receipt(agent.id, &generation, &id)
                .ok()
                .flatten()
                .is_some_and(|r| r.state != ChangeReceiptState::Pending)
                && !failed.load(Ordering::Relaxed)
            {
                return;
            }
        }
        complete(
            store,
            agent,
            generation,
            ChangedFilesSummary::attributed(id, vec![], vec![]),
            failed,
            worker_output,
        );
    }) {
        tracker.record_incomplete(receipt);
        let _ = output
            .tx
            .send_blocking(ChatBackendEvent::ChangeReceiptReady {
                summary: fallback,
                state: ChangeReceiptState::Partial,
            });
    }
}

fn complete(
    store: anyhow::Result<LocalStore>,
    agent: Arc<AgentRecord>,
    generation: String,
    mut summary: ChangedFilesSummary,
    failed: Arc<AtomicBool>,
    output: EventSender,
) {
    summary.attribution_version = 2;
    let mut state = if failed.load(Ordering::Relaxed) {
        ChangeReceiptState::Partial
    } else {
        ChangeReceiptState::Ready
    };
    match store {
        Ok(store) => {
            match store.load_turn_mutation_evidence(
                agent.id,
                &generation,
                summary.turn_id.as_deref().unwrap_or(""),
            ) {
                Ok(evidence) => {
                    if store
                        .turn_mutation_count(
                            agent.id,
                            &generation,
                            summary.turn_id.as_deref().unwrap_or(""),
                        )
                        .unwrap_or(usize::MAX)
                        > evidence.len()
                    {
                        state = ChangeReceiptState::Partial;
                    }
                    let activities = evidence
                        .into_iter()
                        .map(|e| {
                            let mut file = FileChangeStat::new(
                                e.key.path,
                                e.additions.unwrap_or(0),
                                e.deletions.unwrap_or(0),
                            )
                            .with_content_hashes(e.before_hash, e.after_hash)
                            .with_content_projection(e.before.clone(), e.after.clone());
                            file.counts_unavailable = e.additions.is_none();
                            file.attributed_diff = e.patch.or_else(|| {
                                e.before.as_deref().zip(e.after.as_deref()).and_then(
                                    |(before, after)| {
                                        ide_core::git::diff::diff_from_contents(
                                            &file.path, before, after,
                                        )
                                        .ok()
                                    },
                                )
                            });
                            if let Some(diff) =
                                file.attributed_diff.as_ref().filter(|d| !d.is_binary)
                            {
                                if file.counts_unavailable {
                                    file.additions = diff
                                        .hunks
                                        .iter()
                                        .flat_map(|h| &h.lines)
                                        .filter(|l| l.origin == ide_core::git::LineOrigin::Add)
                                        .count();
                                    file.deletions = diff
                                        .hunks
                                        .iter()
                                        .flat_map(|h| &h.lines)
                                        .filter(|l| l.origin == ide_core::git::LineOrigin::Remove)
                                        .count();
                                    file.counts_unavailable = false;
                                }
                            }
                            FileChangeActivity::new(
                                e.key.action_id,
                                e.key.turn_id,
                                file,
                                !e.confirmed,
                                0,
                            )
                        })
                        .collect::<Vec<_>>();
                    if !activities.is_empty() {
                        summary = ChangedFilesSummary::from_activities(
                            summary.turn_id.clone().unwrap_or_default(),
                            activities.iter(),
                        );
                        summary.attribution_version = 2;
                    }
                }
                Err(error) => {
                    state = ChangeReceiptState::Failed;
                    eprintln!("load immutable evidence: {error:#}");
                }
            }
            summary = capture_changed_files_snapshot(&agent, summary, "agent_evidence_v2", &store);
            if summary.files.iter().any(|f| {
                f.attributed_diff.is_none()
                    && (f.baseline_content.is_none() || f.result_content.is_none())
            }) {
                state = ChangeReceiptState::Partial;
            }
            if !summary.files.is_empty() && summary.snapshot_id.is_none() {
                state = ChangeReceiptState::Partial;
            }
            // The historical receipt must survive a crash before the UI
            // consumes this callback. The UI's ledger update is idempotent.
            summary.ledger_revision = store
                .load_chat_file_ledger(agent.id)
                .ok()
                .flatten()
                .map_or(0, |l| l.revision)
                .saturating_add(1)
                .max(now_micros());
            if let Some((kind, key, payload, created_at)) =
                super::super::persistence::stored_timeline_event_parts(
                    &super::super::AgentChatTimelineItem::ChangedFiles(summary.clone()),
                )
            {
                if let Err(error) =
                    store.upsert_timeline_event(agent.id, kind, key, payload, created_at)
                {
                    state = ChangeReceiptState::Failed;
                    eprintln!("durable change receipt: {error:#}");
                }
            }
            let receipt = ChangeReceipt {
                agent_id: agent.id,
                generation,
                turn_id: summary.turn_id.clone().unwrap_or_default(),
                state,
                updated_at: now_micros(),
            };
            if let Err(error) = store.save_change_receipt(&receipt) {
                state = ChangeReceiptState::Failed;
                eprintln!("file receipt persistence: {error:#}");
            }
        }
        Err(error) => {
            state = ChangeReceiptState::Failed;
            eprintln!("file receipt store: {error:#}");
        }
    }
    ide_core::agent_changes::trace(
        "receipt",
        json!({"state":state,"files":summary.files.len()}),
    );
    for file in summary.files.iter_mut().chain(&mut summary.observed_files) {
        *file = file.metadata();
    }
    let _ = output.send_blocking(ChatBackendEvent::ChangeReceiptReady { summary, state });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn receive_receipt(
        rx: &async_channel::Receiver<ChatBackendEvent>,
    ) -> (ChangedFilesSummary, ChangeReceiptState) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(event) = rx.try_recv() {
                if let ChatBackendEvent::ChangeReceiptReady { summary, state } = unpack_event(event)
                {
                    return (summary, state);
                }
            } else {
                thread::sleep(Duration::from_millis(2));
            }
        }
        panic!("receipt did not complete");
    }

    #[test]
    fn sealed_turn_deduplicates_replayed_actions_and_accepts_late_unique_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().join("data")).unwrap();
        let tracker = ChangeTracker::with_store(store.clone());
        let agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            dir.path().to_path_buf(),
            "test",
            "",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::FullAccess,
        );
        let (tx, rx) = event_channel();
        let (out, result) = event_channel();
        route_with_tracker(
            agent.clone(),
            rx,
            out,
            tracker.clone(),
            Arc::new(Default::default()),
        );
        let activity = FileChangeActivity::new(
            "one",
            "original-turn",
            file_stat_from_patch_change(
                &json!({"path":"first.rs", "kind":{"type":"add"}, "diff":"proven edit\n"}),
            )
            .unwrap(),
            false,
            0,
        );
        tx.send_blocking(ChatBackendEvent::FileChangeActivity(activity.clone()))
            .unwrap();
        tx.send_blocking(ChatBackendEvent::ChangedFiles(
            ChangedFilesSummary::attributed("original-turn", vec![], vec![]),
        ))
        .unwrap();
        let (first, state) = receive_receipt(&result);
        assert_eq!(state, ChangeReceiptState::Ready);
        assert_eq!(first.total_additions(), 1);
        tx.send_blocking(ChatBackendEvent::FileChangeActivity(activity))
            .unwrap();
        tx.send_blocking(ChatBackendEvent::ChangedFiles(
            ChangedFilesSummary::attributed("original-turn", vec![], vec![]),
        ))
        .unwrap();
        tx.send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .unwrap();
        loop {
            match unpack_event(result.recv_blocking().unwrap()) {
                ChatBackendEvent::Status(AgentChatStatus::Running) => break,
                event => panic!("duplicate notification reopened the receipt: {event:?}"),
            }
        }
        let (done, waited) = std::sync::mpsc::channel();
        assert!(tracker.enqueue(0, move |_| {
            done.send(()).unwrap();
        }));
        waited.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(result.try_recv().is_err());
        tx.send_blocking(ChatBackendEvent::FileChangeActivity(
            FileChangeActivity::new(
                "late",
                "original-turn",
                FileChangeStat::new("second.rs", 1, 0)
                    .with_content_projection(Some("".into()), Some("late edit\n".into())),
                false,
                0,
            ),
        ))
        .unwrap();
        let (late, state) = receive_receipt(&result);
        assert_eq!(state, ChangeReceiptState::Ready);
        assert_eq!(late.turn_id.as_deref(), Some("original-turn"));
        assert_eq!(late.files.len(), 2);
        assert_eq!(
            store
                .query_agent_changes(agent.project_id.0, agent.id, agent.runtime_path(), &[], 0)
                .unwrap()["own_edits"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn blocked_tracking_does_not_block_status_or_new_turn_and_preserves_original_patch() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().join("state")).unwrap();
        let tracker = ChangeTracker::with_store(store.clone());
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        assert!(tracker.enqueue(0, move |_| {
            release_rx.recv().unwrap();
        }));
        let agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            dir.path().to_path_buf(),
            "test",
            "",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::FullAccess,
        );
        // These assets must never be opened by a prompt/status/receipt handler.
        fs::File::create(dir.path().join("huge.zip"))
            .unwrap()
            .set_len(3_000_000_000)
            .unwrap();
        let (tx, rx) = async_channel::unbounded();
        let (out, result) = async_channel::unbounded();
        route_with_tracker(agent, rx, out.into(), tracker, Arc::new(Default::default()));
        let activity = FileChangeActivity::new(
            "action",
            "old-turn",
            FileChangeStat::new("shared.rs", 1, 1)
                .with_content_projection(Some("original\n".into()), Some("agent edit\n".into())),
            false,
            0,
        );
        tx.send_blocking(ChatBackendEvent::FileChangeActivity(activity.clone()))
            .unwrap();
        tx.send_blocking(ChatBackendEvent::FileChangeActivity(activity))
            .unwrap();
        tx.send_blocking(ChatBackendEvent::ChangedFiles(
            ChangedFilesSummary::attributed("old-turn", vec![], vec![]),
        ))
        .unwrap();
        tx.send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle))
            .unwrap();
        tx.send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut activities = 0;
        let mut running = false;
        while Instant::now() < deadline {
            match result.try_recv() {
                Ok(ChatBackendEvent::FileChangeActivity(_)) => activities += 1,
                Ok(ChatBackendEvent::Status(AgentChatStatus::Running)) => {
                    running = true;
                    break;
                }
                Ok(ChatBackendEvent::ChangeReceiptReady { .. }) => {
                    panic!("tracker is still blocked")
                }
                _ => thread::sleep(Duration::from_millis(2)),
            }
        }
        assert!(running, "provider status was blocked by tracking");
        assert_eq!(
            activities, 0,
            "UI evidence is published only after persistence"
        );
        fs::write(dir.path().join("shared.rs"), "someone else's later edit\n").unwrap();
        release_tx.send(()).unwrap();
        drop(tx);
        let (summary, state) = loop {
            match unpack_event(result.recv_blocking().unwrap()) {
                ChatBackendEvent::FileChangeActivity(_) => activities += 1,
                ChatBackendEvent::ChangeReceiptReady { summary, state } => break (summary, state),
                _ => {}
            }
        };
        assert_eq!(activities, 1);
        assert_eq!(state, ChangeReceiptState::Ready);
        assert_eq!(summary.turn_id.as_deref(), Some("old-turn"));
        let snapshot = store
            .load_agent_diff_snapshot(summary.snapshot_id.unwrap())
            .unwrap()
            .unwrap();
        let text = serde_json::to_string(&snapshot).unwrap();
        assert!(text.contains("agent edit"));
        assert!(!text.contains("someone else's"));
    }
}
