//! Opt-in end-to-end provider fixture. Runtime effects use the real adapters;
//! scheduling drives the same durable coordinator jobs without controlling UI.
use super::*;
use crate::state::agent_chat::protocol::{
    spawn_chat_backend, ChatBackendCommand, ChatBackendController, ChatBackendEvent,
};
use std::collections::HashMap;

struct Runtime {
    controller: ChatBackendController,
    events: async_channel::Receiver<ChatBackendEvent>,
    busy: bool,
    generation: u64,
    settled: bool,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.controller.force_shutdown();
    }
}

fn apply(
    f: &mut Fixture,
    effect: Effect,
    runtimes: &mut HashMap<Uuid, Runtime>,
    next_generation: &mut u64,
) {
    match effect {
        Effect::None => {}
        Effect::Adopt(a) => {
            if let Some(previous) = runtimes.remove(&a.id) {
                let signal = previous.controller.stop_signal();
                drop(previous);
                let deadline = std::time::Instant::now() + Duration::from_secs(10);
                while !signal.is_stopped() {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "Old runtime did not stop"
                    );
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            f.adopt(Effect::Adopt(a));
        }
        Effect::Send(a, _, text, mode, ids) => {
            let target = a.id;
            let runtime = runtimes.entry(target).or_insert_with(|| {
                *next_generation += 1;
                let (controller, events) = spawn_chat_backend(a.clone(), mode).unwrap();
                Runtime {
                    controller,
                    events,
                    busy: false,
                    generation: *next_generation,
                    settled: false,
                }
            });
            assert!(!runtime.busy);
            runtime.busy = true;
            runtime.settled = false;
            f.store
                .append_chat_message(
                    target,
                    "user",
                    text.clone(),
                    ide_core::agents::unix_now(),
                    None,
                )
                .unwrap();
            runtime
                .controller
                .send(ChatBackendCommand::SendTurn { turn_id: uuid::Uuid::new_v4().to_string(),
                    text,
                    mode,
                    read_only: false,
                })
                .unwrap();
            f.store
                .update_delegation(f.run, None, |r| {
                    for d in &mut r.deliveries {
                        if ids.contains(&d.id) {
                            d.runtime_generation = Some(runtime.generation);
                        }
                    }
                    for task in &mut r.tasks {
                        if let Some(attempt) = task.attempt_mut() {
                            if attempt.child_agent_id == target {
                                attempt.generation = runtime.generation;
                            }
                        }
                    }
                    Ok(())
                })
                .unwrap();
        }
    }
}

#[test]
#[ignore = "real Codex and Claude with fresh disposable CHORO_DATA_DIR; no UI control"]
fn natural_language_two_expert_provider_acceptance() {
    let root = std::path::PathBuf::from(
        std::env::var("CHORO_DATA_DIR").expect("Provide a disposable fixture directory"),
    );
    assert!(root
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("choro-delegation-acceptance-"));
    assert!(!root.join("data").exists());
    let mut f = Fixture::with_store_root(Some(root));
    let mut parent = f.parent.clone();
    if std::env::var("CHORO_ACCEPTANCE_LEAD").is_ok_and(|p| p == "claude") {
        parent.provider = AgentKind::Claude;
        parent.model = AgentModel::default_for(parent.provider);
        parent.effort = parent.model.default_effort();
    }
    parent.access_mode = AgentAccessMode::FullAccess;
    parent.doc="Disposable local test project. Preserve all existing files. No external services or package installation are needed. Verify through terminal commands only; do not control browsers or desktop applications.".into();
    f.adopt(Effect::Adopt(parent));
    let assignment="Build a small browser to-do list with add, complete and remove. Delegate design to UI Designer and backend to Backend Master. Do the rest yourself. This is a disposable acceptance fixture: UI Designer owns index.html, Backend Master owns store.js (localStorage persistence). Establish the shared contract before parallel work, wire the files together, and verify the result. Preserve user.txt. No packages or external services. Use Choro's delegation tools and finish the managed run with combined verification.";
    f.store
        .update_delegation(f.run, None, |r| {
            r.tasks.clear();
            r.original_assignment = assignment.into();
            Ok(())
        })
        .unwrap();
    f.store
        .append_chat_message(
            f.parent.id,
            "user",
            assignment,
            ide_core::agents::unix_now(),
            None,
        )
        .unwrap();
    let mut runtimes = HashMap::new();
    let mut generation = 0;
    let first_parent = f.parent.clone();
    apply(
        &mut f,
        Effect::Send(
            first_parent,
            vec![],
            assignment.into(),
            AgentInteractionMode::Default,
            vec![],
        ),
        &mut runtimes,
        &mut generation,
    );
    let started = std::time::Instant::now();
    let mut last_state = String::new();
    loop {
        assert!(
            started.elapsed() < Duration::from_secs(600),
            "Acceptance timed out; state: {}",
            serde_json::to_string(&f.load()).unwrap()
        );
        for (id, runtime) in &mut runtimes {
            while let Ok(event) = runtime.events.try_recv() {
                match event {
                    ChatBackendEvent::Status(AgentChatStatus::Running) => {
                        runtime.busy = true;
                        runtime.settled = false;
                    }
                    ChatBackendEvent::Status(AgentChatStatus::Idle) if runtime.busy => {
                        runtime.busy = false;
                        runtime.settled = true;
                    }
                    ChatBackendEvent::Status(AgentChatStatus::Failed) => {
                        panic!("Provider {id} failed")
                    }
                    ChatBackendEvent::ChatSessionReady { session_id } => {
                        let mut agents = f.store.load_agents().unwrap();
                        let a = agents.iter_mut().find(|a| a.id == *id).unwrap();
                        a.chat_session_id = Some(session_id);
                        a.started_at = Some(ide_core::agents::unix_now());
                        if *id == f.parent.id {
                            f.parent = a.clone();
                        }
                        f.store.save_agents(&agents).unwrap();
                    }
                    ChatBackendEvent::SessionReady { session_id } => {
                        let mut agents = f.store.load_agents().unwrap();
                        let a = agents.iter_mut().find(|a| a.id == *id).unwrap();
                        a.cli_session_id = Some(session_id);
                        a.started_at = Some(ide_core::agents::unix_now());
                        if *id == f.parent.id {
                            f.parent = a.clone();
                        }
                        f.store.save_agents(&agents).unwrap();
                    }
                    ChatBackendEvent::AssistantChunk { message_id, text } => {
                        f.store
                            .upsert_chat_message(
                                *id,
                                "assistant",
                                text,
                                ide_core::agents::unix_now(),
                                message_id,
                            )
                            .unwrap();
                    }
                    ChatBackendEvent::PendingApproval(p) => panic!("Human approval needed: {p:?}"),
                    ChatBackendEvent::PendingUserInput(p) => panic!("Human decision needed: {p:?}"),
                    ChatBackendEvent::Error(e) => panic!("Provider error: {e}"),
                    _ => {}
                }
            }
        }
        let run = f.load();
        let state = format!(
            "{:?} {:?}",
            run.status,
            run.tasks
                .iter()
                .map(|t| (&t.plan.key, t.status))
                .collect::<Vec<_>>()
        );
        if state != last_state {
            eprintln!("{state}");
            last_state = state;
        }
        if run.status == RunStatus::Completed && runtimes.values().all(|r| !r.busy) {
            break;
        }
        assert!(
            !matches!(
                run.status,
                RunStatus::Blocked | RunStatus::Paused | RunStatus::Interrupted
            ),
            "Run needs attention: {:?}",
            run.pause_reason
        );
        let safe = |id| runtimes.get(&id).is_none_or(|r| !r.busy);
        let mut job = None;
        if run.status == RunStatus::Preparing && !run.tasks.is_empty() && safe(run.parent_agent_id)
        {
            job = Some(Job::Configure(run.clone(), f.parent.clone()));
        }
        if run.status.dispatchable() {
            if let Some(d) = run.deliveries.iter().find(|d| {
                d.status == DeliveryStatus::Dispatched
                    && runtimes
                        .get(&d.target)
                        .is_some_and(|r| r.settled && Some(r.generation) == d.runtime_generation)
            }) {
                job = Some(Job::Acknowledge(
                    run.clone(),
                    d.target,
                    d.runtime_generation.unwrap(),
                ));
            }
            if job.is_none() {
                for task in &run.tasks {
                    if let Some(a) = task.attempt() {
                        let settled = runtimes
                            .get(&a.child_agent_id)
                            .is_some_and(|r| r.settled && r.generation == a.generation);
                        let pending = run.deliveries.iter().any(|d| {
                            d.target == a.child_agent_id && d.status != DeliveryStatus::Acknowledged
                        });
                        if settled && !pending && task.status == TaskStatus::CompletionRequested {
                            job = Some(Job::Capture(run.clone(), task.id));
                            break;
                        }
                        if settled && !pending && task.status == TaskStatus::Running {
                            job = Some(Job::MissingReport(run.clone(), task.id));
                            break;
                        }
                        if task.status == TaskStatus::Integrating && safe(run.parent_agent_id) {
                            job = Some(Job::Integrate(run.clone(), task.id));
                            break;
                        }
                    }
                }
            }
            if job.is_none() && safe(run.parent_agent_id) {
                if let Some(task) = run
                    .ready_tasks(
                        run.tasks
                            .iter()
                            .filter(|t| t.status.occupies_slot())
                            .count(),
                    )
                    .first()
                {
                    job = Some(Job::Prepare(run.clone(), *task, f.parent.clone()));
                }
            }
            if job.is_none() {
                for d in &run.deliveries {
                    if d.status != DeliveryStatus::Queued || !safe(d.target) {
                        continue;
                    }
                    if d.target == run.parent_agent_id
                        && run.wait.is_some()
                        && !run.wait_satisfied()
                    {
                        continue;
                    }
                    if run.deliveries.iter().any(|other| {
                        other.target == d.target
                            && matches!(
                                other.status,
                                DeliveryStatus::Dispatched | DeliveryStatus::Uncertain
                            )
                    }) {
                        continue;
                    }
                    let agents = f.store.load_agents().unwrap();
                    let Some(agent) = agents.into_iter().find(|a| a.id == d.target) else {
                        continue;
                    };
                    let ids = run
                        .deliveries
                        .iter()
                        .filter(|x| x.target == d.target && x.status == DeliveryStatus::Queued)
                        .map(|x| x.id)
                        .collect();
                    job = Some(Job::Deliver(
                        run.clone(),
                        agent,
                        ids,
                        AgentInteractionMode::Default,
                    ));
                    break;
                }
            }
        }
        if let Some(job) = job {
            let effect = execute_in_store(&f.store, job, managed::preflight, None).unwrap();
            apply(&mut f, effect, &mut runtimes, &mut generation);
        } else {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    assert!(f.load().tasks.len() >= 2);
    assert!(f
        .load()
        .tasks
        .iter()
        .all(|t| t.status == TaskStatus::Integrated));
    assert_eq!(
        fs::read_to_string(f.parent.runtime_path().join("user.txt")).unwrap(),
        "unfinished user work\n"
    );
    assert!(f.parent.runtime_path().join("index.html").exists());
    assert!(f.parent.runtime_path().join("store.js").exists());
    assert!(f.load().verification.is_some());
}
