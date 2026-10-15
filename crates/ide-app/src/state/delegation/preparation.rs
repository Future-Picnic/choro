//! Create inspectable conversations before doing slow, cancellable setup.
use super::*;

fn new_attempt(task: &DelegationTask) -> DelegationAttempt {
    let id = Uuid::new_v4();
    DelegationAttempt {
        id,
        child_agent_id: task.attempt().map_or_else(Uuid::new_v4, |a| a.child_agent_id),
        generation: 0,
        workspace: task.plan.repository.clone(),
        prepared: false,
        snapshot: None, completed_snapshot: None, archive_snapshot: None,
        working_copy_cleaned: false, result: None, result_revision: None,
        report_requested: false, session_id: None, progress: String::new(), usage: None,
    }
}

pub(super) fn child_record(store: &LocalStore, run: &DelegationRun, task: &DelegationTask, parent: &AgentRecord) -> Result<AgentRecord> {
    let attempt = task.attempt().context("Bandmate conversation has not been created")?;
    let expert = &task.expert.profile;
    let mut child = store.load_agents()?.into_iter().find(|a| a.id == attempt.child_agent_id)
        .unwrap_or_else(|| AgentRecord::new(parent.project_id, parent.project_path.clone(),
            format!("{} · {}", expert.name, task.plan.goal), task.plan.brief.clone(),
            expert.provider, expert.model, expert.effort, parent.access_mode));
    child.id = attempt.child_agent_id;
    child.runtime = AgentRuntimeKind::Chat;
    // The attempt survives a missing chat record. Recover its provider session
    // so opening the assignment cannot turn a continuation into a fresh chat.
    if child.chat_session_id.is_none() && child.cli_session_id.is_none() {
        if child.provider == ide_core::AgentKind::Codex {
            child.chat_session_id = attempt.session_id.clone();
        } else {
            child.cli_session_id = attempt.session_id.clone();
        }
    }
    child.repository_path = Some(task.plan.repository.clone());
    child.expert_snapshot = Some(task.expert.clone());
    child.delegation = Some(DelegationBinding {
        execution_version: SHARED_EXECUTION_VERSION,
        run_id: run.id, parent_agent_id: parent.id, task_id: Some(task.id),
        attempt_id: Some(attempt.id), workspace: Some(attempt.workspace.clone()),
        task_kind: Some(task.plan.kind),
    });
    child.linked_docs = parent.linked_docs.clone();
    Ok(child)
}

pub(super) fn provision(store: &LocalStore, run: DelegationRun, task_id: Uuid, parent: AgentRecord, begin: bool) -> Result<Effect> {
    run.require_shared()?;
    let task = run.task(task_id)?;
    let replacement = task.attempt().is_none_or(|a| begin && (a.prepared || a.generation > 0));
    let attempt = replacement.then(|| new_attempt(task));
    if begin || attempt.is_some() {
        store.update_delegation(run.id, Some(run.revision), |r| {
            if begin {
                ensure!(r.status.dispatchable() && r.task(task_id)?.status == TaskStatus::Queued,
                    "Assignment is no longer ready to prepare");
            }
            let task = r.task_mut(task_id)?;
            if let Some(attempt) = attempt { task.attempts.push(attempt); }
            if begin {
                task.status = TaskStatus::Preparing;
                task.reason = None;
                task.attempt_mut().unwrap().progress = "Preparing Bandmate".into();
            }
            Ok(())
        })?;
    }
    let run = store.load_delegation(run.id)?;
    Ok(Effect::Adopt(child_record(store, &run, run.task(task_id)?, &parent)?))
}

/// Expected cancellation is not a failure. A failed workspace/provider setup
/// affects its assignment only; unrelated queued work remains dispatchable.
pub(super) fn record_job_failure(store: &LocalStore, job: &Job, reason: &str) -> Result<()> {
    store.update_delegation(job.run().id, None, |run| {
        if run.status.stopped() || run.status.terminal() { return Ok(()); }
        if let Some(id) = job.task_id() {
            let task = run.task(id)?;
            if task.status.terminal() || task.status == TaskStatus::Paused { return Ok(()); }
            if matches!(job, Job::Prepare(..)) && task.status != TaskStatus::Preparing { return Ok(()); }
            if matches!(job, Job::Provision(..) | Job::BeginPrepare(..)) && run.revision != job.run().revision {
                return Ok(());
            }
            let old_attempt = job.run().task(id)?.attempt().map(|a| a.id);
            if old_attempt.is_some() && old_attempt != task.attempt().map(|a| a.id) { return Ok(()); }
            {
                let task = run.task_mut(id)?;
                task.status = TaskStatus::Failed;
                task.reason = Some(reason.to_owned());
                run.event(run.parent_agent_id, Some(id), "failed", reason);
                run.queue(run.parent_agent_id, Some(id), format!("Bandmate assignment {id} could not continue: {reason}. Other assignments can continue. Inspect this assignment and retry or revise it."));
                return Ok(());
            }
        }
        run.block(reason);
        Ok(())
    })
}
