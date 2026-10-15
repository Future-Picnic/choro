use super::*;
use ide_core::delegation::{DelegationAttempt, DelegationLimits, TaskKind, TaskPlan};
use ide_core::experts::ExpertSnapshot;

fn fixture() -> DelegationRun {
    let mut profile = ide_core::experts::catalog::catalog().experts[0].profile();
    profile.name = "Designer".into();
    profile.skills.clear();
    let expert = ExpertSnapshot {
        profile,
        skills: vec![],
    };
    let mut run = DelegationRun::new(
        Uuid::new_v4(),
        ProjectId::new(),
        Uuid::new_v4(),
        "Build a page".into(),
        vec![expert.profile.id],
        false,
        DelegationLimits::default(),
    );
    let plans = ["Layout", "Settings"]
        .into_iter()
        .map(|goal| TaskPlan {
            model_request: None,
            key: goal.into(),
            expert_id: expert.profile.id,
            goal: goal.into(),
            brief: format!("Implement {goal}"),
            expected_outcome: "Working page".into(),
            repository: "/in-memory/companion".into(),
            dependencies: vec![],
            kind: TaskKind::Implementation,
            held: false,
        })
        .collect();
    run.add_plans(run.parent_agent_id, plans, &[expert])
        .unwrap();
    run.status = RunStatus::Active;
    for task in &mut run.tasks {
        task.status = TaskStatus::Running;
        task.attempts.push(DelegationAttempt {
            id: Uuid::new_v4(),
            child_agent_id: Uuid::new_v4(),
            generation: 1,
            workspace: task.plan.repository.clone(),
            prepared: true,
            snapshot: None,
            completed_snapshot: None,
            archive_snapshot: None,
            working_copy_cleaned: false,
            result: None,
            result_revision: None,
            report_requested: false,
            session_id: None,
            progress: String::new(),
            usage: None,
        });
    }
    run
}

#[test]
fn durable_bandmates_appear_without_loaded_chat_sessions_and_open_their_own_chats() {
    // The lead can be idle and managed child sessions hidden or not loaded.
    // Restored coordinator metadata alone must expose both assignments.
    let run: DelegationRun =
        serde_json::from_str(&serde_json::to_string(&fixture()).unwrap()).unwrap();
    let items: Vec<_> = run
        .tasks
        .iter()
        .filter_map(|task| bandmate_activity_item(&run, task, "Website", 12, false))
        .collect();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "Designer · Layout");
    assert_eq!(items[1].title, "Designer · Settings");
    assert_ne!(items[0].assignment_id, items[1].assignment_id);
    for (item, task) in items.iter().zip(&run.tasks) {
        assert_eq!(item.agent_id, task.attempt().unwrap().child_agent_id);
        assert_eq!(item.project_id, run.project_id);
        assert_eq!(item.status, "Bandmate · Working");
        assert_eq!(item.animation, CompanionAnimation::Working);
        assert!(!item.acknowledge_on_open);
    }
    let mut next = run.clone();
    let mut attempt = next.tasks[0].attempt().unwrap().clone();
    attempt.id = Uuid::new_v4();
    attempt.child_agent_id = Uuid::new_v4();
    next.tasks[0].attempts.push(attempt.clone());
    assert_eq!(
        bandmate_activity_item(&next, &next.tasks[0], "Website", 13, false)
            .unwrap()
            .agent_id,
        attempt.child_agent_id
    );
}

#[test]
fn bandmate_lifecycle_does_not_keep_settled_or_stopped_work_spinning() {
    let mut run = fixture();
    for (status, label) in [
        (TaskStatus::Preparing, "Preparing Bandmate"),
        (TaskStatus::Running, "Working"),
        (TaskStatus::CompletionRequested, "Finishing"),
        (TaskStatus::WaitingForLead, "Waiting for lead"),
    ] {
        run.tasks[0].status = status;
        let item = bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).unwrap();
        assert_eq!(item.status, format!("Bandmate · {label}"));
        assert_eq!(item.animation, CompanionAnimation::Working);
    }
    for status in [
        TaskStatus::Planned,
        TaskStatus::Queued,
        TaskStatus::ResultReady,
        TaskStatus::Accepted,
        TaskStatus::Cancelled,
        TaskStatus::Superseded,
        TaskStatus::Paused,
    ] {
        run.tasks[0].status = status;
        assert!(
            bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).is_none(),
            "{status:?}"
        );
    }
    run.tasks[0].status = TaskStatus::Running;
    for status in [
        RunStatus::Paused,
        RunStatus::Interrupted,
        RunStatus::Completed,
        RunStatus::Cancelled,
    ] {
        run.status = status;
        assert!(
            bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).is_none(),
            "{status:?}"
        );
    }
    run.status = RunStatus::Waiting;
    assert!(bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).is_some());
    run.execution_version = 0;
    assert!(bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).is_none());
}

#[test]
fn bandmate_attention_wins_over_work_and_work_wins_over_old_done_notices() {
    let mut run = fixture();
    let working = bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).unwrap();
    let mut done = working.clone();
    done.assignment_id = None;
    done.animation = CompanionAnimation::Done;
    assert_eq!(
        primary_animation(&[done.clone(), working.clone()], None),
        CompanionAnimation::Working
    );
    let attention = bandmate_activity_item(&run, &run.tasks[0], "Website", 12, true).unwrap();
    assert_eq!(attention.status, "Bandmate · Needs your input");
    assert_eq!(
        primary_animation(&[working, done, attention], None),
        CompanionAnimation::NeedsAttention
    );
    for status in [TaskStatus::Failed, TaskStatus::NeedsUser] {
        run.tasks[0].status = status;
        assert_eq!(
            bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false)
                .unwrap()
                .animation,
            CompanionAnimation::NeedsAttention
        );
    }
    run.tasks[0].status = TaskStatus::Running;
    run.status = RunStatus::Blocked;
    assert_eq!(
        bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false)
            .unwrap()
            .status,
        "Bandmate · Needs attention"
    );
}

#[test]
fn preparation_without_a_child_routes_to_lead_with_distinct_assignment_rows() {
    let mut run = fixture();
    for task in &mut run.tasks {
        task.status = TaskStatus::Preparing;
        task.attempts.clear();
    }
    let first = bandmate_activity_item(&run, &run.tasks[0], "Website", 12, false).unwrap();
    let second = bandmate_activity_item(&run, &run.tasks[1], "Website", 12, false).unwrap();
    assert_eq!(first.agent_id, run.parent_agent_id);
    assert_eq!(second.agent_id, run.parent_agent_id);
    assert_ne!(first.assignment_id, second.assignment_id);
}
