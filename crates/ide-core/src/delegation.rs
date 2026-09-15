//! Provider-neutral, durable delegation state machine. Runtime idle is not task completion.
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::experts::ExpertSnapshot;
use crate::ProjectId;

pub mod context;
pub mod workspace;

pub const BETA_DISABLED: &str =
    "Delegation is off. Enable it in Settings → Beta features → Delegation to start a new run.";

pub fn enabled() -> bool {
    // Availability of Expert profiles and managed-run recovery. Starting a new
    // run additionally requires the persisted Beta features opt-in.
    !std::env::var("CHORO_EXPERTS").is_ok_and(|v| v == "0")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationBinding {
    pub run_id: Uuid,
    pub parent_agent_id: Uuid,
    pub task_id: Option<Uuid>,
    pub attempt_id: Option<Uuid>,
    pub workspace: Option<PathBuf>,
    /// Resolved assignment scope for provider enforcement, including direct
    /// corrections. Legacy children are repaired from the run before dispatch.
    #[serde(default)]
    pub task_kind: Option<TaskKind>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationLimits {
    pub concurrent_per_run: usize,
    pub concurrent_global: usize,
    pub work_revisions: usize,
}
impl Default for DelegationLimits {
    fn default() -> Self {
        Self {
            concurrent_per_run: 3,
            concurrent_global: 6,
            work_revisions: 3,
        }
    }
}
impl DelegationLimits {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            (1..=6).contains(&self.concurrent_per_run)
                && (1..=12).contains(&self.concurrent_global)
                && (1..=10).contains(&self.work_revisions),
            "Delegation limits are outside supported bounds."
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Preparing,
    Active,
    Waiting,
    Paused,
    Interrupted,
    Blocked,
    Completed,
    Cancelled,
}
impl RunStatus {
    pub fn stopped(self) -> bool {
        matches!(self, Self::Paused | Self::Interrupted | Self::Blocked)
    }
    pub fn dispatchable(self) -> bool {
        matches!(self, Self::Active | Self::Waiting)
    }
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Planned,
    Queued,
    Preparing,
    Running,
    WaitingForLead,
    NeedsUser,
    CompletionRequested,
    ResultReady,
    Integrating,
    Integrated,
    Accepted,
    Paused,
    Failed,
    Cancelled,
    Superseded,
}
impl TaskStatus {
    pub fn satisfied(self) -> bool {
        matches!(self, Self::Integrated | Self::Accepted)
    }
    pub fn terminal(self) -> bool {
        self.satisfied() || matches!(self, Self::Cancelled | Self::Superseded)
    }
    pub fn occupies_slot(self) -> bool {
        matches!(
            self,
            Self::Preparing
                | Self::Running
                | Self::WaitingForLead
                | Self::NeedsUser
                | Self::CompletionRequested
        )
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Planned => "Waiting for lead",
            Self::Queued => "Queued",
            Self::Preparing => "Preparing working copy",
            Self::Running => "Working",
            Self::WaitingForLead => "Waiting for lead",
            Self::NeedsUser => "Needs your input",
            Self::CompletionRequested => "Finishing",
            Self::ResultReady => "Bandmate finished",
            Self::Integrating => "Integrating",
            Self::Integrated => "Integrated",
            Self::Accepted => "Accepted",
            Self::Paused => "Paused",
            Self::Failed => "Needs attention",
            Self::Cancelled => "Cancelled",
            Self::Superseded => "Superseded",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Implementation,
    Consultation,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskPlan {
    pub key: String,
    pub expert_id: Uuid,
    pub goal: String,
    pub brief: String,
    pub expected_outcome: String,
    pub repository: PathBuf,
    #[serde(default)]
    pub dependencies: Vec<String>,
    pub kind: TaskKind,
    #[serde(default)]
    pub held: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskResult {
    pub summary: String,
    pub addressed: Vec<String>,
    pub checks: Vec<String>,
    pub unresolved: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationAttempt {
    pub id: Uuid,
    pub child_agent_id: Uuid,
    pub generation: u64,
    pub workspace: PathBuf,
    pub snapshot: Option<workspace::WorkspaceSnapshot>,
    #[serde(default)]
    pub completed_snapshot: Option<workspace::WorkspaceSnapshot>,
    #[serde(default)]
    pub archive_snapshot: Option<workspace::WorkspaceSnapshot>,
    #[serde(default)]
    pub working_copy_cleaned: bool,
    pub result: Option<TaskResult>,
    pub result_revision: Option<u64>,
    pub report_requested: bool,
    pub session_id: Option<String>,
    pub progress: String,
    pub usage: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationTask {
    pub id: Uuid,
    pub plan: TaskPlan,
    pub expert: ExpertSnapshot,
    pub revision: u64,
    pub status: TaskStatus,
    pub attempts: Vec<DelegationAttempt>,
    pub reason: Option<String>,
    pub integration: Option<workspace::IntegrationOperation>,
    #[serde(default)]
    pub deletion_confirmation: Option<Uuid>,
    #[serde(default)]
    pub preview: Option<DelegationPreview>,
    #[serde(default)]
    pub paused_status: Option<TaskStatus>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationPreview {
    pub url: String,
    pub title: String,
    pub revision: u64,
}
impl DelegationTask {
    pub fn attempt(&self) -> Option<&DelegationAttempt> {
        self.attempts.last()
    }
    pub fn attempt_mut(&mut self) -> Option<&mut DelegationAttempt> {
        self.attempts.last_mut()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationEvent {
    pub sequence: u64,
    pub task_id: Option<Uuid>,
    pub author: Uuid,
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Queued,
    Dispatched,
    Acknowledged,
    Uncertain,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationDelivery {
    pub id: Uuid,
    pub target: Uuid,
    pub task_id: Option<Uuid>,
    pub text: String,
    pub status: DeliveryStatus,
    pub task_revision: Option<u64>,
    #[serde(default)]
    pub runtime_generation: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WaitCondition {
    pub after_sequence: u64,
    pub tasks: Vec<Uuid>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OperationReceipt {
    pub input_hash: String,
    pub response: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DelegationRun {
    pub id: Uuid,
    pub parent_agent_id: Uuid,
    pub project_id: ProjectId,
    pub source_message_id: Uuid,
    pub original_assignment: String,
    pub authorized_experts: Vec<Uuid>,
    #[serde(default)]
    pub temporary_experts: Vec<ExpertSnapshot>,
    pub revision: u64,
    pub status: RunStatus,
    pub pause_reason: Option<String>,
    pub plan_mode: bool,
    pub limits: DelegationLimits,
    pub tasks: Vec<DelegationTask>,
    pub events: Vec<DelegationEvent>,
    pub deliveries: Vec<DelegationDelivery>,
    pub wait: Option<WaitCondition>,
    pub operations: BTreeMap<String, OperationReceipt>,
    pub verification: Option<String>,
}

impl DelegationRun {
    pub fn new(
        parent: Uuid,
        project: ProjectId,
        source: Uuid,
        text: String,
        experts: Vec<Uuid>,
        plan_mode: bool,
        limits: DelegationLimits,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            parent_agent_id: parent,
            project_id: project,
            source_message_id: source,
            original_assignment: text,
            authorized_experts: experts,
            temporary_experts: Vec::new(),
            revision: 1,
            status: RunStatus::Preparing,
            pause_reason: None,
            plan_mode,
            limits,
            tasks: Vec::new(),
            events: Vec::new(),
            deliveries: Vec::new(),
            wait: None,
            operations: BTreeMap::new(),
            verification: None,
        }
    }
    pub fn event(
        &mut self,
        author: Uuid,
        task_id: Option<Uuid>,
        kind: &str,
        text: impl Into<String>,
    ) {
        let sequence = self.events.last().map_or(1, |e| e.sequence + 1);
        self.events.push(DelegationEvent {
            sequence,
            task_id,
            author,
            kind: kind.into(),
            text: text.into(),
        });
    }
    pub fn queue(&mut self, target: Uuid, task_id: Option<Uuid>, text: String) -> Uuid {
        let id = Uuid::new_v4();
        let task_revision = task_id
            .and_then(|id| self.task(id).ok())
            .map(|t| t.revision);
        self.deliveries.push(DelegationDelivery {
            id,
            target,
            task_id,
            text,
            status: DeliveryStatus::Queued,
            task_revision,
            runtime_generation: None,
        });
        id
    }
    pub fn task(&self, id: Uuid) -> Result<&DelegationTask> {
        self.tasks
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow::anyhow!("Unknown task in this run."))
    }
    pub fn task_mut(&mut self, id: Uuid) -> Result<&mut DelegationTask> {
        self.tasks
            .iter_mut()
            .find(|t| t.id == id)
            .ok_or_else(|| anyhow::anyhow!("Unknown task in this run."))
    }
    pub fn authorize_caller(&self, caller: Uuid) -> Result<()> {
        ensure!(
            caller == self.parent_agent_id
                || self
                    .tasks
                    .iter()
                    .any(|t| t.attempt().is_some_and(|a| a.child_agent_id == caller)),
            "This caller is outside the delegation run."
        );
        Ok(())
    }
    pub fn require_lead(&self, caller: Uuid) -> Result<()> {
        ensure!(
            caller == self.parent_agent_id,
            "Only the lead can change assignments."
        );
        Ok(())
    }
    pub fn add_plans(
        &mut self,
        caller: Uuid,
        plans: Vec<TaskPlan>,
        experts: &[ExpertSnapshot],
    ) -> Result<()> {
        self.require_lead(caller)?;
        ensure!(
            !self.status.terminal()
                && !matches!(self.status, RunStatus::Paused | RunStatus::Interrupted),
            "Resume this task in Choro before scheduling work."
        );
        ensure!(
            self.tasks.len()
                + plans
                    .iter()
                    .filter(|p| !self.tasks.iter().any(|t| t.plan.key == p.key))
                    .count()
                <= 64,
            "A run supports at most 64 assignments."
        );
        let mut tasks = self.tasks.clone();
        let mut input_keys = HashSet::new();
        for plan in plans {
            ensure!(
                input_keys.insert(plan.key.clone()),
                "Duplicate task keys in the submitted plan."
            );
            ensure!(
                !plan.key.trim().is_empty()
                    && plan.key.len() <= 100
                    && !plan.goal.trim().is_empty()
                    && !plan.brief.trim().is_empty()
                    && !plan.expected_outcome.trim().is_empty(),
                "Every task needs a key, goal, brief, and expected outcome."
            );
            ensure!(
                plan.brief.len() <= 64_000
                    && plan.goal.len() <= 1_000
                    && plan.expected_outcome.len() <= 16_000,
                "Task brief is too long."
            );
            ensure!(
                self.authorized_experts.contains(&plan.expert_id),
                "The user has not named this Bandmate for the current task."
            );
            ensure!(
                !self.plan_mode || plan.kind == TaskKind::Consultation,
                "Plan mode only permits consultation tasks."
            );
            let mut expert = experts
                .iter()
                .find(|e| e.profile.id == plan.expert_id)
                .ok_or_else(|| anyhow::anyhow!("Bandmate configuration is unavailable."))?
                .clone();
            if self
                .temporary_experts
                .iter()
                .any(|e| e.profile.id == plan.expert_id)
            {
                expert.profile.name = plan.goal.chars().take(80).collect();
            }
            let status = if plan.held {
                TaskStatus::Planned
            } else {
                TaskStatus::Queued
            };
            if let Some(existing) = tasks.iter_mut().find(|t| t.plan.key == plan.key) {
                ensure!(matches!(existing.status,TaskStatus::Planned|TaskStatus::Queued),"Running or completed briefs require request_changes and a new revision before replanning.");
                ensure!(
                    existing.attempts.is_empty() || existing.plan.expert_id == plan.expert_id,
                    "A task chat cannot switch Bandmates. Create another assignment instead."
                );
                existing.plan = plan;
                existing.revision += 1;
                existing.status = status;
                if existing.attempts.is_empty() {
                    existing.expert = expert;
                }
                continue;
            }
            tasks.push(DelegationTask {
                id: Uuid::new_v4(),
                plan,
                expert,
                revision: 1,
                status,
                attempts: Vec::new(),
                reason: None,
                integration: None,
                deletion_confirmation: None,
                preview: None,
                paused_status: None,
            });
        }
        validate_dependencies(&tasks)?;
        self.tasks = tasks;
        self.event(caller, None, "plan", "Assignments updated");
        Ok(())
    }
    pub fn ready_tasks(&self, global_active: usize) -> Vec<Uuid> {
        if !self.status.dispatchable() {
            return Vec::new();
        }
        let active = self
            .tasks
            .iter()
            .filter(|t| t.status.occupies_slot())
            .count();
        let slots = self
            .limits
            .concurrent_per_run
            .saturating_sub(active)
            .min(self.limits.concurrent_global.saturating_sub(global_active));
        self.tasks
            .iter()
            .filter(|t| {
                t.status == TaskStatus::Queued
                    && (!self.plan_mode || t.plan.kind == TaskKind::Consultation)
                    && t.plan.dependencies.iter().all(|key| {
                        self.tasks
                            .iter()
                            .any(|d| &d.plan.key == key && d.status.satisfied())
                    })
            })
            .take(slots)
            .map(|t| t.id)
            .collect()
    }
    pub fn message(
        &mut self,
        caller: Uuid,
        task_id: Uuid,
        text: String,
        blocking: bool,
    ) -> Result<()> {
        ensure!(
            !text.trim().is_empty() && text.len() <= 16_000,
            "Message must contain 1–16000 bytes."
        );
        let parent = self.parent_agent_id;
        let task = self.task_mut(task_id)?;
        ensure!(
            matches!(task.status,TaskStatus::Running|TaskStatus::WaitingForLead|TaskStatus::CompletionRequested),
            "This Bandmate task is not running. Resume a paused task or request_changes for a finished result."
        );
        let child = task
            .attempt()
            .ok_or_else(|| anyhow::anyhow!("This task has not started."))?
            .child_agent_id;
        ensure!(
            caller == parent || caller == child,
            "Bandmates communicate through their lead, not with siblings."
        );
        if caller == child && blocking {
            task.status = TaskStatus::WaitingForLead;
        }
        if caller == parent {
            task.revision += 1;
            task.plan.brief.push_str(&format!(
                "\n\nLead coordination (revision {}):\n{text}",
                task.revision
            ));
            if let Some(a) = task.attempt_mut() {
                a.result = None;
                a.result_revision = None;
                a.completed_snapshot = None;
                a.report_requested = false;
            }
            task.status = TaskStatus::Running;
        }
        let text = if caller == parent {
            format!(
                "Task revision is now {}. Earlier completion reports are invalid.\n{text}",
                task.revision
            )
        } else {
            text
        };
        let target = if caller == parent { child } else { parent };
        self.event(
            caller,
            Some(task_id),
            if blocking { "question" } else { "message" },
            text.clone(),
        );
        self.queue(target, Some(task_id), text);
        Ok(())
    }
    pub fn complete(
        &mut self,
        caller: Uuid,
        task_id: Uuid,
        attempt_id: Uuid,
        revision: u64,
        result: TaskResult,
    ) -> Result<()> {
        let task = self.task_mut(task_id)?;
        ensure!(
            task.revision == revision,
            "This result belongs to an outdated assignment."
        );
        ensure!(
            matches!(
                task.status,
                TaskStatus::Running | TaskStatus::WaitingForLead | TaskStatus::CompletionRequested
            ),
            "This task cannot report completion in its current state."
        );
        ensure!(
            !result.summary.trim().is_empty() && result.summary.len() <= 16_000,
            "Provide a bounded completion report."
        );
        ensure!(
            serde_json::to_vec(&result)?.len() <= 64_000,
            "Completion report is too large. Summarize checks and link detailed evidence."
        );
        let attempt = task
            .attempt_mut()
            .ok_or_else(|| anyhow::anyhow!("No active attempt."))?;
        ensure!(
            attempt.id == attempt_id && attempt.child_agent_id == caller,
            "Only the current Bandmate attempt can report its result."
        );
        attempt.result = Some(result);
        attempt.result_revision = Some(revision);
        task.status = TaskStatus::CompletionRequested;
        Ok(())
    }
    /// Trusted user surface only. Corrections are durable and revoke stale
    /// results immediately; they never grant the child additional authority.
    pub fn user_correction(&mut self, task_id: Uuid, text: String) -> Result<()> {
        ensure!(
            !self.status.terminal(),
            "Start a new task to change a completed run."
        );
        ensure!(
            !text.trim().is_empty() && text.len() <= 32_000,
            "Correction is empty or too long."
        );
        let task = self.task(task_id)?;
        ensure!(
            !matches!(task.status, TaskStatus::Cancelled | TaskStatus::Superseded),
            "This assignment has ended. Send the correction to the lead for a new assignment."
        );
        ensure!(
            !task.status.satisfied(),
            "Integrated work needs a new revision from the lead. Send this correction to the lead."
        );
        ensure!(
            task.status != TaskStatus::Integrating,
            "Integration is in progress. Stop the run before correcting this result."
        );
        let task = self.task_mut(task_id)?;
        task.revision += 1;
        task.plan.brief.push_str(&format!(
            "\n\nUser correction (revision {}):\n{text}",
            task.revision
        ));
        let revision = task.revision;
        let a = task
            .attempt_mut()
            .ok_or_else(|| anyhow::anyhow!("This Bandmate has not started yet."))?;
        let child = a.child_agent_id;
        a.result = None;
        a.result_revision = None;
        a.completed_snapshot = None;
        a.report_requested = false;
        task.integration = None;
        task.deletion_confirmation = None;
        if task.status == TaskStatus::Paused {
            task.paused_status = Some(TaskStatus::Running);
        } else {
            task.status = TaskStatus::Running;
        }
        self.event(
            Uuid::new_v4(),
            Some(task_id),
            "user_correction",
            text.clone(),
        );
        self.queue(child,Some(task_id),format!("User correction. Task revision is now {revision}; previous results are invalid.\n{text}"));
        self.queue(
            self.parent_agent_id,
            Some(task_id),
            format!("The user corrected task {task_id}; its revision is now {revision}.\n{text}"),
        );
        Ok(())
    }
    pub fn control(
        &mut self,
        caller: Uuid,
        task_id: Uuid,
        action: &str,
        reason: &str,
    ) -> Result<()> {
        self.require_lead(caller)?;
        let revisions = self.limits.work_revisions;
        let task = self.task_mut(task_id)?;
        match action {
            "hold" => {
                ensure!(
                    matches!(task.status, TaskStatus::Planned | TaskStatus::Queued),
                    "Only unstarted tasks can be held."
                );
                task.status = TaskStatus::Planned;
            }
            "release" => {
                ensure!(
                    task.status == TaskStatus::Planned,
                    "Only a held task can be released. User-paused tasks require Resume."
                );
                task.status = TaskStatus::Queued;
            }
            "cancel" => {
                ensure!(
                    !reason.trim().is_empty(),
                    "Explain why this assignment is cancelled."
                );
                ensure!(
                    !task.status.satisfied(),
                    "Integrated work cannot be cancelled or reverted automatically."
                );
                task.status = TaskStatus::Cancelled;
                task.reason = Some(reason.into());
            }
            "accept" => {
                ensure!(
                    task.plan.kind == TaskKind::Consultation
                        && task.status == TaskStatus::ResultReady,
                    "Only completed consultation results can be accepted."
                );
                task.status = TaskStatus::Accepted;
            }
            "request_changes" => {
                ensure!(
                    matches!(
                        task.status,
                        TaskStatus::ResultReady
                            | TaskStatus::Integrated
                            | TaskStatus::Accepted
                            | TaskStatus::Failed
                    ),
                    "Wait for the current attempt to settle before requesting changes."
                );
                ensure!(
                    task.attempts.len() < revisions,
                    "The task reached its work revision limit. Ask the user how to proceed."
                );
                ensure!(!reason.trim().is_empty(), "Describe the requested changes.");
                task.revision += 1;
                task.plan.brief.push_str(&format!(
                    "\n\nRevision {} requested by the lead:\n{}",
                    task.revision, reason
                ));
                task.integration = None;
                task.reason = None;
                task.status = TaskStatus::Queued;
            }
            _ => anyhow::bail!("Unknown task action."),
        }
        self.event(caller, Some(task_id), action, reason);
        Ok(())
    }
    /// Runtime errors stop dispatch with the same uncertain-delivery gate as
    /// Stop. Resume must reconcile any command that may have reached a provider.
    pub fn block(&mut self, reason: &str) {
        if !self.status.dispatchable() && self.status != RunStatus::Preparing {
            return;
        }
        self.pause(reason, false);
        self.status = RunStatus::Blocked;
        self.event(self.parent_agent_id, None, "blocked", reason);
    }

    pub fn pause(&mut self, reason: &str, interrupted: bool) {
        if self.status.terminal() {
            return;
        }
        self.status = if interrupted {
            RunStatus::Interrupted
        } else {
            RunStatus::Paused
        };
        self.pause_reason = Some(reason.into());
        for delivery in &mut self.deliveries {
            if delivery.status == DeliveryStatus::Dispatched {
                delivery.status = DeliveryStatus::Uncertain;
            }
        }
        self.event(self.parent_agent_id, None, "paused", reason);
    }
    /// Only called by a trusted UI action, never exposed as an agent command.
    pub fn resume(&mut self) -> Result<()> {
        ensure!(
            matches!(
                self.status,
                RunStatus::Paused | RunStatus::Interrupted | RunStatus::Blocked
            ),
            "This task is not paused."
        );
        ensure!(!self.deliveries.iter().any(|d| d.status == DeliveryStatus::Uncertain), "A previous delivery has an uncertain outcome. Reconcile its conversation before resuming.");
        self.status = RunStatus::Preparing;
        self.pause_reason = None;
        self.event(self.parent_agent_id, None, "resumed", "Resumed by user");
        Ok(())
    }
    pub fn wait_satisfied(&self) -> bool {
        self.wait.as_ref().is_some_and(|wait| {
            self.events.iter().any(|e| {
                e.sequence > wait.after_sequence
                    && (wait.tasks.is_empty()
                        || e.task_id.is_some_and(|id| wait.tasks.contains(&id)))
                    && matches!(
                        e.kind.as_str(),
                        "result" | "question" | "failed" | "integrated"
                    )
            })
        })
    }
    pub fn finish(&mut self, caller: Uuid, summary: String, verification: String) -> Result<()> {
        self.require_lead(caller)?;
        ensure!(
            self.status.dispatchable(),
            "A paused or interrupted run cannot complete."
        );
        ensure!(
            !self.tasks.is_empty(),
            "No assignments have been recorded for this delegation run."
        );
        ensure!(
            self.tasks.iter().all(|t| t.status.terminal()),
            "Assignments remain unfinished or unintegrated."
        );
        ensure!(
            !summary.trim().is_empty() && !verification.trim().is_empty(),
            "Provide the outcome and verification against the user's request."
        );
        self.verification = Some(verification);
        self.status = RunStatus::Completed;
        self.event(caller, None, "completed", summary);
        Ok(())
    }
}

pub fn validate_dependencies(tasks: &[DelegationTask]) -> Result<()> {
    fn visit(
        key: &str,
        tasks: &[DelegationTask],
        active: &mut HashSet<String>,
        done: &mut HashSet<String>,
    ) -> Result<()> {
        if done.contains(key) {
            return Ok(());
        }
        ensure!(
            active.insert(key.into()),
            "Task dependencies contain a cycle."
        );
        let task = tasks
            .iter()
            .find(|t| t.plan.key == key)
            .ok_or_else(|| anyhow::anyhow!("Unknown dependency: {key}"))?;
        for dependency in &task.plan.dependencies {
            visit(dependency, tasks, active, done)?;
        }
        active.remove(key);
        done.insert(key.into());
        Ok(())
    }
    let mut done = HashSet::new();
    for task in tasks {
        visit(&task.plan.key, tasks, &mut HashSet::new(), &mut done)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::experts::ExpertProfile;
    use crate::{AgentEffort, AgentKind, AgentModel};
    fn fixture() -> (DelegationRun, ExpertSnapshot) {
        let model = AgentModel::default_for(AgentKind::Codex);
        let expert = ExpertProfile {
            id: Uuid::new_v4(),
            revision: 1,
            name: "Builder".into(),
            description: String::new(),
            provider: AgentKind::Codex,
            model,
            effort: AgentEffort::default(),
            instructions: "Build it".into(),
            skills: vec![],
            expected_outcome: "Checked".into(),
            enabled: true,
            archived: false,
            additions: Default::default(),
        };
        let snapshot = ExpertSnapshot {
            profile: expert,
            skills: vec![],
        };
        let mut run = DelegationRun::new(
            Uuid::new_v4(),
            ProjectId(Uuid::new_v4()),
            Uuid::new_v4(),
            "Build using Builder".into(),
            vec![snapshot.profile.id],
            false,
            DelegationLimits::default(),
        );
        run.status = RunStatus::Active;
        (run, snapshot)
    }
    fn plan(expert: &ExpertSnapshot, key: &str, dependencies: Vec<&str>) -> TaskPlan {
        TaskPlan {
            key: key.into(),
            expert_id: expert.profile.id,
            goal: "Build".into(),
            brief: "Implement it".into(),
            expected_outcome: "Tests pass".into(),
            repository: PathBuf::from("/tmp/repo"),
            dependencies: dependencies.into_iter().map(str::to_string).collect(),
            kind: TaskKind::Implementation,
            held: false,
        }
    }
    #[test]
    fn dependency_waits_for_integration_and_rejects_cycles_atomically() {
        let (mut run, expert) = fixture();
        run.add_plans(
            run.parent_agent_id,
            vec![plan(&expert, "a", vec![]), plan(&expert, "b", vec!["a"])],
            &[expert.clone()],
        )
        .unwrap();
        assert_eq!(run.ready_tasks(0), vec![run.tasks[0].id]);
        run.tasks[0].status = TaskStatus::ResultReady;
        assert!(run.ready_tasks(0).is_empty());
        run.tasks[0].status = TaskStatus::Integrated;
        assert_eq!(run.ready_tasks(0), vec![run.tasks[1].id]);
        assert!(run
            .add_plans(
                run.parent_agent_id,
                vec![plan(&expert, "c", vec!["d"]), plan(&expert, "d", vec!["c"])],
                &[expert]
            )
            .is_err());
        assert_eq!(run.tasks.len(), 2);
    }
    #[test]
    fn pause_is_durable_and_late_events_cannot_resume() {
        let (mut run, expert) = fixture();
        run.add_plans(
            run.parent_agent_id,
            vec![plan(&expert, "a", vec![])],
            &[expert],
        )
        .unwrap();
        run.pause("Stopped by user", false);
        run.event(
            Uuid::new_v4(),
            Some(run.tasks[0].id),
            "result",
            "Late result",
        );
        let restored: DelegationRun =
            serde_json::from_str(&serde_json::to_string(&run).unwrap()).unwrap();
        assert!(restored.ready_tasks(0).is_empty());
        assert!(run
            .finish(run.parent_agent_id, "Done".into(), "Checked".into())
            .is_err());
        run.resume().unwrap();
        assert_eq!(run.status, RunStatus::Preparing);
    }
    #[test]
    fn forged_experts_and_plan_mode_writes_are_rejected() {
        let (mut run, mut expert) = fixture();
        expert.profile.id = Uuid::new_v4();
        assert!(run
            .add_plans(
                run.parent_agent_id,
                vec![plan(&expert, "a", vec![])],
                &[expert.clone()]
            )
            .is_err());
        run.authorized_experts.push(expert.profile.id);
        run.plan_mode = true;
        assert!(run
            .add_plans(
                run.parent_agent_id,
                vec![plan(&expert, "a", vec![])],
                &[expert]
            )
            .is_err());
    }
    #[test]
    fn wait_observes_result_already_available_and_global_limit() {
        let (mut run, expert) = fixture();
        run.add_plans(
            run.parent_agent_id,
            vec![plan(&expert, "a", vec![])],
            &[expert],
        )
        .unwrap();
        let id = run.tasks[0].id;
        run.event(run.parent_agent_id, Some(id), "result", "Ready");
        run.wait = Some(WaitCondition {
            after_sequence: 0,
            tasks: vec![id],
        });
        assert!(run.wait_satisfied());
        assert!(run.ready_tasks(6).is_empty());
    }
}
