//! Pure presentation rules for Ship-linked GitHub Actions. Rendering lives in
//! `agent_chat_ship_actions`; the companion reuses the same tone and identity
//! rules so both surfaces describe one watch the same way.
use ide_core::ship_actions::{
    ActionsOutcome, GithubActionRun, ShipActionsOrigin, ShipActionsWatch, TrackingPhase,
};
use uuid::Uuid;

/// Workflow rows shown before "Show all N workflows".
pub(crate) const COLLAPSED_RUN_LIMIT: usize = 3;

/// Color family for a watch or run. Text and icon always accompany the color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ActionsTone {
    /// Queued, running, approval, discovery: amber.
    Pending,
    /// Every run settled successfully: sage.
    Success,
    /// A run failed: red.
    Failure,
    /// Skipped, cancelled, stopped, paused, closed, offline: muted.
    Muted,
}

/// How a Ship card relates to the watch for its PR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ShipActionsPlacement {
    /// No watch; a GitHub PR Ship can still be registered explicitly.
    Untracked,
    /// This Ship owns the conversation's Actions activity for the PR.
    Owner,
    /// A later Ship in this conversation tracks the same PR.
    Superseded { latest_ship_id: String },
}

/// Ship card line shown before workflow runs exist (or in place of a card).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ShipActionsInline {
    /// No line; the full Actions card is shown below the Ship card.
    None,
    /// Waiting for merge or discovering runs.
    Discovering,
    /// No run was detected for the merge commit.
    NoRuns,
    /// The PR closed without merging.
    Closed,
    /// Tracking stopped or paused before any run was seen; offer Resume.
    Resumable,
}

pub(crate) fn watch_tone(watch: &ShipActionsWatch) -> ActionsTone {
    match watch.phase {
        TrackingPhase::Stopped
        | TrackingPhase::Paused
        | TrackingPhase::Closed
        | TrackingPhase::NoRuns => return ActionsTone::Muted,
        _ => {}
    }
    if watch.refresh_error.is_some() {
        return ActionsTone::Muted;
    }
    if watch.runs.is_empty() {
        return ActionsTone::Pending;
    }
    let settled = watch.phase == TrackingPhase::Settled;
    match watch.outcome() {
        ActionsOutcome::Failed => ActionsTone::Failure,
        ActionsOutcome::Success | ActionsOutcome::SuccessWithSkipped if settled => {
            ActionsTone::Success
        }
        ActionsOutcome::Skipped | ActionsOutcome::Cancelled if settled => ActionsTone::Muted,
        _ => ActionsTone::Pending,
    }
}

pub(crate) fn run_tone(run: &GithubActionRun) -> ActionsTone {
    match run.outcome() {
        ActionsOutcome::Failed => ActionsTone::Failure,
        ActionsOutcome::Success | ActionsOutcome::SuccessWithSkipped => ActionsTone::Success,
        ActionsOutcome::Skipped | ActionsOutcome::Cancelled => ActionsTone::Muted,
        ActionsOutcome::Queued | ActionsOutcome::Running | ActionsOutcome::Approval => {
            ActionsTone::Pending
        }
        ActionsOutcome::Unknown if run.status == "completed" => ActionsTone::Muted,
        ActionsOutcome::Unknown => ActionsTone::Pending,
    }
}

/// Run status in plain words. Unknown conclusions stay unknown, not success.
pub(crate) fn run_status_label(run: &GithubActionRun) -> String {
    match run.outcome() {
        ActionsOutcome::Queued => "Queued".into(),
        ActionsOutcome::Running => "Running".into(),
        ActionsOutcome::Approval => "Waiting for approval".into(),
        ActionsOutcome::Success => "Succeeded".into(),
        ActionsOutcome::SuccessWithSkipped => "Succeeded".into(),
        ActionsOutcome::Skipped => "Skipped".into(),
        ActionsOutcome::Cancelled => "Cancelled".into(),
        ActionsOutcome::Failed => match run.conclusion.as_deref() {
            Some("timed_out") => "Timed out".into(),
            Some("startup_failure") => "Failed to start".into(),
            Some("action_required") => "Action required".into(),
            _ => "Failed".into(),
        },
        ActionsOutcome::Unknown => match run.conclusion.as_deref() {
            Some(other) => format!("Finished · {}", other.replace('_', " ")),
            None => "Status unknown".into(),
        },
    }
}

/// Names the exact run a row's link opens, including its attempt.
pub(crate) fn run_open_label(run: &GithubActionRun) -> String {
    if run.attempt > 1 {
        format!("Open “{}” (attempt {}) on GitHub", run.name, run.attempt)
    } else {
        format!("Open “{}” on GitHub", run.name)
    }
}

/// Latest Ship of `agent` that refers to this watch.
pub(crate) fn latest_origin_for_agent(
    watch: &ShipActionsWatch,
    agent_id: Uuid,
) -> Option<&ShipActionsOrigin> {
    watch
        .origins
        .iter()
        .filter(|origin| origin.agent_id == agent_id)
        .max_by_key(|origin| origin.created_at)
}

pub(crate) fn placement(
    watch: Option<&ShipActionsWatch>,
    agent_id: Uuid,
    ship_id: &str,
) -> ShipActionsPlacement {
    let Some(watch) = watch else {
        return ShipActionsPlacement::Untracked;
    };
    match latest_origin_for_agent(watch, agent_id) {
        Some(latest) if latest.ship_id != ship_id => ShipActionsPlacement::Superseded {
            latest_ship_id: latest.ship_id.clone(),
        },
        _ => ShipActionsPlacement::Owner,
    }
}

pub(crate) fn inline_state(watch: &ShipActionsWatch) -> ShipActionsInline {
    match watch.phase {
        TrackingPhase::NoRuns => ShipActionsInline::NoRuns,
        TrackingPhase::Closed => ShipActionsInline::Closed,
        _ if !watch.runs.is_empty() => ShipActionsInline::None,
        TrackingPhase::Stopped | TrackingPhase::Paused => ShipActionsInline::Resumable,
        _ => ShipActionsInline::Discovering,
    }
}

/// Stopped and paused watches offer Resume; everything else offers Refresh.
pub(crate) fn offers_resume(watch: &ShipActionsWatch) -> bool {
    matches!(watch.phase, TrackingPhase::Stopped | TrackingPhase::Paused)
}

/// Only watches that are still polled can be stopped.
pub(crate) fn offers_stop(watch: &ShipActionsWatch) -> bool {
    watch.automatically_polled()
}

pub(crate) fn visible_runs(runs: &[GithubActionRun], expanded: bool) -> &[GithubActionRun] {
    if expanded {
        runs
    } else {
        &runs[..runs.len().min(COLLAPSED_RUN_LIMIT)]
    }
}

/// GitHub is being polled after the merge and the last request succeeded.
/// Cached runs only animate or count up while this holds.
pub(crate) fn watch_live(watch: &ShipActionsWatch) -> bool {
    matches!(
        watch.phase,
        TrackingPhase::Discovering | TrackingPhase::Tracking
    ) && watch.refresh_error.is_none()
}

/// A run spins only when it is running and still being watched. A stopped,
/// paused or offline watch shows its last-known status without motion.
pub(crate) fn run_spins(run: &GithubActionRun, live: bool) -> bool {
    live && run.outcome() == ActionsOutcome::Running
}

/// Seconds a run has taken so far. A finished run stops at its last update.
pub(crate) fn run_elapsed(run: &GithubActionRun, now: u64) -> Option<u64> {
    let start = run.started_at?;
    let end = if run.terminal() { run.updated_at } else { now };
    Some(end.saturating_sub(start))
}

/// Elapsed time as shown: while the watch is not live, an unfinished run is
/// frozen at the last successful check (or its last update) rather than
/// implying fresh progress.
pub(crate) fn run_elapsed_shown(
    run: &GithubActionRun,
    now: u64,
    watch: &ShipActionsWatch,
) -> Option<u64> {
    let end = if watch_live(watch) {
        now
    } else {
        watch
            .last_checked_at
            .unwrap_or(run.updated_at)
            .max(run.updated_at)
    };
    run_elapsed(run, end)
}

/// Seconds since the earliest run started. Open-ended only while a run is live.
pub(crate) fn watch_elapsed(watch: &ShipActionsWatch, now: u64) -> Option<u64> {
    let start = watch
        .runs
        .iter()
        .map(|run| run.started_at.unwrap_or(run.created_at))
        .min()?;
    let all_terminal = watch.runs.iter().all(GithubActionRun::terminal);
    let end = if all_terminal || !watch_live(watch) {
        watch.runs.iter().map(|run| run.updated_at).max()?
    } else {
        now
    };
    Some(end.saturating_sub(start))
}

pub(crate) fn short_sha(sha: &str) -> &str {
    sha.get(..7).unwrap_or(sha)
}

/// The failed run when there is one; otherwise the merge commit, whose GitHub
/// page lists every check for it. Falls back to the PR before a merge exists.
pub(crate) fn github_actions_url(watch: &ShipActionsWatch) -> String {
    if let Some(run) = watch
        .runs
        .iter()
        .find(|run| run.outcome() == ActionsOutcome::Failed)
    {
        return run.html_url.clone();
    }
    match watch.merge_sha.as_deref() {
        Some(sha) => format!("https://{}/{}/commit/{sha}", watch.host, watch.repository),
        None => watch.pr_url.clone(),
    }
}

/// The watch to poll right after an in-app merge: only this Ship's watch, and
/// only when GitHub reports merging the same PR number it tracks.
pub(crate) fn watch_to_wake(
    watch: Option<(Uuid, u64)>,
    merged_pr: Option<u64>,
    tracking_active: bool,
) -> Option<Uuid> {
    let (watch_id, watched_pr) = watch?;
    (tracking_active && merged_pr? == watched_pr).then_some(watch_id)
}

/// Everything that can change an Actions card's height for one watch. Status
/// text changes that keep the height are deliberately excluded.
pub(crate) fn layout_signature(
    watch: &ShipActionsWatch,
    expanded: bool,
    global_error: bool,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    watch.id.hash(&mut hasher);
    inline_state(watch).hash(&mut hasher);
    watch_tone(watch).hash(&mut hasher);
    let shown = visible_runs(&watch.runs, expanded);
    shown.len().hash(&mut hasher);
    shown.iter().any(|run| run.attempt > 1).hash(&mut hasher);
    (watch.runs.len() > COLLAPSED_RUN_LIMIT).hash(&mut hasher);
    (watch.outcome() == ActionsOutcome::Approval).hash(&mut hasher);
    watch
        .refresh_error
        .as_deref()
        .map(bounded_error)
        .hash(&mut hasher);
    watch.merge_sha.is_some().hash(&mut hasher);
    for origin in &watch.origins {
        (origin.agent_id, &origin.ship_id).hash(&mut hasher);
    }
    global_error.hash(&mut hasher);
    hasher.finish()
}

/// Keeps command output from flooding a card; the first line is the summary.
pub(crate) fn bounded_error(error: &str) -> String {
    const LIMIT: usize = 180;
    let line = error.lines().next().unwrap_or_default().trim();
    if line.chars().count() <= LIMIT {
        return line.to_string();
    }
    let cut: String = line.chars().take(LIMIT).collect();
    format!("{}…", cut.trim_end())
}

/// Draft for the composer. The user reviews and sends it; nothing is sent here.
pub(crate) fn investigation_draft(watch: &ShipActionsWatch) -> String {
    let failed: Vec<_> = watch
        .runs
        .iter()
        .filter(|run| run.outcome() == ActionsOutcome::Failed)
        .collect();
    let mut lines = vec![format!(
        "GitHub Actions failed after PR #{} was merged in {}.",
        watch.pr_number, watch.repository
    )];
    lines.push(String::new());
    for run in &failed {
        let attempt = if run.attempt > 1 {
            format!(" (attempt {})", run.attempt)
        } else {
            String::new()
        };
        lines.push(format!(
            "- {}{}: {} — {}",
            run.name,
            attempt,
            run_status_label(run),
            run.html_url
        ));
    }
    lines.push(format!("Pull request: {}", watch.pr_url));
    if let Some(sha) = watch.merge_sha.as_deref() {
        lines.push(format!("Merge commit: {sha}"));
    }
    if !watch.base_branch.is_empty() {
        lines.push(format!("Base branch: {}", watch.base_branch));
    }
    lines.push(String::new());
    lines.push(
        "Investigate why it failed and suggest a fix. Don't push or merge anything until I confirm."
            .into(),
    );
    lines.join("\n")
}

/// Row ordering in the companion: lower is shown first.
pub(crate) mod rank {
    pub(crate) const ATTENTION: u8 = 0;
    pub(crate) const ACTIVE_AGENT: u8 = 1;
    pub(crate) const RUNNING_ACTIONS: u8 = 2;
    pub(crate) const COMPLETION: u8 = 3;
}

/// What the companion shows for a watch, or `None` when it has nothing to say.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CompanionActionsRow {
    pub watch_id: Uuid,
    pub tone: ActionsTone,
    pub rank: u8,
    /// True only while GitHub is still being polled and no error is shown.
    pub spinning: bool,
    pub attention: bool,
    pub status: String,
}

pub(crate) fn companion_row(watch: &ShipActionsWatch) -> Option<CompanionActionsRow> {
    watch.latest_origin()?;
    let tone = watch_tone(watch);
    let status = watch.status_label().to_string();
    if watch.unread() {
        let attention = matches!(
            watch.outcome(),
            ActionsOutcome::Failed | ActionsOutcome::Approval
        );
        return Some(CompanionActionsRow {
            watch_id: watch.id,
            tone,
            rank: if attention {
                rank::ATTENTION
            } else {
                rank::COMPLETION
            },
            spinning: false,
            attention,
            status,
        });
    }
    // Waiting for a merge is PR state, not Actions activity.
    let active = matches!(
        watch.phase,
        TrackingPhase::Discovering | TrackingPhase::Tracking
    );
    active.then(|| CompanionActionsRow {
        watch_id: watch.id,
        tone,
        rank: rank::RUNNING_ACTIONS,
        spinning: watch.refresh_error.is_none(),
        attention: false,
        status,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::ProjectId;

    fn watch() -> ShipActionsWatch {
        ShipActionsWatch {
            id: Uuid::new_v4(),
            revision: 1,
            project_id: ProjectId(Uuid::new_v4()),
            project_name: "Project".into(),
            host: "github.com".into(),
            repository: "owner/repo".into(),
            repository_path: "/tmp/repo".into(),
            pr_number: 7,
            pr_url: "https://github.com/owner/repo/pull/7".into(),
            base_branch: "main".into(),
            merge_sha: Some("0123456789abcdef".into()),
            merged_at: Some(100),
            monitoring_started_at: Some(100),
            phase: TrackingPhase::Tracking,
            origins: vec![],
            runs: vec![],
            discovery_seconds: 0,
            last_checked_at: Some(100),
            refresh_error: None,
            error_count: 0,
            next_poll_at: 100,
            stable_terminal_scans: 0,
            delivered_revision: None,
            acknowledged_revision: None,
            created_at: 90,
            updated_at: 100,
        }
    }

    fn run(id: u64, status: &str, conclusion: Option<&str>) -> GithubActionRun {
        GithubActionRun {
            id,
            workflow_id: id,
            name: format!("Workflow {id}"),
            event: "push".into(),
            head_sha: "0123456789abcdef".into(),
            head_branch: "main".into(),
            attempt: 1,
            status: status.into(),
            conclusion: conclusion.map(str::to_owned),
            created_at: 100,
            updated_at: 160,
            started_at: Some(110),
            html_url: format!("https://github.com/owner/repo/actions/runs/{id}"),
        }
    }

    fn origin(agent: Uuid, ship: &str, created_at: u64) -> ShipActionsOrigin {
        ShipActionsOrigin {
            agent_id: agent,
            ship_id: ship.into(),
            agent_title: "Agent".into(),
            created_at,
        }
    }

    #[test]
    fn success_is_sage_only_once_settled() {
        let mut w = watch();
        w.runs = vec![run(1, "completed", Some("success"))];
        assert_eq!(watch_tone(&w), ActionsTone::Pending);
        w.phase = TrackingPhase::Settled;
        assert_eq!(watch_tone(&w), ActionsTone::Success);
    }

    #[test]
    fn skipped_cancelled_stopped_and_offline_are_never_success() {
        let mut w = watch();
        w.phase = TrackingPhase::Settled;
        w.runs = vec![run(1, "completed", Some("skipped"))];
        assert_eq!(watch_tone(&w), ActionsTone::Muted);
        w.runs = vec![run(1, "completed", Some("cancelled"))];
        assert_eq!(watch_tone(&w), ActionsTone::Muted);
        w.runs = vec![run(1, "completed", Some("success"))];
        for phase in [TrackingPhase::Stopped, TrackingPhase::Paused] {
            w.phase = phase;
            assert_eq!(watch_tone(&w), ActionsTone::Muted);
        }
        w.phase = TrackingPhase::Tracking;
        w.refresh_error = Some("offline".into());
        assert_eq!(watch_tone(&w), ActionsTone::Muted);
    }

    #[test]
    fn failure_and_approval_tones() {
        let mut w = watch();
        w.runs = vec![
            run(1, "completed", Some("failure")),
            run(2, "in_progress", None),
        ];
        assert_eq!(watch_tone(&w), ActionsTone::Failure);
        w.runs = vec![run(1, "waiting", None)];
        assert_eq!(watch_tone(&w), ActionsTone::Pending);
        assert_eq!(run_status_label(&w.runs[0]), "Waiting for approval");
    }

    #[test]
    fn unknown_conclusions_are_reported_as_is() {
        let r = run(1, "completed", Some("brand_new"));
        assert_eq!(run_tone(&r), ActionsTone::Muted);
        assert_eq!(run_status_label(&r), "Finished · brand new");
        assert_eq!(
            run_status_label(&run(2, "completed", Some("timed_out"))),
            "Timed out"
        );
    }

    #[test]
    fn only_the_latest_ship_in_a_conversation_owns_the_card() {
        let agent = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut w = watch();
        w.origins = vec![
            origin(agent, "first", 10),
            origin(agent, "second", 20),
            origin(other, "elsewhere", 30),
        ];
        assert_eq!(
            placement(Some(&w), agent, "second"),
            ShipActionsPlacement::Owner
        );
        assert_eq!(
            placement(Some(&w), agent, "first"),
            ShipActionsPlacement::Superseded {
                latest_ship_id: "second".into()
            }
        );
        // Another conversation's later Ship does not hide this one's card.
        assert_eq!(
            placement(Some(&w), other, "elsewhere"),
            ShipActionsPlacement::Owner
        );
        assert_eq!(
            placement(None, agent, "first"),
            ShipActionsPlacement::Untracked
        );
    }

    #[test]
    fn inline_lines_cover_states_without_runs() {
        let mut w = watch();
        w.phase = TrackingPhase::AwaitingMerge;
        assert_eq!(inline_state(&w), ShipActionsInline::Discovering);
        w.phase = TrackingPhase::NoRuns;
        assert_eq!(inline_state(&w), ShipActionsInline::NoRuns);
        w.phase = TrackingPhase::Stopped;
        assert_eq!(inline_state(&w), ShipActionsInline::Resumable);
        assert!(offers_resume(&w) && !offers_stop(&w));
        w.runs = vec![run(1, "queued", None)];
        assert_eq!(inline_state(&w), ShipActionsInline::None);
    }

    #[test]
    fn collapsed_runs_show_three() {
        let runs: Vec<_> = (1..=5).map(|id| run(id, "queued", None)).collect();
        assert_eq!(visible_runs(&runs, false).len(), 3);
        assert_eq!(visible_runs(&runs, true).len(), 5);
        assert_eq!(visible_runs(&runs[..2], false).len(), 2);
    }

    #[test]
    fn elapsed_stops_when_runs_finish() {
        let mut w = watch();
        w.runs = vec![run(1, "in_progress", None)];
        assert_eq!(watch_elapsed(&w, 200), Some(90));
        w.runs = vec![run(1, "completed", Some("success"))];
        assert_eq!(watch_elapsed(&w, 999), Some(50));
        assert_eq!(run_elapsed(&w.runs[0], 999), Some(50));
        assert_eq!(watch_elapsed(&watch(), 200), None);
    }

    #[test]
    fn investigation_draft_names_failed_runs_pr_and_merge() {
        let mut w = watch();
        w.runs = vec![
            GithubActionRun {
                attempt: 2,
                ..run(1, "completed", Some("failure"))
            },
            run(2, "completed", Some("success")),
        ];
        let draft = investigation_draft(&w);
        assert!(draft.contains("PR #7"));
        assert!(draft.contains("Workflow 1 (attempt 2): Failed"));
        assert!(draft.contains("actions/runs/1"));
        assert!(!draft.contains("Workflow 2"));
        assert!(draft.contains("Merge commit: 0123456789abcdef"));
        assert!(draft.contains(&w.pr_url));
    }

    #[test]
    fn github_link_prefers_failure_then_merge_commit() {
        let mut w = watch();
        assert_eq!(
            github_actions_url(&w),
            "https://github.com/owner/repo/commit/0123456789abcdef"
        );
        w.runs = vec![
            run(1, "completed", Some("success")),
            run(2, "completed", Some("failure")),
        ];
        assert_eq!(
            github_actions_url(&w),
            "https://github.com/owner/repo/actions/runs/2"
        );
        w.runs.clear();
        w.merge_sha = None;
        assert_eq!(github_actions_url(&w), w.pr_url);
    }

    #[test]
    fn cached_running_runs_only_spin_and_count_while_watched() {
        let mut w = watch();
        let running = run(1, "in_progress", None);
        w.runs = vec![running.clone()];
        assert!(watch_live(&w) && run_spins(&running, true));
        assert_eq!(run_elapsed_shown(&running, 300, &w), Some(190));
        assert!(!run_spins(&run(2, "queued", None), true));

        w.last_checked_at = Some(200);
        for phase in [TrackingPhase::Stopped, TrackingPhase::Paused] {
            w.phase = phase;
            assert!(!watch_live(&w));
            assert!(!run_spins(&running, watch_live(&w)));
            // Frozen at the last check; status text stays last-known.
            assert_eq!(run_elapsed_shown(&running, 300, &w), Some(90));
            assert_eq!(watch_elapsed(&w, 300), Some(50));
            assert_eq!(run_status_label(&running), "Running");
        }
        w.phase = TrackingPhase::Tracking;
        w.refresh_error = Some("offline".into());
        w.last_checked_at = None;
        assert!(!watch_live(&w));
        assert_eq!(run_elapsed_shown(&running, 300, &w), Some(50));
        w.phase = TrackingPhase::AwaitingMerge;
        w.refresh_error = None;
        assert!(!watch_live(&w));
    }

    #[test]
    fn each_run_link_names_its_workflow_and_attempt() {
        let first = run(1, "completed", Some("failure"));
        assert_eq!(run_open_label(&first), "Open “Workflow 1” on GitHub");
        let retry = GithubActionRun {
            attempt: 3,
            ..first
        };
        assert_eq!(
            run_open_label(&retry),
            "Open “Workflow 1” (attempt 3) on GitHub"
        );
    }

    #[test]
    fn merge_wakes_only_the_matching_watch() {
        let id = Uuid::new_v4();
        assert_eq!(watch_to_wake(Some((id, 7)), Some(7), true), Some(id));
        assert_eq!(watch_to_wake(Some((id, 7)), Some(8), true), None);
        assert_eq!(watch_to_wake(Some((id, 7)), None, true), None);
        assert_eq!(watch_to_wake(None, Some(7), true), None);
        // Stop tracking can be clicked while the merge request is in flight.
        assert_eq!(watch_to_wake(Some((id, 7)), Some(7), false), None);
    }

    #[test]
    fn errors_are_bounded_to_one_line() {
        assert_eq!(bounded_error("first\nsecond"), "first");
        let long = "x".repeat(400);
        assert_eq!(bounded_error(&long).chars().count(), 181);
    }

    #[test]
    fn companion_rows_rank_attention_running_and_unread_completion() {
        let agent = Uuid::new_v4();
        let mut w = watch();
        w.origins = vec![origin(agent, "ship", 10)];
        w.runs = vec![run(1, "in_progress", None)];
        let running = companion_row(&w).unwrap();
        assert_eq!(
            (running.rank, running.spinning, running.tone),
            (rank::RUNNING_ACTIONS, true, ActionsTone::Pending)
        );
        assert_eq!(running.watch_id, w.id);

        w.refresh_error = Some("offline".into());
        assert!(!companion_row(&w).unwrap().spinning);
        w.refresh_error = None;

        w.runs = vec![run(1, "completed", Some("failure"))];
        let failed = companion_row(&w).unwrap();
        assert!(failed.attention);
        assert_eq!(
            (failed.rank, failed.tone),
            (rank::ATTENTION, ActionsTone::Failure)
        );

        w.runs = vec![run(1, "completed", Some("success"))];
        w.phase = TrackingPhase::Settled;
        let done = companion_row(&w).unwrap();
        assert_eq!(
            (done.rank, done.tone, done.spinning),
            (rank::COMPLETION, ActionsTone::Success, false)
        );

        // Read completions leave; stopped and waiting-for-merge never spin.
        w.acknowledged_revision = w.notice_revision();
        assert!(companion_row(&w).is_none());
        for phase in [
            TrackingPhase::Stopped,
            TrackingPhase::Paused,
            TrackingPhase::AwaitingMerge,
        ] {
            w.phase = phase;
            assert!(companion_row(&w).is_none());
        }
        w.origins.clear();
        w.phase = TrackingPhase::Tracking;
        assert!(companion_row(&w).is_none());
    }
}
