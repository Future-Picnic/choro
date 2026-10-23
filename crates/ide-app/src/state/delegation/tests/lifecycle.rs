use super::*;

#[test]
fn committed_startup_repairs_missing_lead_adoption_without_duplicate_delivery() {
    let mut f = Fixture::new();
    assert!(lead_configuration_required(&f.load(), &f.parent));
    // Activation committed, but applying its adoption effect was interrupted.
    let _effect = f.job(Job::Configure(f.load(), f.parent.clone())).unwrap();
    let run = f.load();
    assert_eq!(run.status, RunStatus::Active);
    assert!(lead_configuration_required(&run, &f.parent));
    let effect = f.job(Job::Configure(run.clone(), f.parent.clone())).unwrap();
    f.adopt(effect);
    assert!(!lead_configuration_required(&f.load(), &f.parent));
    assert_eq!(serde_json::to_value(f.load()).unwrap(), serde_json::to_value(run).unwrap());
}

#[test]
fn database_contention_retries_startup_without_failing_dependents_or_duplicating_work() {
    let mut f = Fixture::new();
    // Hold a real Turso writer while another thread tries to acquire it. This
    // exercises the actual error type/context returned by LocalStore, without
    // touching the live app's database or starting an AI provider.
    let competing = f.store.clone();
    let run_id = f.run;
    let error = f.store.update_delegation(run_id, None, |_| {
        Ok(std::thread::spawn(move || {
            competing.update_delegation(run_id, None, |_| Ok(()))
        }).join().unwrap().unwrap_err())
    }).unwrap().context("could not start teammate");
    assert!(ide_core::local_store::is_database_busy(&error), "{error:#}");

    f.store.update_delegation(f.run, None, |run| {
        run.tasks[1].plan.dependencies = vec![run.tasks[0].plan.key.clone()];
        Ok(())
    }).unwrap();
    let id = f.load().tasks[0].id;
    for stage in 0..4 {
        let run = f.load();
        let job = match stage {
            0 => Job::Configure(run.clone(), f.parent.clone()),
            1 => Job::Provision(run.clone(), id, f.parent.clone()),
            2 => Job::BeginPrepare(run.clone(), id, f.parent.clone()),
            _ => Job::Prepare(run.clone(), id, f.parent.clone()),
        };
        // Repeated contention must not turn a pending assignment into Failed
        // or consume attempts, revisions, or deliveries.
        for _ in 0..2 {
            let waiting = preparation::record_job_failure(&f.store, &job, &error).unwrap_err();
            assert!(waiting.to_string().contains("retry automatically"));
            assert_eq!(serde_json::to_value(f.load()).unwrap(), serde_json::to_value(&run).unwrap());
        }
        let effect = f.job(job).unwrap();
        f.adopt(effect);
        let current = f.load();
        assert_eq!(current.tasks[1].status, TaskStatus::Queued);
        assert!(!current.events.iter().any(|e| e.kind == "failed"));
    }
    let run = f.load();
    let task = run.task(id).unwrap();
    assert_eq!(task.status, TaskStatus::Running);
    assert_eq!(task.attempts.len(), 1);
    let child = task.attempt().unwrap().child_agent_id;
    assert_eq!(run.deliveries.iter().filter(|d| d.target == child).count(), 1);
    assert!(run.deliveries.iter().filter(|d| d.target == child)
        .all(|d| d.status == DeliveryStatus::Queued && d.runtime_generation.is_none()));

    // Delivery is deliberately excluded: a lock after sending must not cause
    // the provider command to be replayed automatically.
    let child_record = f.store.load_agents().unwrap().into_iter().find(|a| a.id == child).unwrap();
    let deliveries = run.deliveries.iter().filter(|d| d.target == child).map(|d| d.id).collect();
    let job = Job::Deliver(run, child_record, deliveries, AgentInteractionMode::Default);
    preparation::record_job_failure(&f.store, &job, &error).unwrap();
    assert_eq!(f.load().task(id).unwrap().status, TaskStatus::Failed);
}

#[test]
fn queued_and_blocked_assignments_have_a_chat_before_any_copy_or_provider() {
    let mut f = Fixture::new();
    for blocked in [false, true] {
        if blocked { f.store.update_delegation(f.run, None, |r| { r.block("Provider unavailable"); Ok(()) }).unwrap(); }
        let run = f.load();
        let id = run.tasks[usize::from(blocked)].id;
        let effect = f.job(Job::Provision(run, id, f.parent.clone())).unwrap();
        let child = f.adopt(effect);
        let run = f.load();
        let task = run.task(id).unwrap();
        let attempt = task.attempt().unwrap();
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(attempt.child_agent_id, child.id);
        assert_eq!(child.delegation.as_ref().unwrap().attempt_id, Some(attempt.id));
        assert_eq!(attempt.workspace, f.parent.runtime_path());
        assert!(attempt.snapshot.is_none());
        assert!(!run.deliveries.iter().any(|d| d.target == child.id));
    }
}

#[test]
fn recovering_a_missing_chat_preserves_provider_sessions_and_rejects_a_fresh_start() {
    let mut f = Fixture::new();
    f.configure();
    let tasks = f.load().tasks.iter().map(|t| t.id).collect::<Vec<_>>();
    for id in tasks {
        let original = f.prepare(id);
        f.store.update_delegation(f.run, None, |r| {
            let attempt = r.task_mut(id)?.attempt_mut().unwrap();
            attempt.generation = 1;
            attempt.session_id = Some("saved-provider-session".into());
            Ok(())
        }).unwrap();
        f.store.save_agents(&[f.parent.clone()]).unwrap();
        let effect = f.job(Job::Provision(f.load(), id, f.parent.clone())).unwrap();
        let recovered = f.adopt(effect);
        assert_eq!(recovered.id, original.id);
        if recovered.provider == AgentKind::Codex {
            assert_eq!(recovered.chat_session_id.as_deref(), Some("saved-provider-session"));
        } else {
            assert_eq!(recovered.cli_session_id.as_deref(), Some("saved-provider-session"));
        }
        let mut missing_session = recovered;
        missing_session.chat_session_id = None;
        missing_session.cli_session_id = None;
        let run = f.load();
        let deliveries = run.deliveries.iter().filter(|d| d.target == missing_session.id)
            .map(|d| d.id).collect::<Vec<_>>();
        assert!(!deliveries.is_empty());
        let error = f.job(Job::Deliver(run, missing_session, deliveries.clone(), AgentInteractionMode::Default)).err().unwrap();
        assert!(error.to_string().contains("saved provider session ID is missing"));
        assert!(f.load().deliveries.iter().filter(|d| deliveries.contains(&d.id))
            .all(|d| d.status == DeliveryStatus::Queued));
    }
}

#[test]
fn preparing_reuses_the_visible_chat_and_progress_does_not_change_authority() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let effect = f.job(Job::Provision(f.load(), id, f.parent.clone())).unwrap();
    let visible = f.adopt(effect);
    let effect = f.job(Job::BeginPrepare(f.load(), id, f.parent.clone())).unwrap();
    let preparing = f.adopt(effect);
    assert_eq!(visible.id, preparing.id);
    let run = f.load();
    let task = run.task(id).unwrap();
    let attempt = task.attempt().unwrap();
    assert_eq!(task.status, TaskStatus::Preparing);
    assert_eq!(task.attempts.len(), 1);
    assert_eq!(attempt.workspace, f.parent.runtime_path());
    assert!(f.store.update_delegation_preparation(run.id, id, attempt.id, "Checking selected skills").unwrap());
    let progressed = f.load();
    assert_eq!(progressed.revision, run.revision);
    assert_eq!(progressed.task(id).unwrap().revision, task.revision);
    assert_eq!(progressed.task(id).unwrap().attempt().unwrap().progress, "Checking selected skills");
    assert!(!f.store.update_delegation_preparation(run.id, id, Uuid::new_v4(), "Stale progress").unwrap());
    f.store.update_delegation(run.id, None, |r| r.control(r.parent_agent_id, id, "cancel", "Replace assignment")).unwrap();
    assert!(!f.store.update_delegation_preparation(run.id, id, attempt.id, "Late progress").unwrap());
    assert_eq!(f.load().task(id).unwrap().attempt().unwrap().progress, "Checking selected skills");
}

#[test]
fn stopped_preparation_cannot_block_or_fail_a_replacement() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let effect = f.job(Job::BeginPrepare(f.load(), id, f.parent.clone())).unwrap();
    f.adopt(effect);
    let job = Job::Prepare(f.load(), id, f.parent.clone());
    f.store.update_delegation(f.run, None, |r| r.control(r.parent_agent_id, id, "cancel", "Use the other Bandmate")).unwrap();
    preparation::record_job_failure(&f.store, &job, &anyhow::anyhow!("Preparation stopped")).unwrap();
    let run = f.load();
    assert_eq!(run.status, RunStatus::Active);
    assert_eq!(run.task(id).unwrap().status, TaskStatus::Cancelled);
    assert_eq!(run.ready_tasks(0), vec![run.tasks[1].id]);
    assert!(run.pause_reason.is_none());
}

#[test]
fn setup_failure_is_actionable_and_does_not_stop_sibling_work() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let effect = f.job(Job::BeginPrepare(f.load(), id, f.parent.clone())).unwrap();
    let child = f.adopt(effect);
    let job = Job::Prepare(f.load(), id, f.parent.clone());
    preparation::record_job_failure(&f.store, &job, &anyhow::anyhow!("Could not prepare selected skills: disk full")).unwrap();
    let run = f.load();
    assert_eq!(run.status, RunStatus::Active);
    assert_eq!(run.task(id).unwrap().status, TaskStatus::Failed);
    assert!(run.task(id).unwrap().reason.as_ref().unwrap().contains("disk full"));
    assert!(f.store.load_agents().unwrap().iter().any(|a| a.id == child.id));
    assert_eq!(run.ready_tasks(0), vec![run.tasks[1].id]);
    let entries = display::assignment_overview(&[run], f.parent.id, &|_| false);
    let failed = entries.iter().find(|e| e.task_id == id).unwrap();
    assert_eq!(failed.status_label, "Could not continue");
    assert!(failed.detail.contains("disk full"));
}

#[test]
fn an_unavailable_child_provider_still_has_an_openable_chat_and_does_not_stop_the_band() {
    let mut f = Fixture::new();
    f.configure();
    let task = f.load().tasks[0].id;
    let effect = f.job(Job::BeginPrepare(f.load(), task, f.parent.clone())).unwrap();
    let child = f.adopt(effect);
    let job = Job::Prepare(f.load(), task, f.parent.clone());
    let error = execute_in_store(&f.store, job.clone(), |_| anyhow::bail!("Sign in to the selected provider"), None).err().unwrap();
    preparation::record_job_failure(&f.store, &job, &error).unwrap();
    let run = f.load();
    assert_eq!(run.status, RunStatus::Active);
    assert_eq!(run.task(task).unwrap().attempt().unwrap().child_agent_id, child.id);
    assert_eq!(run.task(task).unwrap().reason.as_deref(), Some("Sign in to the selected provider"));
    assert_eq!(child.runtime_path(), f.parent.runtime_path());
    assert!(!run.deliveries.iter().any(|d| d.target == child.id));
}

#[test]
fn queue_details_name_holds_dependencies_and_global_capacity() {
    let f = Fixture::new();
    let mut run = f.load();
    run.status = RunStatus::Active;
    let task = run.tasks[0].id;
    let key = run.tasks[1].plan.key.clone();
    run.task_mut(task).unwrap().plan.dependencies.push(key.clone());
    assert_eq!(display::task_detail(&run, run.task(task).unwrap()), Some(format!("Waiting for {key}")));
    run.task_mut(task).unwrap().status = TaskStatus::Planned;
    assert!(display::task_detail(&run, run.task(task).unwrap()).unwrap().contains("held"));
    run.task_mut(task).unwrap().status = TaskStatus::Queued;
    run.task_mut(task).unwrap().plan.dependencies.clear();
    let mut other = run.clone();
    other.id = Uuid::new_v4();
    other.tasks[0].status = TaskStatus::Running;
    run.limits.concurrent_global = 1;
    assert!(display::task_detail_across_runs(&[run.clone(), other], &run, run.task(task).unwrap()).unwrap().contains("across your projects"));
}

#[test]
fn cancelled_rows_keep_their_state_and_blocked_rows_explain_recovery() {
    let f = Fixture::new();
    let id = f.load().tasks[0].id;
    f.store.update_delegation(f.run, None, |r| {
        r.control(r.parent_agent_id, id, "cancel", "Replaced")?;
        r.block("Could not prepare the repository. Check its location, then Resume.");
        Ok(())
    }).unwrap();
    let run = f.load();
    let entries = display::assignment_overview(&[run], f.parent.id, &|_| true);
    let cancelled = entries.iter().find(|e| e.task_id == id).unwrap();
    assert_eq!(cancelled.status_label, "Cancelled");
    assert_eq!(cancelled.activity, display::DelegationActivity::Idle);
    let blocked = entries.iter().find(|e| e.task_id != id).unwrap();
    assert_eq!(blocked.status_label, "Needs attention");
    assert!(blocked.detail.contains("Check its location"));
}

#[test]
fn preparation_spinner_is_distinct_from_working_and_real_questions() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let effect = f.job(Job::BeginPrepare(f.load(), id, f.parent.clone())).unwrap();
    f.adopt(effect);
    let run = f.load();
    assert_eq!(display::delegation_indicator(&[run.clone()], f.parent.id, &|_| false).unwrap().label, "Band · 1 preparing");
    assert_eq!(display::task_label(run.status, run.task(id).unwrap(), false), "Preparing Bandmate");
    assert_eq!(display::task_label(run.status, run.task(id).unwrap(), true), "Needs your input");
}

#[cfg(feature = "ui-layout-tests")]
#[gpui::test]
fn a_slow_preparation_keeps_refresh_live_and_does_not_gate_another_task_on_the_lead(cx: &mut gpui::TestAppContext) {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let effect = f.job(Job::BeginPrepare(f.load(), id, f.parent.clone())).unwrap();
    let child = f.adopt(effect);
    let mut run = f.load();
    run.deliveries.clear();
    let sibling = run.tasks[1].id;
    let parent = f.parent.clone();
    cx.update(|cx| {
        let agents = cx.new(|_| AgentRecords::in_memory(vec![parent.clone(), child]));
        let chats = cx.new(|_| AgentChatState::new());
        let terminals = cx.new(|_| TerminalManager::new());
        let (wake, _) = async_channel::bounded(1);
        let coordinator = cx.new(|_| DelegationCoordinator {
            runs: vec![run.clone()], error: None, agents, chats, terminals,
            settled: HashMap::new(), cursor: 0, stopped_parents: HashSet::new(), wake,
            reload_epoch: 0, snapshot: vec![],
            in_flight: HashMap::from([((run.id, id), Job::Prepare(run.clone(), id, parent.clone()))]),
            telemetry_in_flight: HashSet::new(),
            stop_requested: HashSet::new(), stop_in_flight: HashSet::new(),
        });
        coordinator.update(cx, |s, cx| {
            run.task_mut(id).unwrap().attempt_mut().unwrap().progress = "Checking selected skills".into();
            assert!(s.accept_snapshot(0, vec![run.clone()], run_stamps(&[run.clone()]).unwrap(), cx));
            assert!(s.runs[0].task(id).unwrap().attempt().unwrap().progress.contains("skills"));
            assert!(matches!(s.choose_active(&run, &parent, false, 1, cx), Some(Job::BeginPrepare(_, task, _)) if task == sibling));
            run.task_mut(id).unwrap().status = TaskStatus::Cancelled;
            assert!(matches!(s.choose_active(&run, &parent, false, 0, cx), Some(Job::BeginPrepare(_, task, _)) if task == sibling));
        });
    });
}
