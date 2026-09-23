use super::*;
use ide_core::{
    experts::ExpertProfile, AgentAccessMode, AgentKind, AgentModel, AppConfig, Project,
};
use std::fs;
mod acceptance;

struct Fixture {
    _dir: tempfile::TempDir,
    store: LocalStore,
    parent: AgentRecord,
    run: Uuid,
}

fn available(_: AgentKind) -> Result<()> {
    Ok(())
}

#[cfg(feature = "ui-layout-tests")]
#[gpui::test]
fn reload_hints_reject_stale_results_and_unchanged_polls_are_silent(cx: &mut gpui::TestAppContext) {
    use std::{cell::Cell, rc::Rc};
    let notifications = Rc::new(Cell::new(0));
    let coordinator = cx.update(|cx| {
        let agents = cx.new(|_| AgentRecords::in_memory(vec![]));
        let chats = cx.new(|_| AgentChatState::new());
        let terminals = cx.new(|_| TerminalManager::new());
        let (wake, _) = async_channel::bounded(1);
        let coordinator = cx.new(|_| DelegationCoordinator {
            runs: vec![], error: None, agents, chats, terminals, settled: HashMap::new(), cursor: 0,
            stopped_parents: HashSet::new(), wake, reload_epoch: 0, snapshot: vec![],
        });
        let count = notifications.clone();
        cx.observe(&coordinator, move |_, _| count.set(count.get() + 1)).detach();
        coordinator
    });
    let mut run = DelegationRun::new(Uuid::new_v4(), ide_core::ProjectId::new(), Uuid::new_v4(), "Work".into(), vec![], false, Default::default());
    let install = |run: &DelegationRun, epoch, cx: &mut gpui::TestAppContext| {
        let runs = vec![run.clone()];
        let bytes = run_stamps(&runs).unwrap();
        cx.update(|cx| coordinator.update(cx, |s, cx| s.accept_snapshot(epoch, runs, bytes, cx)))
    };
    assert!(install(&run, 0, cx));
    let count = notifications.get();
    for _ in 0..10 { assert!(install(&run, 0, cx)); }
    assert_eq!(notifications.get(), count);
    // Telemetry/event data can move without an authority revision.
    let revision = run.revision;
    run.event(run.parent_agent_id, None, "telemetry", "Updated usage");
    assert!(install(&run, 0, cx));
    assert_eq!(run.revision, revision);
    assert_eq!(notifications.get(), count + 1);
    cx.update(|cx| coordinator.update(cx, |s, _| s.request_reload()));
    run.status = RunStatus::Completed;
    assert!(!install(&run, 0, cx));
    cx.update(|cx| assert_ne!(coordinator.read(cx).runs[0].status, RunStatus::Completed));
    cx.update(|cx| coordinator.update(cx, |s, cx| s.accept_error(1, "Unavailable".into(), cx)));
    let count = notifications.get();
    cx.update(|cx| coordinator.update(cx, |s, cx| s.accept_error(1, "Unavailable".into(), cx)));
    assert_eq!(notifications.get(), count);
    assert!(install(&run, 1, cx));
    cx.update(|cx| { assert!(coordinator.read(cx).error.is_none()); assert_eq!(coordinator.read(cx).runs[0].status, RunStatus::Completed); });
}

#[test]
fn stopped_delegation_stops_lead_once_and_preserves_expert_gate() {
    let fixture = Fixture::new();
    for status in [
        RunStatus::Paused,
        RunStatus::Interrupted,
        RunStatus::Blocked,
    ] {
        let mut run = fixture.load();
        let mut stopped = HashSet::new();
        run.status = status;
        assert!(parent_stop_required(&mut stopped, &run));
        // Later polls must leave a new user-requested lead turn alive, while
        // automatic Expert work remains ineligible for dispatch.
        for _ in 0..3 {
            assert!(!parent_stop_required(&mut stopped, &run));
            assert!(!run.status.dispatchable());
            assert_eq!(run.status, status);
        }
        run.status = RunStatus::Preparing;
        assert!(!parent_stop_required(&mut stopped, &run));
        run.status = status;
        assert!(parent_stop_required(&mut stopped, &run));
    }
}

#[test]
fn on_demand_model_override_reaches_the_prepared_child_without_changing_lead() {
    let mut fixture = Fixture::new();
    let lead_model = fixture.parent.model;
    let lead_provider = fixture.parent.provider;
    let source = Uuid::new_v4();
    let auth = fixture
        .store
        .authorize_experts(
            fixture.parent.id,
            source,
            "Delegate teammates using GPT6 asrta and Sonnet 5",
            &[],
            false,
        )
        .unwrap()
        .unwrap();
    let run = fixture
        .store
        .begin_delegation(fixture.parent.id, source)
        .unwrap();
    assert_eq!(run.id, fixture.run);
    let plans = ["GPT6 asrta", "Sonnet 5"]
        .iter()
        .enumerate()
        .map(|(i, model)| TaskPlan {
            model_request: Some((*model).into()),
            key: format!("model-variant-{i}"),
            expert_id: source,
            goal: format!("Independent variant {i}"),
            brief: "Build a separate deliverable from the same source brief".into(),
            expected_outcome: "Verified result".into(),
            repository: fixture.parent.project_path.clone(),
            dependencies: vec![],
            kind: TaskKind::Implementation,
            held: false,
        })
        .collect();
    fixture
        .store
        .update_delegation(run.id, None, |r| {
            r.add_plans(fixture.parent.id, plans, &auth.temporary_experts)
        })
        .unwrap();
    fixture.configure();
    let tasks: Vec<_> = fixture
        .load()
        .tasks
        .into_iter()
        .filter(|t| t.plan.expert_id == source)
        .collect();
    assert_eq!(tasks.len(), 2);
    for (task, (provider, model)) in tasks.into_iter().zip([
        (AgentKind::Codex, AgentModel::CodexGpt6Astra),
        (AgentKind::Claude, AgentModel::ClaudeSonnet),
    ]) {
        let child = fixture.prepare(task.id);
        assert_eq!(child.provider, provider);
        assert_eq!(child.model, model);
        assert_eq!(child.expert_snapshot.as_ref().unwrap().profile.model, model);
        assert_ne!(child.id, fixture.parent.id);
    }
    assert_eq!(fixture.parent.model, lead_model);
    assert_eq!(fixture.parent.provider, lead_provider);
}

#[test]
fn resume_requires_session_identity_after_execution_started() {
    let fixture = Fixture::new();
    assert!(validate_resume_session(&fixture.parent, false).is_ok());
    let error = validate_resume_session(&fixture.parent, true).unwrap_err();
    assert!(error.to_string().contains("session ID is missing"));
    assert!(error
        .to_string()
        .contains("Files and conversations are preserved"));
}

impl Fixture {
    fn new() -> Self {
        Self::with_store_root(None)
    }
    fn with_store_root(root: Option<std::path::PathBuf>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(root.unwrap_or_else(|| dir.path().join("data"))).unwrap();
        let repo = dir.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        assert!(std::process::Command::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(&repo)
            .status()
            .unwrap()
            .success());
        fs::write(repo.join("user.txt"), "unfinished user work\n").unwrap();
        let project = Project::from_path(repo);
        let mut config = AppConfig::default();
        config.projects.push(project.clone());
        store.save_workspace_config(&config).unwrap();
        store
            .save_beta_features(ide_core::config::BetaFeatures { delegation: true, ..Default::default() })
            .unwrap();
        let model = AgentModel::default_for(AgentKind::Codex);
        let mut parent = AgentRecord::new(
            project.id,
            project.path,
            "Lead",
            "Build to-do",
            AgentKind::Codex,
            model,
            model.default_effort(),
            AgentAccessMode::default(),
        );
        parent.runtime = AgentRuntimeKind::Chat;
        store.save_agents(&[parent.clone()]).unwrap();
        let mut profiles = vec![];
        for (name, provider) in [
            ("UI Designer", AgentKind::Claude),
            ("Backend Master", AgentKind::Codex),
        ] {
            let model = AgentModel::default_for(provider);
            profiles.push(
                store
                    .save_expert(
                        ExpertProfile {
                            id: Uuid::new_v4(),
                            revision: 0,
                            name: name.into(),
                            description: name.into(),
                            provider,
                            model,
                            effort: model.default_effort(),
                            instructions: "Implement your assigned file and verify it.".into(),
                            skills: vec![],
                            expected_outcome: "Checked files".into(),
                            enabled: true,
                            archived: false,
                            additions: Default::default(),
                        },
                        None,
                    )
                    .unwrap(),
            );
        }
        let source = Uuid::new_v4();
        store.authorize_experts(parent.id,source,"Build a to-do list. Delegate design to UI Designer and backend to Backend Master. Do the rest yourself.",&[],false).unwrap();
        let run = store.begin_delegation(parent.id, source).unwrap();
        let plans = profiles
            .iter()
            .enumerate()
            .map(|(i, p)| TaskPlan {
                model_request: None,
                key: format!("task-{i}"),
                expert_id: p.id,
                goal: p.name.clone(),
                brief: format!("Implement file {i}.txt"),
                expected_outcome: "Checked output".into(),
                repository: parent.project_path.clone(),
                dependencies: vec![],
                kind: TaskKind::Implementation,
                held: false,
            })
            .collect();
        let snapshots = profiles
            .iter()
            .map(|p| p.snapshot().unwrap())
            .collect::<Vec<_>>();
        store
            .update_delegation(run.id, None, |r| r.add_plans(parent.id, plans, &snapshots))
            .unwrap();
        Self {
            _dir: dir,
            store,
            parent,
            run: run.id,
        }
    }
    fn load(&self) -> DelegationRun {
        self.store.load_delegation(self.run).unwrap()
    }
    fn job(&mut self, job: Job) -> Result<Effect> {
        execute_in_store(&self.store, job, available, None)
    }
    fn adopt(&mut self, effect: Effect) -> AgentRecord {
        let Effect::Adopt(agent) = effect else {
            panic!("Expected authoritative record adoption")
        };
        let mut records = self.store.load_agents().unwrap();
        records.retain(|a| a.id != agent.id);
        records.push(agent.clone());
        self.store.save_agents(&records).unwrap();
        if agent.id == self.parent.id {
            self.parent = agent.clone();
        }
        agent
    }
    fn configure(&mut self) {
        let effect = self
            .job(Job::Configure(self.load(), self.parent.clone()))
            .unwrap();
        self.adopt(effect);
    }
    fn prepare(&mut self, task: Uuid) -> AgentRecord {
        let effect = self
            .job(Job::Prepare(self.load(), task, self.parent.clone()))
            .unwrap();
        self.adopt(effect)
    }
    fn report(&mut self, task: Uuid) {
        self.store
            .update_delegation(self.run, None, |r| {
                let t = r.task(task)?;
                let a = t.attempt().unwrap();
                r.complete(
                    a.child_agent_id,
                    task,
                    a.id,
                    t.revision,
                    TaskResult {
                        summary: "Assignment checked".into(),
                        addressed: vec!["Assigned output".into()],
                        checks: vec!["Read output".into()],
                        unresolved: vec![],
                    },
                )
            })
            .unwrap();
        self.job(Job::Capture(self.load(), task)).unwrap();
    }
    fn integrate(&mut self, task: Uuid) {
        self.store
            .update_delegation(self.run, None, |r| {
                r.task_mut(task)?.status = TaskStatus::Integrating;
                Ok(())
            })
            .unwrap();
        self.job(Job::Integrate(self.load(), task)).unwrap();
    }
}

/// Run explicitly in a separate process with CHORO_EXPERTS=1 and a fresh
/// CHORO_DATA_DIR. Uses installed authentication; never the live Choro store.
#[test]
fn prepare_launches_bundled_skills_from_the_frozen_snapshot() {
    let mut f = Fixture::new();
    let task = f.load().tasks[0].id;
    let snapshot = ide_core::experts::catalog::catalog()
        .experts
        .iter()
        .find(|e| e.name == "UI Designer")
        .unwrap()
        .profile()
        .snapshot_at(f.store.root())
        .unwrap();
    assert!(snapshot.skills.iter().all(|s| !s.reference.source.exists()));
    f.store
        .update_delegation(f.run, None, |r| {
            r.task_mut(task)?.expert = snapshot.clone();
            Ok(())
        })
        .unwrap();
    f.configure();
    let child = f.prepare(task);
    assert_eq!(child.expert_snapshot.as_ref(), Some(&snapshot));
    let instructions = child
        .expert_snapshot
        .unwrap()
        .runtime_instructions(&f.store.root().join("expert-skill-cache"))
        .unwrap();
    assert!(instructions.contains("Frontend Design") && instructions.contains("Impeccable"));
    assert_eq!(f.load().task(task).unwrap().status, TaskStatus::Running);
}

/// Uses installed authentication in a disposable data directory.
#[test]
#[ignore = "requires authenticated Codex and Claude and a disposable CHORO_DATA_DIR"]
fn real_managed_providers_return_structured_results() {
    use super::super::agent_chat::protocol::{
        spawn_chat_backend, ChatBackendCommand, ChatBackendEvent,
    };
    let root = std::path::PathBuf::from(
        std::env::var("CHORO_DATA_DIR").expect("Provide a disposable data directory"),
    );
    assert!(
        root.file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("choro-delegation-acceptance-"),
        "This test refuses the live state directory"
    );
    assert!(
        !root.join("data").exists(),
        "Use a new acceptance fixture for every run"
    );
    let mut f = Fixture::with_store_root(Some(root));
    let mut frozen_proofs = std::collections::HashMap::new();
    if std::env::var("CHORO_ACCEPTANCE_BUILTINS").is_ok_and(|value| value == "1") {
        for task in f.load().tasks {
            let name = if task.expert.profile.provider == AgentKind::Claude {
                "UI Designer"
            } else {
                "Backend Engineer"
            };
            let mut profile = ide_core::experts::catalog::catalog()
                .experts
                .iter()
                .find(|e| e.name == name)
                .unwrap()
                .profile();
            profile.id = task.expert.profile.id;
            profile.revision = task.expert.profile.revision;
            profile.name = task.expert.profile.name.clone();
            let source = f._dir.path().join(format!("proof-skill-{}", task.id));
            fs::create_dir_all(source.join("references")).unwrap();
            fs::write(source.join("SKILL.md"), "Acceptance proof for this disposable fixture: before reporting completion, read references/proof.txt relative to this skill directory and append its entire contents as a line to your assigned text file. Preserve that line during follow-up revisions.").unwrap();
            let proof = format!("frozen-evidence-{}", Uuid::new_v4());
            fs::write(source.join("references/proof.txt"), &proof).unwrap();
            profile.skills.push(ide_core::experts::ExpertSkill {
                provider: profile.provider,
                name: "Frozen reference acceptance".into(),
                source: source.join("SKILL.md"),
            });
            let profile = f
                .store
                .save_expert(profile.clone(), Some(profile.revision))
                .unwrap();
            let snapshot = profile.snapshot_at(f.store.root()).unwrap();
            // Provider must read the frozen resource, not the changed installation.
            fs::write(
                source.join("references/proof.txt"),
                "Changed after snapshot",
            )
            .unwrap();
            f.store
                .update_delegation(f.run, None, |run| {
                    run.task_mut(task.id)?.expert = snapshot;
                    Ok(())
                })
                .unwrap();
            frozen_proofs.insert(task.id, proof);
        }
    }
    f.configure();
    let ids = f.load().ready_tasks(0);
    for task in ids {
        if std::env::var("CHORO_ACCEPTANCE_ONLY").is_ok_and(|p| p == "codex")
            && f.load().task(task).unwrap().expert.profile.provider != AgentKind::Codex
        {
            continue;
        }
        for work_revision in 1..=2 {
            if work_revision == 2 {
                f.store.update_delegation(f.run,None,|r|r.control(r.parent_agent_id,task,"request_changes","Append the exact line '- [ ] Sync' to the existing checklist. Preserve its other lines.")).unwrap();
            }
            // The disposable harness supplies its own file-only test instructions;
            // it does not alter any saved user's permissions or accept prompts.
            let mut child = f.prepare(task);
            child.access_mode = AgentAccessMode::FullAccess;
            let mut records = f.store.load_agents().unwrap();
            records
                .iter_mut()
                .find(|a| a.id == child.id)
                .unwrap()
                .access_mode = child.access_mode;
            f.store.save_agents(&records).unwrap();
            managed::preflight(child.provider).unwrap();
            let expected_session = child
                .chat_session_id
                .clone()
                .or(child.cli_session_id.clone());
            let (controller, events) =
                spawn_chat_backend(child.clone(), AgentInteractionMode::Default).unwrap();
            controller.send(ChatBackendCommand::SendTurn{text:format!("{}\n\nAcceptance fixture: write only the assigned file containing a short to-do checklist, read it back, and call delegation_complete. No dependencies, network services or external actions are needed.",child.doc),mode:AgentInteractionMode::Default,read_only:false}).unwrap();
            let started = std::time::Instant::now();
            let mut running = false;
            let mut settled = false;
            while started.elapsed() < Duration::from_secs(240) {
                match events.try_recv() {
                    Ok(ChatBackendEvent::Status(AgentChatStatus::Running)) => running = true,
                    Ok(ChatBackendEvent::Status(AgentChatStatus::Idle)) if running => {
                        settled = true;
                        break;
                    }
                    Ok(ChatBackendEvent::Status(AgentChatStatus::Failed)) => {
                        panic!("Provider failed")
                    }
                    Ok(ChatBackendEvent::ChatSessionReady { session_id })
                    | Ok(ChatBackendEvent::SessionReady { session_id }) => {
                        if let Some(expected) = &expected_session {
                            assert_eq!(
                                &session_id, expected,
                                "Resumption must retain the provider session identity"
                            );
                        }
                        let mut agents = f.store.load_agents().unwrap();
                        let saved = agents.iter_mut().find(|a| a.id == child.id).unwrap();
                        if child.provider == AgentKind::Claude {
                            saved.cli_session_id = Some(session_id);
                        } else {
                            saved.chat_session_id = Some(session_id);
                        }
                        saved.started_at = Some(ide_core::agents::unix_now());
                        f.store.save_agents(&agents).unwrap();
                    }
                    Ok(ChatBackendEvent::PendingApproval(p)) => {
                        panic!("Acceptance needs approval: {p:?}")
                    }
                    Ok(ChatBackendEvent::PendingUserInput(p)) => {
                        panic!("Acceptance needs user input: {p:?}")
                    }
                    Ok(ChatBackendEvent::Error(error)) => panic!("Provider error: {error}"),
                    Ok(ChatBackendEvent::WorkLog(entry)) => {
                        eprintln!("{:?}: {}", child.provider, entry.title)
                    }
                    Ok(_) => {}
                    Err(async_channel::TryRecvError::Empty) => {
                        std::thread::sleep(Duration::from_millis(100))
                    }
                    Err(async_channel::TryRecvError::Closed) => break,
                }
            }
            let stopped = controller.stop_signal();
            controller.force_shutdown();
            assert!(
                settled,
                "Provider did not settle within the acceptance deadline"
            );
            assert_eq!(
                f.load().task(task).unwrap().status,
                TaskStatus::CompletionRequested,
                "Provider did not submit the required MCP result"
            );
            f.job(Job::Capture(f.load(), task)).unwrap();
            f.integrate(task);
            if let Some(proof) = frozen_proofs.get(&task) {
                let key = f.load().task(task).unwrap().plan.key.clone();
                let path = f
                    .parent
                    .project_path
                    .join(format!("{}.txt", key.trim_start_matches("task-")));
                assert!(
                    fs::read_to_string(path).unwrap().contains(proof),
                    "Provider must read the frozen supporting reference, including after resume"
                );
            }
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !stopped.is_stopped() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Provider did not stop before session resume"
                );
                std::thread::sleep(Duration::from_millis(50));
            }
            if work_revision == 2 {
                let key = f.load().task(task).unwrap().plan.key.clone();
                let text = fs::read_to_string(
                    f.parent
                        .runtime_path()
                        .join(format!("{}.txt", key.trim_start_matches("task-"))),
                )
                .unwrap();
                assert_eq!(text.matches("- [ ] Sync").count(), 1);
            }
        }
    }
    assert!(f
        .load()
        .tasks
        .iter()
        .filter(|t| !t.attempts.is_empty())
        .all(|t| t.status == TaskStatus::Integrated));
}

#[test]
fn coordinator_integrates_two_experts_preserving_dirty_source_and_reuses_revision_chat() {
    let mut f = Fixture::new();
    f.configure();
    let ids = f.load().ready_tasks(0);
    assert_eq!(ids.len(), 2);
    let mut children = vec![];
    for (index, id) in ids.iter().enumerate() {
        let child = f.prepare(*id);
        assert_eq!(
            fs::read_to_string(child.runtime_path().join("user.txt")).unwrap(),
            "unfinished user work\n"
        );
        fs::write(
            child.runtime_path().join(format!("{index}.txt")),
            "expert output\n",
        )
        .unwrap();
        children.push(child);
    }
    fs::write(f.parent.runtime_path().join("lead.txt"), "lead output\n").unwrap();
    for id in &ids {
        f.report(*id);
        assert_eq!(f.load().task(*id).unwrap().status, TaskStatus::ResultReady);
        f.integrate(*id);
    }
    assert!(f.parent.runtime_path().join("0.txt").is_file());
    assert!(f.parent.runtime_path().join("1.txt").is_file());
    assert!(f.parent.runtime_path().join("lead.txt").is_file());
    assert!(!f.parent.runtime_path().join(".git/index").exists());
    f.store
        .update_delegation(f.run, None, |r| {
            r.control(
                r.parent_agent_id,
                ids[0],
                "request_changes",
                "Add one further requirement",
            )
        })
        .unwrap();
    let second = f.prepare(ids[0]);
    assert_eq!(second.id, children[0].id);
    assert_ne!(second.runtime_path(), children[0].runtime_path());
    assert!(second.runtime_path().join("1.txt").exists());
    fs::write(
        second.runtime_path().join("followup.txt"),
        "revision output",
    )
    .unwrap();
    f.report(ids[0]);
    f.integrate(ids[0]);
    f.store
        .update_delegation(f.run, None, |r| {
            r.finish(
                r.parent_agent_id,
                "Combined implementation".into(),
                "Verified all three expert files and the user's current work".into(),
            )
        })
        .unwrap();
    assert_eq!(f.load().status, RunStatus::Completed);
    assert!(children.iter().all(|a| a.runtime_path().exists()));
}

#[test]
fn durable_stop_invalidates_inflight_job_without_creating_a_child() {
    let mut f = Fixture::new();
    f.configure();
    let old = f.load();
    let job = Job::Prepare(old.clone(), old.tasks[0].id, f.parent.clone());
    f.store
        .update_delegation(f.run, None, |r| {
            r.pause("Stop", false);
            Ok(())
        })
        .unwrap();
    assert!(matches!(f.job(job).unwrap(), Effect::None));
    assert!(f.load().tasks.iter().all(|t| t.attempts.is_empty()));
    assert_eq!(f.store.load_agents().unwrap().len(), 1);
}

#[test]
fn delivery_is_persisted_before_runtime_dispatch_and_acknowledged_by_generation() {
    let mut f = Fixture::new();
    f.configure();
    let run = f.load();
    let target = run.parent_agent_id;
    let ids = run.deliveries.iter().map(|d| d.id).collect::<Vec<_>>();
    let old_job = Job::Deliver(
        run,
        f.parent.clone(),
        ids.clone(),
        AgentInteractionMode::Default,
    );
    let effect = f.job(old_job.clone()).unwrap();
    let Effect::Send(_, _, text, _, _) = effect else {
        panic!("Expected runtime effect")
    };
    assert!(text.contains(&format!("[Choro delivery {}]", ids[0])));
    assert!(f
        .load()
        .deliveries
        .iter()
        .all(|d| d.status == DeliveryStatus::Dispatched));
    assert!(matches!(f.job(old_job).unwrap(), Effect::None));
    f.store
        .update_delegation(f.run, None, |r| {
            r.deliveries[0].runtime_generation = Some(7);
            Ok(())
        })
        .unwrap();
    f.job(Job::Acknowledge(f.load(), target, 6)).unwrap();
    assert_eq!(f.load().deliveries[0].status, DeliveryStatus::Dispatched);
    f.job(Job::Acknowledge(f.load(), target, 7)).unwrap();
    assert_eq!(f.load().deliveries[0].status, DeliveryStatus::Acknowledged);
}

#[test]
fn conflict_resolution_and_late_parent_edit_are_reprepared_without_overwriting() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let child = f.prepare(id);
    fs::write(child.runtime_path().join("user.txt"), "expert change\n").unwrap();
    fs::write(f.parent.runtime_path().join("user.txt"), "parent change\n").unwrap();
    f.report(id);
    f.integrate(id);
    let run = f.load();
    let op = run.task(id).unwrap().integration.as_ref().unwrap();
    assert_eq!(op.status, workspace::IntegrationStatus::Conflict);
    fs::write(op.resolution_dir.join("user.txt"), "combined\n").unwrap();
    f.store
        .update_delegation(f.run, None, |r| {
            let t = r.task_mut(id)?;
            t.reason = Some("resolutions_submitted".into());
            t.status = TaskStatus::Integrating;
            Ok(())
        })
        .unwrap();
    fs::write(
        f.parent.runtime_path().join("user.txt"),
        "new parent change\n",
    )
    .unwrap();
    f.job(Job::Integrate(f.load(), id)).unwrap();
    assert_eq!(
        fs::read_to_string(f.parent.runtime_path().join("user.txt")).unwrap(),
        "new parent change\n"
    );
    assert_eq!(f.load().task(id).unwrap().status, TaskStatus::ResultReady);
}

#[test]
fn missing_result_is_requested_once_and_forged_workspace_is_rejected() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    f.prepare(id);
    f.job(Job::MissingReport(f.load(), id)).unwrap();
    assert!(
        f.load()
            .task(id)
            .unwrap()
            .attempt()
            .unwrap()
            .report_requested
    );
    f.job(Job::MissingReport(f.load(), id)).unwrap();
    assert_eq!(f.load().task(id).unwrap().status, TaskStatus::Failed);
    let mut run = f.load();
    run.tasks[0].attempt_mut().unwrap().workspace = f.parent.runtime_path().to_path_buf();
    assert!(validate_working_scope(&f.store, &run, &run.tasks[0]).is_err());
}

#[test]
fn lead_contract_changes_invalidate_pending_reports_and_corrections_preserve_user_pause() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    let child = f.prepare(id);
    let run = f.load();
    let task = run.task(id).unwrap();
    let attempt = task.attempt().unwrap().id;
    let old_revision = task.revision;
    let result = TaskResult {
        summary: "Done".into(),
        addressed: vec![],
        checks: vec!["Read file".into()],
        unresolved: vec![],
    };
    f.store
        .update_delegation(f.run, None, |r| {
            r.complete(child.id, id, attempt, old_revision, result.clone())
        })
        .unwrap();
    f.store
        .update_delegation(f.run, None, |r| {
            r.message(
                r.parent_agent_id,
                id,
                "Use the new API contract".into(),
                false,
            )
        })
        .unwrap();
    assert!(f
        .load()
        .task(id)
        .unwrap()
        .attempt()
        .unwrap()
        .result
        .is_none());
    assert!(f
        .store
        .update_delegation(f.run, None, |r| r.complete(
            child.id,
            id,
            attempt,
            old_revision,
            result
        ))
        .is_err());
    f.store
        .update_delegation(f.run, None, |r| {
            let t = r.task_mut(id)?;
            t.paused_status = Some(t.status);
            t.status = TaskStatus::Paused;
            r.user_correction(id, "Also include keyboard support".into())
        })
        .unwrap();
    assert_eq!(f.load().task(id).unwrap().status, TaskStatus::Paused);
    assert!(f
        .store
        .update_delegation(f.run, None, |r| r.control(
            r.parent_agent_id,
            id,
            "release",
            "Restart"
        ))
        .is_err());
}

#[test]
fn provider_preflight_failure_does_not_create_or_start_children() {
    let f = Fixture::new();
    let run = f.load();
    let failure = execute_in_store(
        &f.store,
        Job::Configure(run.clone(), f.parent.clone()),
        |_| anyhow::bail!("Unsupported installation"),
        None,
    );
    assert!(failure.is_err());
    assert_eq!(f.load().status, RunStatus::Preparing);
    assert!(f.load().tasks.iter().all(|t| t.attempts.is_empty()));
    assert_eq!(f.store.load_agents().unwrap().len(), 1);
}

#[test]
fn completed_history_cannot_detach_a_new_managed_run() {
    let mut f = Fixture::new();
    f.configure();
    let mut old = f.load();
    old.status = RunStatus::Completed;
    assert!(can_restore_lead_policy(&old, &f.parent));
    let mut next = f.parent.clone();
    next.delegation.as_mut().unwrap().run_id = Uuid::new_v4();
    assert!(!can_restore_lead_policy(&old, &next));
    old.status = RunStatus::Active;
    assert!(!can_restore_lead_policy(&old, &f.parent));
}

#[test]
fn consultation_delivers_in_default_mode_and_repairs_legacy_scope() {
    let mut f = Fixture::new();
    f.store
        .update_delegation(f.run, None, |run| {
            run.plan_mode = true;
            for task in &mut run.tasks {
                task.plan.kind = TaskKind::Consultation;
            }
            Ok(())
        })
        .unwrap();
    f.configure();
    assert_eq!(
        managed::interaction_mode(&f.parent, AgentInteractionMode::Plan),
        AgentInteractionMode::Plan
    );
    let task = f.load().tasks[0].id;
    let mut child = f.prepare(task);
    assert_eq!(child.access_mode, f.parent.access_mode);
    assert!(managed::consultation(&child));
    assert_eq!(
        child.delegation.as_ref().unwrap().task_kind,
        Some(TaskKind::Consultation)
    );
    let instructions = managed::instructions(String::new(), &child).unwrap();
    assert!(instructions.contains("Plan mode belongs only to the lead"));
    assert!(instructions.contains("return your actual answer"));
    // Older saved records did not include task_kind. They stay read-only until
    // their durable task repairs the policy, including direct user corrections.
    let mut json = serde_json::to_value(&child).unwrap();
    json["delegation"]
        .as_object_mut()
        .unwrap()
        .remove("task_kind");
    child = serde_json::from_value(json).unwrap();
    assert!(managed::consultation(&child));
    assert_eq!(
        managed::interaction_mode(&child, AgentInteractionMode::Plan),
        AgentInteractionMode::Default
    );
    let run = f.load();
    let ids = run
        .deliveries
        .iter()
        .filter(|d| d.target == child.id)
        .map(|d| d.id)
        .collect();
    let Effect::Send(repaired, _, _, mode, _) = f
        .job(Job::Deliver(run, child, ids, AgentInteractionMode::Plan))
        .unwrap()
    else {
        panic!("Expected delivery");
    };
    assert_eq!(mode, AgentInteractionMode::Default);
    assert_eq!(
        repaired.delegation.as_ref().unwrap().task_kind,
        Some(TaskKind::Consultation)
    );
    f.report(task);
    f.store
        .update_delegation(f.run, None, |run| {
            run.control(
                run.parent_agent_id,
                task,
                "accept",
                "Copy received and reviewed",
            )
        })
        .unwrap();
    assert_eq!(f.load().task(task).unwrap().status, TaskStatus::Accepted);
}

#[test]
fn implementation_children_ignore_plan_mode_without_changing_access() {
    let mut f = Fixture::new();
    f.configure();
    let task = f.load().tasks[0].id;
    let child = f.prepare(task);
    assert!(managed::is_child(&child));
    assert!(!managed::consultation(&child));
    assert_eq!(child.access_mode, f.parent.access_mode);
    assert_eq!(
        managed::interaction_mode(&child, AgentInteractionMode::Plan),
        AgentInteractionMode::Default
    );
    assert!(!managed::is_child(&f.parent));
    assert!(!managed::consultation(&f.parent));
}

#[test]
fn corrections_cannot_restart_cancelled_assignments() {
    let mut f = Fixture::new();
    f.configure();
    let id = f.load().tasks[0].id;
    f.prepare(id);
    f.store
        .update_delegation(f.run, None, |run| {
            run.control(run.parent_agent_id, id, "cancel", "User ended this work")
        })
        .unwrap();
    let before = f.load();
    assert!(f
        .store
        .update_delegation(f.run, None, |run| run
            .user_correction(id, "Try again".into()))
        .is_err());
    let after = f.load();
    assert_eq!(after.task(id).unwrap().status, TaskStatus::Cancelled);
    assert_eq!(after.deliveries.len(), before.deliveries.len());
    assert_eq!(after.revision, before.revision);
}

#[test]
fn runtime_failure_requires_reconciling_dispatched_work_before_resume() {
    let mut f = Fixture::new();
    f.configure();
    let mut run = f.load();
    run.deliveries[0].status = DeliveryStatus::Dispatched;
    run.deliveries[0].runtime_generation = Some(7);
    run.block("Runtime disconnected during delivery");
    assert_eq!(run.status, RunStatus::Blocked);
    assert_eq!(run.deliveries[0].status, DeliveryStatus::Uncertain);
    assert!(run.ready_tasks(0).is_empty());
    assert!(run.resume().is_err());
    run.deliveries[0].status = DeliveryStatus::Acknowledged;
    run.resume().unwrap();
    assert_eq!(run.status, RunStatus::Preparing);
}

#[test]
fn paused_teammate_corrections_survive_resume_and_other_assignments_keep_running() {
    let mut f = Fixture::new();
    f.configure();
    let ids = f.load().ready_tasks(0);
    let child = f.prepare(ids[0]);
    let workspace = child.runtime_path().to_path_buf();
    fs::write(workspace.join("partial.txt"), "keep this work").unwrap();
    let old_revision = f.load().task(ids[0]).unwrap().revision;
    f.store
        .update_delegation(f.run, None, |r| {
            let task = r.task_mut(ids[0])?;
            task.paused_status = Some(task.status);
            task.status = TaskStatus::Paused;
            r.wait = Some(ide_core::delegation::WaitCondition {
                after_sequence: r.events.last().map_or(0, |e| e.sequence),
                tasks: vec![ids[0]],
            });
            r.user_correction(ids[0], "Keep the video to thirty seconds".into())?;
            r.message(
                r.parent_agent_id,
                ids[0],
                "Keep the original voiceover".into(),
                false,
            )?;
            Ok(())
        })
        .unwrap();
    let paused = f.load();
    assert_eq!(paused.task(ids[0]).unwrap().status, TaskStatus::Paused);
    assert!(paused.wait_satisfied());
    assert!(paused.ready_tasks(0).contains(&ids[1]));
    let source = Uuid::new_v4();
    assert_eq!(
        f.store
            .prepare_delegation_submission(
                f.parent.id,
                source,
                "Add another teammate to check accessibility",
                &[],
                false
            )
            .unwrap(),
        Some(f.run)
    );
    let expanded = f.load();
    let mut plan = expanded.tasks[1].plan.clone();
    plan.key = "accessibility".into();
    plan.expert_id = source;
    let expert = expanded
        .temporary_experts
        .iter()
        .find(|e| e.profile.id == source)
        .unwrap()
        .clone();
    f.store
        .update_delegation(f.run, None, |r| {
            r.add_plans(r.parent_agent_id, vec![plan], &[expert])
        })
        .unwrap();
    assert_eq!(f.load().tasks.len(), 3);
    assert_eq!(f.load().task(ids[0]).unwrap().status, TaskStatus::Paused);
    // The same aggregate survives a restart without executing a queued message.
    let reopened = LocalStore::open(f.store.root().to_path_buf()).unwrap();
    assert_eq!(
        reopened
            .load_delegation(f.run)
            .unwrap()
            .task(ids[0])
            .unwrap()
            .status,
        TaskStatus::Paused
    );
    f.store
        .update_delegation(f.run, None, |r| {
            r.deliveries
                .iter_mut()
                .find(|d| d.target == child.id)
                .unwrap()
                .status = DeliveryStatus::Uncertain;
            Ok(())
        })
        .unwrap();
    assert!(f
        .store
        .update_delegation(f.run, None, |r| r.resume_user_task(ids[0]))
        .is_err());
    assert_eq!(f.load().task(ids[0]).unwrap().status, TaskStatus::Paused);
    f.store
        .update_delegation(f.run, None, |r| {
            r.deliveries
                .iter_mut()
                .find(|d| d.target == child.id && d.status == DeliveryStatus::Uncertain)
                .unwrap()
                .status = DeliveryStatus::Acknowledged;
            Ok(())
        })
        .unwrap();
    f.store
        .update_delegation(f.run, None, |r| r.resume_user_task(ids[0]))
        .unwrap();
    let resumed = f.load();
    assert!(resumed.task(ids[0]).unwrap().revision > old_revision);
    assert_eq!(
        resumed
            .task(ids[0])
            .unwrap()
            .attempt()
            .unwrap()
            .child_agent_id,
        child.id
    );
    assert_eq!(
        fs::read_to_string(workspace.join("partial.txt")).unwrap(),
        "keep this work"
    );
    let deliveries = resumed
        .deliveries
        .iter()
        .filter(|d| d.target == child.id && d.status == DeliveryStatus::Queued)
        .map(|d| d.id)
        .collect();
    let effect = f
        .job(Job::Deliver(
            resumed.clone(),
            child,
            deliveries,
            AgentInteractionMode::Default,
        ))
        .unwrap();
    let Effect::Send(_, _, text, _, _) = effect else {
        panic!("Expected correction delivery");
    };
    assert!(text.contains("thirty seconds"));
    assert!(text.contains("original voiceover"));
    assert!(text.contains(&format!(
        "Current task revision: {}",
        resumed.task(ids[0]).unwrap().revision
    )));
}

#[test]
fn corrections_before_start_and_after_integration_use_the_current_baseline() {
    let mut f = Fixture::new();
    let task = f.load().tasks[0].id;
    f.store
        .update_delegation(f.run, None, |r| {
            r.user_correction(task, "Use compact controls".into())
        })
        .unwrap();
    assert_eq!(f.load().task(task).unwrap().status, TaskStatus::Queued);
    f.configure();
    let first = f.prepare(task);
    assert!(f
        .load()
        .task(task)
        .unwrap()
        .plan
        .brief
        .contains("compact controls"));
    fs::write(first.runtime_path().join("first.txt"), "first result").unwrap();
    f.report(task);
    f.integrate(task);
    fs::write(
        f.parent.runtime_path().join("later.txt"),
        "later parent work",
    )
    .unwrap();
    f.store
        .update_delegation(f.run, None, |r| {
            r.user_correction(task, "Add keyboard support".into())
        })
        .unwrap();
    assert_eq!(f.load().task(task).unwrap().status, TaskStatus::Queued);
    let second = f.prepare(task);
    assert_eq!(first.id, second.id);
    assert_ne!(first.runtime_path(), second.runtime_path());
    assert_eq!(
        fs::read_to_string(second.runtime_path().join("later.txt")).unwrap(),
        "later parent work"
    );
    assert_eq!(
        fs::read_to_string(second.runtime_path().join("first.txt")).unwrap(),
        "first result"
    );
    fs::write(
        second.runtime_path().join("keyboard.txt"),
        "keyboard support",
    )
    .unwrap();
    f.report(task);
    f.integrate(task);
    assert_eq!(
        fs::read_to_string(f.parent.runtime_path().join("first.txt")).unwrap(),
        "first result"
    );
    assert!(first.runtime_path().exists());
}

#[test]
fn paused_integration_cannot_lose_its_result_to_a_correction() {
    let mut f = Fixture::new();
    f.configure();
    let task = f.load().tasks[0].id;
    let child = f.prepare(task);
    fs::write(child.runtime_path().join("output.txt"), "ready work").unwrap();
    f.report(task);
    f.store
        .update_delegation(f.run, None, |r| {
            r.task_mut(task)?.status = TaskStatus::Paused;
            r.task_mut(task)?.paused_status = Some(TaskStatus::Integrating);
            Ok(())
        })
        .unwrap();
    let before = serde_json::to_value(f.load()).unwrap();
    assert!(f
        .store
        .update_delegation(f.run, None, |r| r
            .user_correction(task, "Replace the result".into()))
        .is_err());
    assert!(f
        .store
        .update_delegation(f.run, None, |r| r.message(
            r.parent_agent_id,
            task,
            "Replace the result".into(),
            false
        ))
        .is_err());
    assert_eq!(serde_json::to_value(f.load()).unwrap(), before);
}

#[test]
fn preparing_assignment_accepts_corrections_without_losing_a_users_pause() {
    let mut f = Fixture::new();
    f.configure();
    let task_id = f.load().tasks[0].id;
    let child = f.prepare(task_id);
    let mut run = f.load();
    let task = run.task_mut(task_id).unwrap();
    let revision = task.revision;
    let attempt_id = task.attempt().unwrap().id;
    let snapshot = task.attempt_mut().unwrap().snapshot.take().unwrap();
    task.status = TaskStatus::Paused;
    task.paused_status = Some(TaskStatus::Preparing);
    run.user_correction(task_id, "Use the revised video script".into())
        .unwrap();
    run.message(
        run.parent_agent_id,
        task_id,
        "Retain the closing title".into(),
        false,
    )
    .unwrap();
    assert_eq!(
        run.task(task_id).unwrap().paused_status,
        Some(TaskStatus::Preparing)
    );
    let mut context = "Original assignment".to_string();
    assert!(record_prepared_assignment(
        &mut run,
        task_id,
        attempt_id,
        revision,
        snapshot,
        &mut context
    )
    .unwrap());
    let task = run.task(task_id).unwrap();
    assert_eq!(task.status, TaskStatus::Paused);
    assert_eq!(task.paused_status, Some(TaskStatus::Running));
    assert!(task.attempt().unwrap().snapshot.is_some());
    assert!(context.contains("revised video script"));
    assert!(context.contains("Retain the closing title"));
    assert!(context.contains(&format!("Current task revision: {}", task.revision)));
    assert_eq!(task.attempt().unwrap().child_agent_id, child.id);
    run.resume_user_task(task_id).unwrap();
    assert_eq!(run.task(task_id).unwrap().status, TaskStatus::Running);
}

#[test]
fn resume_before_initial_snapshot_reprepares_without_a_new_chat() {
    let mut f = Fixture::new();
    f.configure();
    let task_id = f.load().tasks[0].id;
    let first = f.prepare(task_id);
    f.store
        .update_delegation(f.run, None, |r| {
            let task = r.task_mut(task_id)?;
            task.attempt_mut().unwrap().snapshot = None;
            task.status = TaskStatus::Paused;
            task.paused_status = Some(TaskStatus::Preparing);
            r.user_correction(task_id, "Use the corrected brief".into())?;
            r.resume_user_task(task_id)
        })
        .unwrap();
    assert!(f.load().ready_tasks(0).contains(&task_id));
    let second = f.prepare(task_id);
    assert_eq!(second.id, first.id);
    assert!(second.doc.contains("corrected brief"));
    assert_eq!(f.load().task(task_id).unwrap().status, TaskStatus::Running);
    assert!(first.runtime_path().exists());
}
