//! Durable external automation. These types deliberately have no agent execution state.
use crate::ProjectId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub const DISCOVERY_SECONDS: u64 = 120;
pub const MAX_TRACKING_SECONDS: u64 = 24 * 60 * 60;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipActionsOrigin {
    pub agent_id: Uuid,
    pub ship_id: String,
    pub agent_title: String,
    pub created_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrackingPhase {
    AwaitingMerge,
    Discovering,
    Tracking,
    Settled,
    NoRuns,
    Closed,
    Stopped,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionsOutcome {
    Queued,
    Running,
    Approval,
    Success,
    SuccessWithSkipped,
    Skipped,
    Failed,
    Cancelled,
    Unknown,
}

impl ActionsOutcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "Waiting to start",
            Self::Running => "Actions running",
            Self::Approval => "Approval needed on GitHub",
            Self::Success => "Actions complete",
            Self::SuccessWithSkipped => "Actions complete · Some skipped",
            Self::Skipped => "Actions skipped",
            Self::Failed => "Action failed",
            Self::Cancelled => "Actions cancelled",
            Self::Unknown => "Checking Actions status…",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GithubActionRun {
    pub id: u64,
    pub workflow_id: u64,
    pub name: String,
    pub event: String,
    pub head_sha: String,
    pub head_branch: String,
    pub attempt: u64,
    pub status: String,
    pub conclusion: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub started_at: Option<u64>,
    pub html_url: String,
}

impl GithubActionRun {
    pub fn outcome(&self) -> ActionsOutcome {
        match self.status.as_str() {
            "waiting" => ActionsOutcome::Approval,
            "queued" | "requested" | "pending" => ActionsOutcome::Queued,
            "in_progress" => ActionsOutcome::Running,
            "completed" => match self.conclusion.as_deref() {
                Some("success" | "neutral") => ActionsOutcome::Success,
                Some("skipped") => ActionsOutcome::Skipped,
                Some("failure" | "timed_out" | "startup_failure" | "action_required" | "stale") => {
                    ActionsOutcome::Failed
                }
                Some("cancelled") => ActionsOutcome::Cancelled,
                _ => ActionsOutcome::Unknown,
            },
            _ => ActionsOutcome::Unknown,
        }
    }
    pub fn terminal(&self) -> bool {
        self.status == "completed" && self.outcome() != ActionsOutcome::Unknown
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShipActionsWatch {
    pub id: Uuid,
    pub revision: u64,
    pub project_id: ProjectId,
    pub project_name: String,
    pub host: String,
    pub repository: String,
    pub repository_path: PathBuf,
    pub pr_number: u64,
    pub pr_url: String,
    pub base_branch: String,
    pub merge_sha: Option<String>,
    pub merged_at: Option<u64>,
    pub monitoring_started_at: Option<u64>,
    pub phase: TrackingPhase,
    pub origins: Vec<ShipActionsOrigin>,
    pub runs: Vec<GithubActionRun>,
    pub discovery_seconds: u64,
    pub last_checked_at: Option<u64>,
    pub refresh_error: Option<String>,
    pub error_count: u32,
    pub next_poll_at: u64,
    pub stable_terminal_scans: u8,
    pub delivered_revision: Option<String>,
    pub acknowledged_revision: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ShipActionsWatch {
    pub fn latest_origin(&self) -> Option<&ShipActionsOrigin> {
        self.origins.iter().max_by_key(|o| o.created_at)
    }
    pub fn automatically_polled(&self) -> bool {
        matches!(
            self.phase,
            TrackingPhase::AwaitingMerge | TrackingPhase::Discovering | TrackingPhase::Tracking
        )
    }
    pub fn outcome(&self) -> ActionsOutcome {
        let outcomes: Vec<_> = self.runs.iter().map(GithubActionRun::outcome).collect();
        for state in [
            ActionsOutcome::Failed,
            ActionsOutcome::Approval,
            ActionsOutcome::Running,
            ActionsOutcome::Queued,
            ActionsOutcome::Unknown,
        ] {
            if outcomes.contains(&state) {
                return state;
            }
        }
        if outcomes.is_empty() {
            return ActionsOutcome::Unknown;
        }
        if outcomes.contains(&ActionsOutcome::Cancelled) {
            return ActionsOutcome::Cancelled;
        }
        if outcomes.iter().all(|s| *s == ActionsOutcome::Skipped) {
            return ActionsOutcome::Skipped;
        }
        if outcomes.contains(&ActionsOutcome::Skipped) {
            return ActionsOutcome::SuccessWithSkipped;
        }
        ActionsOutcome::Success
    }
    pub fn status_label(&self) -> &'static str {
        match self.phase {
            TrackingPhase::Stopped => "Tracking stopped",
            TrackingPhase::Paused => "Tracking paused · Check on GitHub",
            _ if self.refresh_error.is_some() => "Unable to refresh",
            TrackingPhase::AwaitingMerge => "Waiting for PR merge",
            TrackingPhase::NoRuns => "No GitHub Actions detected",
            TrackingPhase::Closed => "PR closed without merging",
            _ if self.runs.is_empty() => "Checking for GitHub Actions…",
            _ if matches!(
                self.outcome(),
                ActionsOutcome::Success
                    | ActionsOutcome::SuccessWithSkipped
                    | ActionsOutcome::Skipped
            ) && self.phase != TrackingPhase::Settled =>
            {
                "Checking for more Actions…"
            }
            _ => self.outcome().label(),
        }
    }
    /// Only exact, post-merge push/chain runs belong to this merge.
    pub fn matches_run(&self, run: &GithubActionRun) -> bool {
        self.merge_sha.as_deref() == Some(run.head_sha.as_str())
            && run.head_branch == self.base_branch
            && matches!(run.event.as_str(), "push" | "workflow_run")
            && self
                .merged_at
                .is_some_and(|t| run.created_at >= t.saturating_sub(5))
    }
    pub fn observe_runs(&mut self, incoming: Vec<GithubActionRun>, now: u64) {
        let before = self.runs.clone();
        for run in incoming
            .into_iter()
            .filter(|r| self.matches_run(r))
            .collect::<Vec<_>>()
        {
            if let Some(old) = self.runs.iter_mut().find(|r| r.id == run.id) {
                if (run.attempt, run.updated_at) >= (old.attempt, old.updated_at) {
                    *old = run;
                }
            } else {
                self.runs.push(run);
            }
        }
        self.runs.sort_by_key(|r| r.id);
        // Failed/offline requests exclude the reconnect interval; sleep and
        // application downtime never consume more than one polling interval.
        let online_interval = if self.refresh_error.is_some() {
            0
        } else {
            self.last_checked_at
                .map_or(0, |last| now.saturating_sub(last).min(15))
        };
        self.discovery_seconds = self.discovery_seconds.saturating_add(online_interval);
        let spaced = self
            .last_checked_at
            .is_none_or(|last| now.saturating_sub(last) >= 15);
        self.last_checked_at = Some(now);
        self.refresh_error = None;
        self.error_count = 0;
        let all_terminal = !self.runs.is_empty() && self.runs.iter().all(GithubActionRun::terminal);
        if !all_terminal || before != self.runs {
            self.stable_terminal_scans = 0;
        } else if spaced {
            self.stable_terminal_scans = self.stable_terminal_scans.saturating_add(1);
        }
        self.phase = if self.discovery_seconds >= DISCOVERY_SECONDS {
            if self.runs.is_empty() {
                TrackingPhase::NoRuns
            } else if all_terminal && self.stable_terminal_scans >= 2 {
                TrackingPhase::Settled
            } else {
                TrackingPhase::Tracking
            }
        } else {
            TrackingPhase::Discovering
        };
        self.updated_at = now;
        self.next_poll_at = now
            + if self.discovery_seconds < DISCOVERY_SECONDS {
                10
            } else {
                15
            };
    }
    pub fn notice_revision(&self) -> Option<String> {
        if self.runs.is_empty()
            || matches!(self.phase, TrackingPhase::Stopped | TrackingPhase::Paused)
        {
            return None;
        }
        let outcome = self.outcome();
        if !matches!(outcome, ActionsOutcome::Failed | ActionsOutcome::Approval)
            && self.phase != TrackingPhase::Settled
        {
            return None;
        }
        let relevant = self.runs.iter().filter(|r| {
            !matches!(outcome, ActionsOutcome::Failed | ActionsOutcome::Approval)
                || r.outcome() == outcome
        });
        Some(format!(
            "{:?}:{}",
            outcome,
            relevant
                .map(|r| format!("{}-{}", r.id, r.attempt))
                .collect::<Vec<_>>()
                .join(",")
        ))
    }
    pub fn unread(&self) -> bool {
        self.notice_revision()
            .is_some_and(|r| self.acknowledged_revision.as_ref() != Some(&r))
    }
    pub fn resume(&mut self, now: u64) {
        self.phase = if self.merge_sha.is_some() {
            TrackingPhase::Discovering
        } else {
            TrackingPhase::AwaitingMerge
        };
        self.stable_terminal_scans = 0;
        self.next_poll_at = now;
        self.monitoring_started_at = Some(now);
        self.last_checked_at = None;
        self.updated_at = now;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn watch() -> ShipActionsWatch {
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
            merge_sha: Some("merged".into()),
            merged_at: Some(100),
            monitoring_started_at: Some(100),
            phase: TrackingPhase::Discovering,
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
    pub(crate) fn run(id: u64, status: &str, conclusion: Option<&str>) -> GithubActionRun {
        GithubActionRun {
            id,
            workflow_id: 1,
            name: "Deploy".into(),
            event: "push".into(),
            head_sha: "merged".into(),
            head_branch: "main".into(),
            attempt: 1,
            status: status.into(),
            conclusion: conclusion.map(str::to_owned),
            created_at: 100,
            updated_at: 110,
            started_at: Some(105),
            html_url: "https://github.com/owner/repo/actions/runs/1".into(),
        }
    }
    #[test]
    fn correlation_rejects_unrelated_and_premerge_runs() {
        let w = watch();
        let good = run(1, "queued", None);
        assert!(w.matches_run(&good));
        for bad in [
            GithubActionRun {
                head_sha: "source".into(),
                ..good.clone()
            },
            GithubActionRun {
                event: "pull_request".into(),
                ..good.clone()
            },
            GithubActionRun {
                created_at: 50,
                ..good.clone()
            },
            GithubActionRun {
                head_branch: "other".into(),
                ..good
            },
        ] {
            assert!(!w.matches_run(&bad));
        }
    }
    #[test]
    fn success_waits_for_discovery_and_every_run() {
        let mut w = watch();
        w.observe_runs(
            vec![
                run(1, "completed", Some("success")),
                run(2, "in_progress", None),
            ],
            115,
        );
        assert_eq!(w.outcome(), ActionsOutcome::Running);
        assert!(w.notice_revision().is_none());
        w.discovery_seconds = 120;
        w.observe_runs(vec![run(2, "completed", Some("success"))], 130);
        assert_ne!(w.phase, TrackingPhase::Settled);
        w.observe_runs(vec![], 145);
        w.observe_runs(vec![], 160);
        assert_eq!(w.phase, TrackingPhase::Settled);
        assert!(w.unread());
        w.acknowledged_revision = w.notice_revision();
        assert!(!w.unread());
    }
    #[test]
    fn failures_approval_and_reruns_have_separate_revisions() {
        let mut w = watch();
        w.observe_runs(
            vec![
                run(1, "completed", Some("failure")),
                run(2, "in_progress", None),
            ],
            115,
        );
        let failed = w.notice_revision().unwrap();
        assert_eq!(w.outcome(), ActionsOutcome::Failed);
        w.acknowledged_revision = Some(failed.clone());
        w.observe_runs(
            vec![GithubActionRun {
                attempt: 2,
                ..run(1, "waiting", None)
            }],
            130,
        );
        assert_eq!(w.outcome(), ActionsOutcome::Approval);
        assert_ne!(w.notice_revision(), Some(failed));
        assert!(w.unread());
        w.observe_runs(vec![run(1, "completed", Some("failure"))], 145);
        assert_eq!(w.runs[0].attempt, 2);
    }
    #[test]
    fn new_unrelated_workflow_does_not_repeat_a_failure_notice() {
        let mut w = watch();
        w.observe_runs(vec![run(1, "completed", Some("failure"))], 115);
        let first = w.notice_revision();
        w.observe_runs(vec![run(2, "in_progress", None)], 130);
        assert_eq!(w.notice_revision(), first);
        w.observe_runs(vec![run(2, "completed", Some("failure"))], 145);
        assert_ne!(w.notice_revision(), first);
    }
    #[test]
    fn skipped_cancelled_unknown_and_downtime_are_not_success() {
        let mut w = watch();
        w.observe_runs(vec![run(1, "completed", Some("skipped"))], 10000);
        assert_eq!(w.discovery_seconds, 15);
        assert_eq!(w.outcome(), ActionsOutcome::Skipped);
        w.runs = vec![run(1, "completed", Some("cancelled"))];
        assert_eq!(w.outcome(), ActionsOutcome::Cancelled);
        w.runs = vec![run(1, "completed", Some("new_conclusion"))];
        assert_eq!(w.outcome(), ActionsOutcome::Unknown);
        assert!(!w.runs[0].terminal());
    }
}
