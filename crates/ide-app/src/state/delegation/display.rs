//! Presentation state derived purely from durable coordinator runs.
//!
//! The lead's provider is usually idle while its Experts work, so no surface
//! may read provider runtime status to decide whether delegated work is in
//! progress. Everything here is computed from run/task status plus one caller
//! supplied fact per child: whether its conversation is waiting on the user.
use ide_core::delegation::{DelegationRun, DelegationTask, RunStatus, TaskStatus};
use uuid::Uuid;

/// What a delegated assignment (or the parent that owns several) is doing now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DelegationActivity {
    /// Nothing live: no run, a finished run, or a queued/finished assignment.
    #[default]
    Idle,
    /// An Expert is actively working, or the lead owes it a reply.
    Working,
    /// Stopped by the user, by Plan mode, or by an interrupted application.
    Paused,
    /// A person has to act: a question, a permission, or a failure.
    Attention,
}

/// One assignment as the sidebar and cards present it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegatedTaskRow {
    pub run_id: Uuid,
    pub task_id: Uuid,
    /// Original assignment position in this run, before any display sorting.
    pub bandmate_index: usize,
    pub child_agent_id: Option<Uuid>,
    pub expert: String,
    /// Expert name, suffixed with an ordinal when the same Expert holds more
    /// than one live assignment under this parent.
    pub label: String,
    pub goal: String,
    pub status: TaskStatus,
    pub activity: DelegationActivity,
}

/// Everything a parent row needs to know about its delegated work.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParentDelegationState {
    pub activity: DelegationActivity,
    pub live_run: bool,
    /// Assignments currently occupying an Expert.
    pub working: usize,
    /// Live assignments in run order. Finished runs contribute nothing.
    pub tasks: Vec<DelegatedTaskRow>,
}

impl ParentDelegationState {
    pub fn has_live_run(&self) -> bool {
        self.live_run
    }
}

/// Plans append assignments and revise them in place. Using that durable order
/// keeps a bandmate's instrument stable through retries, status sorting and
/// reopening history, including multiple assignments to the same profile.
pub fn bandmate_index(run: &DelegationRun, task_id: Uuid) -> Option<usize> {
    run.tasks.iter().position(|task| task.id == task_id)
}

/// Activity of one assignment inside a run of the given status.
pub fn task_activity(
    run_status: RunStatus,
    task: &DelegationTask,
    child_needs_user: bool,
) -> DelegationActivity {
    if run_status.terminal() {
        return DelegationActivity::Idle;
    }
    if run_status == RunStatus::Blocked {
        return DelegationActivity::Attention;
    }
    if matches!(run_status, RunStatus::Paused | RunStatus::Interrupted) {
        return DelegationActivity::Paused;
    }
    match task.status {
        TaskStatus::NeedsUser | TaskStatus::Failed => DelegationActivity::Attention,
        TaskStatus::Paused => DelegationActivity::Paused,
        TaskStatus::Preparing
        | TaskStatus::Running
        | TaskStatus::WaitingForLead
        | TaskStatus::CompletionRequested
        | TaskStatus::ResultReady
        | TaskStatus::Integrating => {
            if child_needs_user {
                DelegationActivity::Attention
            } else {
                DelegationActivity::Working
            }
        }
        TaskStatus::Planned
        | TaskStatus::Queued
        | TaskStatus::Integrated
        | TaskStatus::Accepted
        | TaskStatus::Cancelled
        | TaskStatus::Superseded => DelegationActivity::Idle,
    }
}

/// Activity of a whole run, as its header and the composer indicator show it.
pub fn run_activity(
    run: &DelegationRun,
    child_needs_user: &dyn Fn(Uuid) -> bool,
) -> DelegationActivity {
    if run.status.terminal() {
        return DelegationActivity::Idle;
    }
    if run.status == RunStatus::Blocked {
        return DelegationActivity::Attention;
    }
    if matches!(run.status, RunStatus::Paused | RunStatus::Interrupted) {
        return DelegationActivity::Paused;
    }
    let activities = run.tasks.iter().map(|task| {
        let needs_user = task
            .attempt()
            .is_some_and(|attempt| child_needs_user(attempt.child_agent_id));
        task_activity(run.status, task, needs_user)
    });
    fold_activities(activities).unwrap_or(if run.status == RunStatus::Preparing {
        // The lead is still planning; nothing is idle about that.
        DelegationActivity::Working
    } else {
        DelegationActivity::Idle
    })
}

fn fold_activities(
    activities: impl IntoIterator<Item = DelegationActivity>,
) -> Option<DelegationActivity> {
    activities.into_iter().reduce(|left, right| {
        use DelegationActivity::*;
        match (left, right) {
            (Attention, _) | (_, Attention) => Attention,
            (Working, _) | (_, Working) => Working,
            (Paused, _) | (_, Paused) => Paused,
            _ => Idle,
        }
    })
}

/// Activity-only lookup for sidebar filters and indicators. Do not construct
/// assignment rows (or copy their goals/reports) just to read a status.
pub fn parent_delegation_activity(
    runs: &[DelegationRun],
    parent: Uuid,
    child_needs_user: &dyn Fn(Uuid) -> bool,
) -> DelegationActivity {
    fold_activities(
        runs.iter()
            .filter(|run| run.parent_agent_id == parent && !run.status.terminal())
            .map(|run| run_activity(run, child_needs_user)),
    )
    .unwrap_or_default()
}

/// Sidebar and card state for `parent`, from every run it owns.
pub fn parent_delegation_state(
    runs: &[DelegationRun],
    parent: Uuid,
    child_needs_user: &dyn Fn(Uuid) -> bool,
) -> ParentDelegationState {
    let live = runs
        .iter()
        .filter(|run| run.parent_agent_id == parent && !run.status.terminal())
        .collect::<Vec<_>>();
    let mut tasks = Vec::new();
    for run in &live {
        for (bandmate_index, task) in run.tasks.iter().enumerate() {
            let child = task.attempt().map(|attempt| attempt.child_agent_id);
            let needs_user = child.is_some_and(child_needs_user);
            tasks.push(DelegatedTaskRow {
                run_id: run.id,
                task_id: task.id,
                bandmate_index,
                child_agent_id: child,
                expert: task.expert.profile.name.clone(),
                label: task.expert.profile.name.clone(),
                goal: task.plan.goal.clone(),
                status: task.status,
                activity: task_activity(run.status, task, needs_user),
            });
        }
    }
    let tasks = with_ordinal_labels(tasks);
    let run_level = fold_activities(live.iter().map(|run| run_activity(run, child_needs_user)));
    let activity = run_level.unwrap_or_default();
    let working = tasks
        .iter()
        .filter(|task| {
            task.activity == DelegationActivity::Working && expert_is_working(task.status)
        })
        .count();
    ParentDelegationState {
        live_run: !live.is_empty(),
        activity,
        working,
        tasks,
    }
}

/// Only unsettled Expert execution contributes to the working count. A ready
/// result can keep the lead busy without pretending its Expert is still running.
pub fn expert_is_working(status: TaskStatus) -> bool {
    matches!(
        status,
        TaskStatus::Preparing | TaskStatus::Running | TaskStatus::CompletionRequested
    )
}

/// The composer's aggregate label and glyph must describe the same state.
/// In particular, one ready result must not hide another Expert's active work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegationIndicator {
    pub label: String,
    pub activity: DelegationActivity,
    pub status: TaskStatus,
    pub live: bool,
}

impl DelegationIndicator {
    /// Keep persistent header chrome short without losing the richer composer
    /// recovery hints, which also remain available in the header tooltip.
    pub fn header_label(&self) -> &str {
        match self.label.as_str() {
            "Band interrupted · Resume available" => "Band · Interrupted",
            "Band paused · You can still message here" => "Band · Paused",
            "Band · Preparing assignments" => "Band · Preparing",
            "Band · Awaiting lead verification" => "Band · Verify",
            label => label,
        }
    }
}

pub fn delegation_indicator(
    runs: &[DelegationRun],
    parent: Uuid,
    child_needs_user: &dyn Fn(Uuid) -> bool,
) -> Option<DelegationIndicator> {
    let current = runs
        .iter()
        .find(|run| run.parent_agent_id == parent && !run.status.terminal())
        .or_else(|| runs.iter().rev().find(|run| run.parent_agent_id == parent))?;
    let activity = parent_delegation_activity(runs, parent, child_needs_user);
    let working = runs
        .iter()
        .filter(|run| run.parent_agent_id == parent && !run.status.terminal())
        .map(|run| {
            run.tasks
                .iter()
                .filter(|task| {
                    let needs_user = task
                        .attempt()
                        .is_some_and(|attempt| child_needs_user(attempt.child_agent_id));
                    expert_is_working(task.status)
                        && task_activity(run.status, task, needs_user)
                            == DelegationActivity::Working
                })
                .count()
        })
        .sum::<usize>();
    let ready = current
        .tasks
        .iter()
        .filter(|task| task.status == TaskStatus::ResultReady)
        .count();
    let all_settled = !current.tasks.is_empty()
        && current
            .tasks
            .iter()
            .all(|task| task.status.terminal() || task.status == TaskStatus::ResultReady);
    let (label, activity, status, live) = match activity {
        DelegationActivity::Attention => (
            "Band · Needs attention".into(),
            DelegationActivity::Attention,
            TaskStatus::NeedsUser,
            false,
        ),
        DelegationActivity::Paused => (
            if current.status == RunStatus::Interrupted {
                "Band interrupted · Resume available"
            } else {
                "Band paused · You can still message here"
            }
            .into(),
            DelegationActivity::Paused,
            TaskStatus::Paused,
            false,
        ),
        _ if working > 0 => (
            format!("Band · {working} working"),
            DelegationActivity::Working,
            TaskStatus::Running,
            true,
        ),
        _ if current.status == RunStatus::Completed => (
            "Band · Complete".into(),
            DelegationActivity::Idle,
            TaskStatus::Accepted,
            false,
        ),
        _ if current.status == RunStatus::Cancelled => (
            "Band · Ended".into(),
            DelegationActivity::Idle,
            TaskStatus::Cancelled,
            false,
        ),
        _ if current
            .tasks
            .iter()
            .any(|task| task.status == TaskStatus::Integrating) =>
        {
            (
                "Band · Integrating".into(),
                DelegationActivity::Working,
                TaskStatus::Integrating,
                true,
            )
        }
        _ if ready > 0 => (
            format!("Band · {ready} finished"),
            DelegationActivity::Idle,
            if all_settled {
                TaskStatus::ResultReady
            } else {
                TaskStatus::Queued
            },
            false,
        ),
        _ if current.tasks.is_empty() => (
            "Band · Preparing assignments".into(),
            DelegationActivity::Working,
            TaskStatus::Preparing,
            true,
        ),
        _ if all_settled => (
            "Band · Awaiting lead verification".into(),
            DelegationActivity::Idle,
            TaskStatus::Accepted,
            false,
        ),
        _ => (
            "Band · Waiting".into(),
            DelegationActivity::Idle,
            TaskStatus::Queued,
            false,
        ),
    };
    Some(DelegationIndicator {
        label,
        activity,
        status,
        live,
    })
}

/// Several assignments to one Expert stay distinguishable: the first keeps the
/// plain name and later ones carry their order (`UI Designer · 2`).
fn with_ordinal_labels(tasks: Vec<DelegatedTaskRow>) -> Vec<DelegatedTaskRow> {
    tasks
        .iter()
        .enumerate()
        .map(|(index, task)| {
            let same_expert = tasks
                .iter()
                .filter(|other| other.expert == task.expert)
                .count();
            if same_expert < 2 {
                return task.clone();
            }
            let ordinal = tasks[..index]
                .iter()
                .filter(|other| other.expert == task.expert)
                .count()
                + 1;
            DelegatedTaskRow {
                label: format!("{} · {ordinal}", task.expert),
                ..task.clone()
            }
        })
        .collect()
}

/// Whether a finished Expert's structured report can be shown, and whether it
/// still belongs to the assignment as currently worded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportState {
    /// No report on the latest attempt.
    None,
    /// The report answers the assignment's current revision.
    Fresh,
    /// The assignment was corrected after this report was written.
    Stale,
}

pub fn report_state(task: &DelegationTask) -> ReportState {
    match task.attempt() {
        Some(attempt) if attempt.result.is_some() => {
            if task.status == TaskStatus::CompletionRequested
                && attempt.result_revision == Some(task.revision)
            {
                return ReportState::None;
            }
            if attempt.result_revision == Some(task.revision) {
                ReportState::Fresh
            } else {
                ReportState::Stale
            }
        }
        _ => ReportState::None,
    }
}

/// The completion stage as the user should read it. Provider idleness plays
/// no part: only these durable statuses mean anything finished.
pub fn completion_stage(run_status: RunStatus, task_status: TaskStatus) -> Option<&'static str> {
    if run_status == RunStatus::Completed && task_status.satisfied() {
        return Some("Task complete");
    }
    match task_status {
        TaskStatus::ResultReady => Some("Bandmate finished"),
        TaskStatus::Integrating => Some("Integrating"),
        TaskStatus::Integrated => Some("Integrated"),
        TaskStatus::Accepted => Some("Accepted"),
        _ => None,
    }
}

/// Everything the assignments overview lists for one task, live or finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignmentEntry {
    pub run_id: Uuid,
    pub task_id: Uuid,
    pub bandmate_index: usize,
    pub child_agent_id: Option<Uuid>,
    pub expert: String,
    pub label: String,
    pub model: String,
    pub goal: String,
    pub status: TaskStatus,
    pub activity: DelegationActivity,
    /// Finished rows sit in their own de-emphasised group.
    pub finished: bool,
    pub report: ReportState,
    pub stage: Option<&'static str>,
    pub detail: String,
}

fn overview_rank(entry: &AssignmentEntry) -> u8 {
    if entry.finished {
        return 4;
    }
    match entry.activity {
        DelegationActivity::Attention => 0,
        DelegationActivity::Working => 1,
        DelegationActivity::Paused => 2,
        DelegationActivity::Idle => 3,
    }
}

/// Every assignment `parent` has ever delegated, attention and active work
/// first, finished work last. Each entry carries its own task and child ids,
/// so several assignments to one Expert never collapse into one.
pub fn assignment_overview(
    runs: &[DelegationRun],
    parent: Uuid,
    child_needs_user: &dyn Fn(Uuid) -> bool,
) -> Vec<AssignmentEntry> {
    let mut entries = Vec::new();
    let mut seen = std::collections::HashMap::<String, usize>::new();
    let mut totals = std::collections::HashMap::<String, usize>::new();
    for run in runs.iter().filter(|run| run.parent_agent_id == parent) {
        for task in &run.tasks {
            *totals.entry(task.expert.profile.name.clone()).or_default() += 1;
        }
    }
    for run in runs.iter().filter(|run| run.parent_agent_id == parent) {
        for (bandmate_index, task) in run.tasks.iter().enumerate() {
            let child = task.attempt().map(|attempt| attempt.child_agent_id);
            let needs_user = child.is_some_and(child_needs_user);
            let expert = task.expert.profile.name.clone();
            let ordinal = seen.entry(expert.clone()).or_default();
            *ordinal += 1;
            let label = if totals.get(&expert).copied().unwrap_or(0) > 1 {
                format!("{expert} · {ordinal}")
            } else {
                expert.clone()
            };
            entries.push(AssignmentEntry {
                run_id: run.id,
                task_id: task.id,
                bandmate_index,
                child_agent_id: child,
                model: task.expert.profile.model.label().to_string(),
                goal: task.plan.goal.clone(),
                status: task.status,
                activity: task_activity(run.status, task, needs_user),
                finished: run.status.terminal()
                    || task.status.terminal()
                    || task.status == TaskStatus::ResultReady,
                detail: if report_state(task) == ReportState::Fresh
                    && !expert_is_working(task.status)
                {
                    task.attempt()
                        .and_then(|a| a.result.as_ref())
                        .map(|r| bounded_text(&r.summary, 180).0)
                        .unwrap_or_default()
                } else if !task.plan.dependencies.is_empty()
                    && matches!(task.status, TaskStatus::Planned | TaskStatus::Queued)
                {
                    format!("Waiting for {}", task.plan.dependencies.join(", "))
                } else {
                    task.reason
                        .clone()
                        .or_else(|| task.attempt().map(|a| a.progress.clone()))
                        .unwrap_or_default()
                },
                report: report_state(task),
                stage: completion_stage(run.status, task.status),
                expert,
                label,
            });
        }
    }
    entries.sort_by_key(overview_rank);
    entries
}

/// Cut long report text for a first look without breaking a character. The
/// flag tells the caller a "show full" affordance is needed.
pub fn bounded_text(text: &str, max_chars: usize) -> (String, bool) {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max_chars {
        return (trimmed.to_string(), false);
    }
    let head: String = trimmed.chars().take(max_chars).collect();
    let cut = head
        .rfind(char::is_whitespace)
        .filter(|at| *at > max_chars / 2)
        .unwrap_or(head.len());
    (format!("{}…", head[..cut].trim_end()), true)
}

#[cfg(test)]
mod tests {
    #[test]
    fn header_labels_preserve_status_without_composer_recovery_instructions() {
        use super::{DelegationActivity, DelegationIndicator};
        use ide_core::delegation::TaskStatus;
        for (label, expected) in [
            ("Band · 2 working", "Band · 2 working"),
            ("Band · Needs attention", "Band · Needs attention"),
            ("Band · Complete", "Band · Complete"),
            ("Band · Ended", "Band · Ended"),
            ("Band interrupted · Resume available", "Band · Interrupted"),
            ("Band paused · You can still message here", "Band · Paused"),
            ("Band · Preparing assignments", "Band · Preparing"),
            ("Band · Awaiting lead verification", "Band · Verify"),
        ] {
            let indicator = DelegationIndicator {
                label: label.into(),
                activity: DelegationActivity::Idle,
                status: TaskStatus::Queued,
                live: false,
            };
            assert_eq!(indicator.header_label(), expected);
            assert_eq!(indicator.label, label, "composer text must stay unchanged");
        }
    }

    use super::*;
    use ide_core::delegation::{DelegationAttempt, DelegationLimits, TaskKind, TaskPlan};
    use ide_core::experts::{ExpertProfile, ExpertSnapshot};
    use ide_core::{AgentKind, AgentModel, ProjectId};

    fn expert(name: &str) -> ExpertSnapshot {
        let model = AgentModel::default_for(AgentKind::Codex);
        ExpertSnapshot {
            profile: ExpertProfile {
                id: Uuid::new_v4(),
                revision: 0,
                name: name.into(),
                description: name.into(),
                provider: AgentKind::Codex,
                model,
                effort: model.default_effort(),
                instructions: String::new(),
                skills: vec![],
                expected_outcome: String::new(),
                enabled: true,
                archived: false,
                additions: Default::default(),
            },
            skills: vec![],
        }
    }

    fn task(name: &str, goal: &str, status: TaskStatus, child: Option<Uuid>) -> DelegationTask {
        let expert = expert(name);
        DelegationTask {
            id: Uuid::new_v4(),
            plan: TaskPlan {
                model_request: None,
                key: goal.to_lowercase().replace(' ', "-"),
                expert_id: expert.profile.id,
                goal: goal.into(),
                brief: String::new(),
                expected_outcome: String::new(),
                repository: "/tmp/repo".into(),
                dependencies: vec![],
                kind: TaskKind::Implementation,
                held: false,
            },
            expert,
            revision: 1,
            status,
            attempts: child
                .map(|child| {
                    vec![DelegationAttempt {
                        id: Uuid::new_v4(),
                        child_agent_id: child,
                        generation: 1,
                        workspace: "/tmp/work".into(),
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
                    }]
                })
                .unwrap_or_default(),
            reason: None,
            integration: None,
            deletion_confirmation: None,
            preview: None,
            paused_status: None,
        }
    }

    fn run(parent: Uuid, status: RunStatus, tasks: Vec<DelegationTask>) -> DelegationRun {
        let mut run = DelegationRun::new(
            parent,
            ProjectId::new(),
            Uuid::new_v4(),
            "assignment".into(),
            vec![],
            false,
            DelegationLimits::default(),
        );
        run.status = status;
        run.tasks = tasks;
        run
    }

    fn nobody_waiting(_: Uuid) -> bool {
        false
    }

    #[test]
    fn activity_lookup_matches_rows_across_run_states_and_child_attention() {
        let parent = Uuid::new_v4();
        let child = Uuid::new_v4();
        for status in [
            RunStatus::Preparing,
            RunStatus::Active,
            RunStatus::Waiting,
            RunStatus::Paused,
            RunStatus::Interrupted,
            RunStatus::Blocked,
            RunStatus::Completed,
            RunStatus::Cancelled,
        ] {
            for task_status in [
                TaskStatus::Queued,
                TaskStatus::Running,
                TaskStatus::WaitingForLead,
                TaskStatus::ResultReady,
                TaskStatus::Integrated,
                TaskStatus::Failed,
                TaskStatus::NeedsUser,
                TaskStatus::Paused,
            ] {
                for waiting in [false, true] {
                    let runs = vec![
                        run(Uuid::new_v4(), RunStatus::Blocked, vec![]),
                        run(
                            parent,
                            RunStatus::Completed,
                            vec![task("Old", "History", TaskStatus::Failed, None)],
                        ),
                        run(
                            parent,
                            status,
                            vec![task("Expert", "Work", task_status, Some(child))],
                        ),
                    ];
                    let needs_user = |id| waiting && id == child;
                    assert_eq!(
                        parent_delegation_activity(&runs, parent, &needs_user),
                        parent_delegation_state(&runs, parent, &needs_user).activity,
                        "{status:?} / {task_status:?} / waiting={waiting}"
                    );
                }
            }
            let empty_run = [run(parent, status, vec![])];
            assert_eq!(
                parent_delegation_activity(&empty_run, parent, &nobody_waiting),
                parent_delegation_state(&empty_run, parent, &nobody_waiting).activity
            );
        }
    }

    #[test]
    #[ignore = "local sidebar status microbenchmark"]
    fn sidebar_activity_lookup_benchmark() {
        use std::{hint::black_box, time::Instant};
        let parent = Uuid::new_v4();
        let goal = "Assignment context with a long conversation. ".repeat(10_000);
        let runs = vec![run(
            parent,
            RunStatus::Completed,
            (0..24)
                .map(|_| task("Expert", &goal, TaskStatus::Integrated, None))
                .collect(),
        )];
        let before = Instant::now();
        for _ in 0..1_000 {
            // The old sidebar status path also built history when no run was live.
            let state = parent_delegation_state(black_box(&runs), parent, &nobody_waiting);
            if !state.has_live_run() {
                black_box(assignment_overview(&runs, parent, &nobody_waiting));
            }
            black_box(state.activity);
        }
        let old = before.elapsed();
        let after = Instant::now();
        for _ in 0..1_000 {
            black_box(parent_delegation_activity(
                black_box(&runs),
                parent,
                &nobody_waiting,
            ));
        }
        eprintln!(
            "1,000 sidebar status lookups, 24 long assignments: before={old:?}, after={:?}",
            after.elapsed()
        );
    }

    #[test]
    fn composer_indicator_distinguishes_completed_and_cancelled_runs() {
        let parent = Uuid::new_v4();
        for (run_status, task_status, label, glyph_status) in [
            (
                RunStatus::Completed,
                TaskStatus::Integrated,
                "Band · Complete",
                TaskStatus::Accepted,
            ),
            (
                RunStatus::Cancelled,
                TaskStatus::Cancelled,
                "Band · Ended",
                TaskStatus::Cancelled,
            ),
        ] {
            let runs = vec![run(
                parent,
                run_status,
                vec![task("UI Designer", "Design", task_status, None)],
            )];
            let indicator = delegation_indicator(&runs, parent, &nobody_waiting).unwrap();
            assert_eq!(indicator.label, label);
            assert_eq!(indicator.status, glyph_status);
            assert_eq!(indicator.activity, DelegationActivity::Idle);
            assert!(!indicator.live);
        }
    }

    #[test]
    fn a_ready_result_does_not_hide_other_work_in_the_composer() {
        let parent = Uuid::new_v4();
        let mut runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                task("UI Designer", "Design", TaskStatus::ResultReady, None),
                task("Backend", "Persistence", TaskStatus::Running, None),
            ],
        )];
        for (other_status, expected_status, live) in [
            (TaskStatus::Running, TaskStatus::Running, true),
            (TaskStatus::Integrating, TaskStatus::Integrating, true),
            (TaskStatus::Queued, TaskStatus::Queued, false),
            (TaskStatus::Accepted, TaskStatus::ResultReady, false),
        ] {
            runs[0].tasks[1].status = other_status;
            let indicator = delegation_indicator(&runs, parent, &nobody_waiting).unwrap();
            assert_eq!(indicator.status, expected_status, "{other_status:?}");
            assert_eq!(indicator.live, live, "{other_status:?}");
        }
    }

    #[test]
    fn a_new_run_and_pending_verification_do_not_inherit_an_old_complete_label() {
        let parent = Uuid::new_v4();
        let mut runs = vec![
            run(
                parent,
                RunStatus::Completed,
                vec![task(
                    "UI Designer",
                    "Old design",
                    TaskStatus::Integrated,
                    None,
                )],
            ),
            run(
                parent,
                RunStatus::Active,
                vec![task(
                    "UI Designer",
                    "New design",
                    TaskStatus::Integrated,
                    None,
                )],
            ),
        ];
        let indicator = delegation_indicator(&runs, parent, &nobody_waiting).unwrap();
        assert_eq!(indicator.label, "Band · Awaiting lead verification");
        assert_eq!(indicator.status, TaskStatus::Accepted);
        for status in [RunStatus::Paused, RunStatus::Interrupted] {
            runs[1].status = status;
            let indicator = delegation_indicator(&runs, parent, &nobody_waiting).unwrap();
            assert_eq!(indicator.activity, DelegationActivity::Paused);
            assert_eq!(indicator.status, TaskStatus::Paused);
            assert!(!indicator.live);
        }
    }

    #[test]
    fn idle_lead_with_running_experts_is_in_progress() {
        let parent = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                task(
                    "UI Designer",
                    "Settings",
                    TaskStatus::Running,
                    Some(Uuid::new_v4()),
                ),
                task("Reviewer", "Review", TaskStatus::Queued, None),
            ],
        )];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(state.activity, DelegationActivity::Working);
        assert_eq!(state.working, 1);
        assert_eq!(state.tasks.len(), 2);
        assert_eq!(state.tasks[1].activity, DelegationActivity::Idle);
    }

    #[test]
    fn waiting_for_lead_still_counts_as_work_in_progress() {
        let parent = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Waiting,
            vec![task(
                "UI Designer",
                "Settings",
                TaskStatus::WaitingForLead,
                Some(Uuid::new_v4()),
            )],
        )];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(state.activity, DelegationActivity::Working);
    }

    #[test]
    fn preparing_run_without_tasks_is_in_progress_not_idle() {
        let parent = Uuid::new_v4();
        let runs = vec![run(parent, RunStatus::Preparing, vec![])];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(state.activity, DelegationActivity::Working);
        assert!(state.has_live_run());
        assert_eq!(state.working, 0);
    }

    #[test]
    fn paused_interrupted_and_finished_runs_never_look_active() {
        let parent = Uuid::new_v4();
        let child = Uuid::new_v4();
        for status in [RunStatus::Paused, RunStatus::Interrupted] {
            let runs = vec![run(
                parent,
                status,
                vec![task(
                    "UI Designer",
                    "Settings",
                    TaskStatus::Running,
                    Some(child),
                )],
            )];
            let state = parent_delegation_state(&runs, parent, &nobody_waiting);
            assert_eq!(state.activity, DelegationActivity::Paused, "{status:?}");
            assert_eq!(state.working, 0, "{status:?}");
            assert_eq!(state.tasks[0].activity, DelegationActivity::Paused);
        }
        for status in [RunStatus::Completed, RunStatus::Cancelled] {
            let runs = vec![run(
                parent,
                status,
                vec![task(
                    "UI Designer",
                    "Settings",
                    TaskStatus::Running,
                    Some(child),
                )],
            )];
            let state = parent_delegation_state(&runs, parent, &nobody_waiting);
            assert_eq!(state, ParentDelegationState::default(), "{status:?}");
            assert!(!state.has_live_run());
        }
    }

    #[test]
    fn a_single_stopped_task_pauses_only_itself() {
        let parent = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                task(
                    "UI Designer",
                    "Settings",
                    TaskStatus::Paused,
                    Some(Uuid::new_v4()),
                ),
                task(
                    "Reviewer",
                    "Review",
                    TaskStatus::Running,
                    Some(Uuid::new_v4()),
                ),
            ],
        )];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(state.tasks[0].activity, DelegationActivity::Paused);
        assert_eq!(state.tasks[1].activity, DelegationActivity::Working);
        assert_eq!(state.activity, DelegationActivity::Working);
    }

    #[test]
    fn real_attention_outranks_work_and_comes_from_tasks_children_or_the_run() {
        let parent = Uuid::new_v4();
        let asking = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                task("UI Designer", "Settings", TaskStatus::Running, Some(asking)),
                task(
                    "Reviewer",
                    "Review",
                    TaskStatus::Running,
                    Some(Uuid::new_v4()),
                ),
            ],
        )];
        let needs_user = |id: Uuid| id == asking;
        let state = parent_delegation_state(&runs, parent, &needs_user);
        assert_eq!(state.activity, DelegationActivity::Attention);
        assert_eq!(state.tasks[0].activity, DelegationActivity::Attention);
        assert_eq!(state.tasks[1].activity, DelegationActivity::Working);
        assert_eq!(state.working, 1);

        let failed = vec![run(
            parent,
            RunStatus::Active,
            vec![task("Reviewer", "Review", TaskStatus::Failed, None)],
        )];
        assert_eq!(
            parent_delegation_state(&failed, parent, &nobody_waiting).activity,
            DelegationActivity::Attention
        );

        let blocked = vec![run(
            parent,
            RunStatus::Blocked,
            vec![task("Reviewer", "Review", TaskStatus::Queued, None)],
        )];
        assert_eq!(
            parent_delegation_state(&blocked, parent, &nobody_waiting).activity,
            DelegationActivity::Attention
        );
    }

    #[test]
    fn several_assignments_to_one_expert_are_distinguishable() {
        let parent = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                task(
                    "UI Designer",
                    "Settings",
                    TaskStatus::Running,
                    Some(Uuid::new_v4()),
                ),
                task("Reviewer", "Review", TaskStatus::Queued, None),
                task("UI Designer", "Chat cards", TaskStatus::Queued, None),
            ],
        )];
        let labels = parent_delegation_state(&runs, parent, &nobody_waiting)
            .tasks
            .into_iter()
            .map(|task| task.label)
            .collect::<Vec<_>>();
        assert_eq!(labels, ["UI Designer · 1", "Reviewer", "UI Designer · 2"]);
    }

    #[test]
    fn other_parents_are_excluded_and_finished_assignments_remain_accessible() {
        let parent = Uuid::new_v4();
        let runs = vec![
            run(
                Uuid::new_v4(),
                RunStatus::Active,
                vec![task("UI Designer", "Elsewhere", TaskStatus::Running, None)],
            ),
            run(
                parent,
                RunStatus::Active,
                vec![
                    task("UI Designer", "Done", TaskStatus::Integrated, None),
                    task(
                        "Reviewer",
                        "Review",
                        TaskStatus::Running,
                        Some(Uuid::new_v4()),
                    ),
                ],
            ),
        ];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(state.tasks.len(), 2);
        assert_eq!(state.tasks[0].goal, "Done");
        assert_eq!(state.tasks[1].goal, "Review");
    }

    fn finished_task(name: &str, goal: &str, status: TaskStatus, child: Uuid) -> DelegationTask {
        let mut task = task(name, goal, status, Some(child));
        let report = ide_core::delegation::TaskResult {
            summary: "Did the work.".into(),
            addressed: vec!["all".into()],
            checks: vec![],
            unresolved: vec![],
        };
        let attempt = task.attempts.last_mut().unwrap();
        attempt.result = Some(report);
        attempt.result_revision = Some(task.revision);
        task
    }

    #[test]
    fn overview_keeps_an_integrated_and_a_running_assignment_of_one_expert_apart() {
        let parent = Uuid::new_v4();
        let done_child = Uuid::new_v4();
        let live_child = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                finished_task(
                    "UI Designer",
                    "Settings",
                    TaskStatus::Integrated,
                    done_child,
                ),
                task(
                    "UI Designer",
                    "Chat cards",
                    TaskStatus::Running,
                    Some(live_child),
                ),
            ],
        )];
        let overview = assignment_overview(&runs, parent, &nobody_waiting);
        assert_eq!(overview.len(), 2);
        // The running one comes first and routes to its own child, never to
        // the integrated assignment that merely shares the Expert.
        assert_eq!(overview[0].goal, "Chat cards");
        assert_eq!(overview[0].child_agent_id, Some(live_child));
        assert!(!overview[0].finished);
        assert_eq!(overview[0].label, "UI Designer · 2");
        assert_eq!(overview[1].goal, "Settings");
        assert_eq!(overview[1].child_agent_id, Some(done_child));
        assert!(overview[1].finished);
        assert_eq!(overview[1].report, ReportState::Fresh);
        assert_eq!(overview[1].stage, Some("Integrated"));
        assert_eq!(overview[1].label, "UI Designer · 1");
        // The sidebar retains both task identities as one completes.
        let live = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(live.tasks.len(), 2);
        assert_eq!(live.tasks[1].child_agent_id, Some(live_child));
        assert_eq!(live.tasks[1].label, "UI Designer · 2");
    }

    #[test]
    fn overview_lists_every_simultaneously_active_assignment_with_attention_first() {
        let parent = Uuid::new_v4();
        let children: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                task("UI Designer", "A", TaskStatus::Running, Some(children[0])),
                task("Reviewer", "B", TaskStatus::NeedsUser, Some(children[1])),
                task("Backend", "C", TaskStatus::Running, Some(children[2])),
                task("Docs", "D", TaskStatus::Queued, None),
            ],
        )];
        let overview = assignment_overview(&runs, parent, &nobody_waiting);
        assert_eq!(overview.len(), 4);
        assert_eq!(overview[0].goal, "B");
        assert_eq!(overview[0].activity, DelegationActivity::Attention);
        let live_children: Vec<Option<Uuid>> = overview.iter().map(|e| e.child_agent_id).collect();
        for child in &children {
            assert!(
                live_children.contains(&Some(*child)),
                "child {child} hidden"
            );
        }
        assert_eq!(overview[3].goal, "D");
        assert_eq!(overview[3].report, ReportState::None);
    }

    #[test]
    fn a_report_written_before_a_correction_is_stale_not_fresh() {
        let child = Uuid::new_v4();
        let mut task = finished_task("UI Designer", "Settings", TaskStatus::Running, child);
        assert_eq!(report_state(&task), ReportState::Fresh);
        task.revision += 1;
        assert_eq!(report_state(&task), ReportState::Stale);
        let bare = super::tests::task("UI Designer", "Settings", TaskStatus::Running, Some(child));
        assert_eq!(report_state(&bare), ReportState::None);
    }

    #[test]
    fn completion_stages_come_only_from_durable_status() {
        assert_eq!(
            completion_stage(RunStatus::Active, TaskStatus::Running),
            None
        );
        assert_eq!(
            completion_stage(RunStatus::Active, TaskStatus::WaitingForLead),
            None
        );
        assert_eq!(
            completion_stage(RunStatus::Active, TaskStatus::ResultReady),
            Some("Bandmate finished")
        );
        assert_eq!(
            completion_stage(RunStatus::Active, TaskStatus::Integrating),
            Some("Integrating")
        );
        assert_eq!(
            completion_stage(RunStatus::Active, TaskStatus::Integrated),
            Some("Integrated")
        );
        assert_eq!(
            completion_stage(RunStatus::Completed, TaskStatus::Integrated),
            Some("Task complete")
        );
        assert_eq!(
            completion_stage(RunStatus::Completed, TaskStatus::Cancelled),
            None
        );
    }

    #[test]
    fn settled_experts_do_not_count_as_working_while_the_lead_integrates() {
        let parent = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![
                finished_task(
                    "Designer",
                    "Design",
                    TaskStatus::ResultReady,
                    Uuid::new_v4(),
                ),
                finished_task("Backend", "API", TaskStatus::Integrating, Uuid::new_v4()),
                task(
                    "Reviewer",
                    "Review",
                    TaskStatus::Running,
                    Some(Uuid::new_v4()),
                ),
            ],
        )];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert_eq!(state.working, 1);
        assert_eq!(state.activity, DelegationActivity::Working);
        let overview = assignment_overview(&runs, parent, &nobody_waiting);
        assert_eq!(overview.last().unwrap().goal, "Design");
        assert!(overview.last().unwrap().finished);
    }

    #[test]
    fn unsettled_report_is_not_announced_as_finished_and_corrections_invalidate_it() {
        let mut task = finished_task(
            "Designer",
            "Design",
            TaskStatus::CompletionRequested,
            Uuid::new_v4(),
        );
        assert_eq!(report_state(&task), ReportState::None);
        task.status = TaskStatus::ResultReady;
        assert_eq!(report_state(&task), ReportState::Fresh);
        task.revision += 1;
        task.status = TaskStatus::Running;
        assert_eq!(report_state(&task), ReportState::Stale);
    }

    #[test]
    fn all_integrated_assignments_keep_run_access_until_lead_verification() {
        let parent = Uuid::new_v4();
        let runs = vec![run(
            parent,
            RunStatus::Active,
            vec![finished_task(
                "Designer",
                "Design",
                TaskStatus::Integrated,
                Uuid::new_v4(),
            )],
        )];
        let state = parent_delegation_state(&runs, parent, &nobody_waiting);
        assert!(state.has_live_run());
        assert_eq!(state.working, 0);
        assert_eq!(state.tasks.len(), 1);
        assert_eq!(
            assignment_overview(&runs, parent, &nobody_waiting)[0].stage,
            Some("Integrated")
        );
    }

    #[test]
    fn bounded_text_cuts_long_reports_on_a_word_and_leaves_short_ones_alone() {
        assert_eq!(bounded_text("  short  ", 20), ("short".to_string(), false));
        let long = "alpha beta gamma delta epsilon zeta eta theta";
        let (head, cut) = bounded_text(long, 20);
        assert!(cut);
        assert_eq!(head, "alpha beta gamma…");
        let (empty, cut) = bounded_text("", 10);
        assert_eq!(empty, "");
        assert!(!cut);
    }
}
