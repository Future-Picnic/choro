//! Application-owned orchestration. Views observe this entity; MCP writes the
//! durable commands it consumes. No selected-chat state drives scheduling.
pub mod display;
#[cfg(test)]
mod tests;
use super::agent_chat::protocol::managed;
use super::agent_chat::{AgentChatEvent, AgentChatStatus, AgentInteractionMode};
use super::{AgentChatState, AgentRecords, TerminalManager};
use anyhow::{ensure, Context as _, Result};
use gpui::{App, AppContext, Context, Entity, Global};
use ide_core::{delegation::*, local_store::LocalStore, AgentRecord, AgentRuntimeKind};
use std::{
    collections::{HashMap, HashSet},
    time::Duration,
};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct DelegationHandle(pub Entity<DelegationCoordinator>);
impl Global for DelegationHandle {}

pub(crate) struct DelegationCoordinator {
    pub runs: Vec<DelegationRun>,
    pub error: Option<String>,
    agents: Entity<AgentRecords>,
    chats: Entity<AgentChatState>,
    terminals: Entity<TerminalManager>,
    settled: HashMap<Uuid, u64>,
    cursor: usize,
    stopped_parents: HashSet<Uuid>,
}

#[derive(Clone)]
enum Job {
    Configure(DelegationRun, AgentRecord),
    Prepare(DelegationRun, Uuid, AgentRecord),
    Deliver(DelegationRun, AgentRecord, Vec<Uuid>, AgentInteractionMode),
    Capture(DelegationRun, Uuid),
    Integrate(DelegationRun, Uuid),
    Acknowledge(DelegationRun, Uuid, u64),
    MissingReport(DelegationRun, Uuid),
    Failure(DelegationRun, Uuid, String),
}
impl Job {
    fn task_id(&self) -> Option<Uuid> {
        match self {
            Self::Prepare(_, id, _)
            | Self::Capture(_, id)
            | Self::Integrate(_, id)
            | Self::MissingReport(_, id)
            | Self::Failure(_, id, _) => Some(*id),
            Self::Deliver(_, a, _, _) => a.delegation.as_ref().and_then(|b| b.task_id),
            _ => None,
        }
    }
    fn run(&self) -> &DelegationRun {
        match self {
            Self::Configure(r, _)
            | Self::Prepare(r, _, _)
            | Self::Deliver(r, _, _, _)
            | Self::Capture(r, _)
            | Self::Integrate(r, _)
            | Self::Acknowledge(r, _, _)
            | Self::MissingReport(r, _)
            | Self::Failure(r, _, _) => r,
        }
    }
    fn reserved_agent(&self) -> Option<Uuid> {
        match self {
            Self::Configure(r, _) | Self::Prepare(r, _, _) | Self::Integrate(r, _) => {
                Some(r.parent_agent_id)
            }
            Self::Deliver(_, a, _, _) => Some(a.id),
            Self::Capture(r, t) => r.task(*t).ok()?.attempt().map(|a| a.child_agent_id),
            _ => None,
        }
    }
}
enum Effect {
    None,
    Adopt(AgentRecord),
    Send(
        AgentRecord,
        Vec<super::agent_chat::AgentChatTimelineItem>,
        String,
        AgentInteractionMode,
        Vec<Uuid>,
    ),
}

impl DelegationCoordinator {
    pub fn start(
        agents: Entity<AgentRecords>,
        chats: Entity<AgentChatState>,
        terminals: Entity<TerminalManager>,
        cx: &mut App,
    ) -> Entity<Self> {
        if let Some(existing) = cx.try_global::<DelegationHandle>() {
            return existing.0.clone();
        }
        let recovery_ids = LocalStore::open_default()
            .and_then(|s| s.load_delegations())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| !r.status.terminal())
            .map(|r| r.id)
            .collect::<Vec<_>>();
        let entity = cx.new(|cx| {
            cx.subscribe(&chats, |this: &mut Self, _, event, cx| {
                if let AgentChatEvent::WorkFinished { agent_id } = event {
                    this.settled
                        .insert(*agent_id, this.chats.read(cx).backend_generation(*agent_id));
                }
            })
            .detach();
            Self {
                runs: vec![],
                error: None,
                agents,
                chats,
                terminals,
                settled: HashMap::new(),
                cursor: 0,
                stopped_parents: HashSet::new(),
            }
        });
        cx.set_global(DelegationHandle(entity.clone()));
        let owner = Uuid::new_v4();
        entity.update(cx, |_, cx| {
            // Renewal is independent of slow snapshots and integrations.
            cx.spawn(async move |this,cx| loop {
                cx.background_executor().timer(Duration::from_secs(4)).await;
                if this.upgrade().is_none(){break;}
                let _=cx.background_executor().spawn(async move {LocalStore::open_default().and_then(|s|s.claim_delegation_coordinator(owner))}).await;
            }).detach();
            cx.spawn(async move |this, cx| {
                let mut recovered = false;
                loop {
                    cx.background_executor().timer(Duration::from_millis(750)).await;
                    let recovery = !recovered;
                    let interrupted_ids=recovery_ids.clone();
                    let loaded = cx.background_executor().spawn(async move {
                        let store = LocalStore::open_default()?;
                        ensure!(store.claim_delegation_coordinator(owner)?, "Another Choro window owns delegation. Waiting for its coordinator.");
                        if recovery {
                            for run in store.load_delegations()?.into_iter().filter(|r| !r.status.terminal()&&interrupted_ids.contains(&r.id)) {
                                store.update_delegation(run.id, None, |r| { r.pause("Application interrupted — Resume available", true); Ok(()) })?;
                            }
                        }
                        store.load_delegations()
                    }).await;
                    let runs = match loaded {
                        Ok(runs) => { recovered = true; runs },
                        Err(error) => { if this.update(cx, |s,cx| { s.error = Some(error.to_string()); cx.notify(); }).is_err() { break; } continue; },
                    };
                    let job = match this.update(cx, |s,cx| { s.runs = runs; s.error = None; cx.notify(); s.next_job(cx) }) { Ok(job) => job, Err(_) => break };
                    let Some(job) = job else { continue; };
                    let run_id = job.run().id;
                    let reserved = job.reserved_agent();
                    let work = job.clone();
                    let result = cx.background_executor().spawn(async move { execute(work,owner) }).await;
                    if this.update(cx, |s,cx| {
                        if let Some(id) = reserved { s.chats.update(cx, |chats,_| { chats.delegation_reservations.remove(&id); }); }
                        match result {
                            Ok(effect) => if let Err(error) = s.apply_effect(run_id, effect, cx) { s.fail_run(run_id, error.to_string()); },
                            Err(error) => {
                                let stopped=job.task_id().and_then(|id|LocalStore::open_default().ok()?.load_delegation(run_id).ok()?.task(id).ok().map(|t|matches!(t.status,TaskStatus::Paused|TaskStatus::Cancelled))).unwrap_or(false);
                                if !stopped{s.fail_run(run_id, error.to_string());}
                            },
                        }
                        if let Some(id) = reserved { s.chats.update(cx, |chats,cx| chats.release_delegation_reservation(id,cx)); }
                        cx.notify();
                    }).is_err() { break; }
                }
            }).detach();
        });
        entity
    }

    fn fail_run(&mut self, id: Uuid, error: String) {
        self.error = Some(error.clone());
        if let Ok(store) = LocalStore::open_default() {
            let _ = store.update_delegation(id, None, |r| {
                r.block(&error);
                Ok(())
            });
        }
    }

    fn stop_runtime(&self, id: Uuid, cx: &mut Context<Self>) {
        self.chats.update(cx, |chats, cx| {
            if chats.has_backend(id) {
                chats.force_stop_backend(id, cx);
            }
        });
        if let Some(a) = self.agents.read(cx).agent(id) {
            let terminals = self
                .terminals
                .read(cx)
                .sessions_for(a.project_id)
                .into_iter()
                .filter(|s| {
                    s.agent_record_id == Some(id)
                        || a.delegation.as_ref().is_some_and(|b| b.task_id.is_some())
                            && s.cwd.starts_with(a.runtime_path())
                })
                .map(|s| s.id)
                .collect::<Vec<_>>();
            for terminal in terminals {
                self.terminals.update(cx, |t, cx| t.close(terminal, cx));
            }
        }
    }

    /// A user message may restart the lead's conversation while Expert work
    /// remains stopped. Observe the pause before dispatch so the next poll
    /// cannot kill that newly requested turn.
    pub fn prepare_parent_message(&mut self, parent: Uuid, cx: &mut Context<Self>) -> Result<bool> {
        let runs = LocalStore::open_default()?.load_delegations()?;
        let Some(run) = runs
            .iter()
            .find(|r| r.parent_agent_id == parent && r.status.stopped())
        else {
            return Ok(false);
        };
        if parent_stop_required(&mut self.stopped_parents, run) {
            self.stop_runtime(parent, cx);
        }
        Ok(true)
    }

    fn next_job(&mut self, cx: &mut Context<Self>) -> Option<Job> {
        for run in &self.runs {
            let parent = self.agents.read(cx).agent(run.parent_agent_id).cloned();
            if let Some(parent) = parent {
                self.chats.update(cx,|chats,cx| {
                    let session=chats.ensure_session(parent.id,parent.title.clone(),cx);
                    if !session.timeline.iter().any(|i|matches!(i,super::agent_chat::AgentChatTimelineItem::DelegationGroup{run_id,..}if *run_id==run.id)) {
                        let position=session.timeline.iter().rposition(|i|matches!(i,super::agent_chat::AgentChatTimelineItem::Message(super::agent_chat::AgentChatMessage::User{text,..})if text.contains(&run.original_assignment)));
                        if let Some(position)=position {
                            let item=super::agent_chat::AgentChatTimelineItem::DelegationGroup{run_id:run.id,created_at:ide_core::agents::unix_now()};
                            session.timeline.insert(position+1,item.clone());
                            super::agent_chat::persist_timeline_item(parent.id,item,cx);
                            cx.emit(AgentChatEvent::Changed);cx.notify();
                        }
                    }
                });
                for task in run
                    .tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::Integrated)
                {
                    if let Some(op) = &task.integration {
                        let turn = format!("delegation-integration-{}", op.id);
                        let needs_receipt=self.chats.read(cx).session(parent.id).is_some_and(|s|!s.messages.is_empty()&&!s.timeline.iter().any(|i|matches!(i,super::agent_chat::AgentChatTimelineItem::ChangedFiles(summary)if summary.turn_id.as_deref()==Some(&turn))));
                        if needs_receipt {
                            match integration_summary(op, parent.runtime_path()) {
                                Ok(summary) => {
                                    self.agents.update(cx, |records, cx| {
                                        records.merge_changed_files(parent.id, &summary.files, cx)
                                    });
                                    self.chats.update(cx, |chats, cx| {
                                        chats.record_delegation_integration(parent.id, summary, cx)
                                    });
                                }
                                Err(e) => self.error = Some(e.to_string()),
                            }
                        }
                    }
                }
            }
        }
        // Keep provider resume IDs in the same authoritative record owner even
        // when neither the parent nor its children have a mounted chat view.
        for a in self
            .agents
            .read(cx)
            .all_records()
            .into_iter()
            .filter(|a| a.delegation.is_some())
        {
            if let Some(s) = self.chats.read(cx).session(a.id) {
                let chat = s.chat_session_id.clone();
                let cli = s.cli_session_id.clone();
                let usage = s.usage.as_ref().and_then(|u| serde_json::to_value(u).ok());
                let session_id = chat.clone().or(cli.clone());
                if let Some(b) = a.delegation.as_ref().filter(|b| b.task_id.is_some()) {
                    let changed = self
                        .runs
                        .iter()
                        .find(|r| r.id == b.run_id)
                        .and_then(|r| r.task(b.task_id?).ok())
                        .and_then(|t| t.attempt())
                        .is_some_and(|attempt| {
                            attempt.session_id != session_id || attempt.usage != usage
                        });
                    if changed {
                        if let Ok(store) = LocalStore::open_default() {
                            if let Some(attempt) = b.attempt_id {
                                let _ = store.update_delegation_telemetry(
                                    b.run_id,
                                    b.task_id.unwrap(),
                                    attempt,
                                    session_id,
                                    usage,
                                );
                            }
                        }
                    }
                }
                self.agents.update(cx, |records, cx| {
                    if let Some(id) = chat {
                        records.set_chat_session_id(a.id, id, cx);
                    }
                    if let Some(id) = cli {
                        records.set_cli_session_id(a.id, id, cx);
                    }
                });
            }
        }
        let total = self
            .runs
            .iter()
            .filter(|r| r.status.dispatchable())
            .flat_map(|r| &r.tasks)
            .filter(|t| t.status.occupies_slot())
            .count();
        let n = self.runs.len();
        for offset in 0..n {
            let index = (self.cursor + offset) % n;
            let run = self.runs[index].clone();
            let Some(parent) = self.agents.read(cx).agent(run.parent_agent_id).cloned() else {
                continue;
            };
            let stop_parent = parent_stop_required(&mut self.stopped_parents, &run);
            if run.status.stopped() {
                if stop_parent {
                    self.stop_runtime(parent.id, cx);
                }
                for t in &run.tasks {
                    if let Some(a) = t.attempt() {
                        self.stop_runtime(a.child_agent_id, cx);
                    }
                }
                continue;
            }
            if run.status.terminal() {
                if can_restore_lead_policy(&run, &parent)
                    && self.chats.read(cx).safe_for_delegation(parent.id)
                {
                    let mut parent = parent;
                    if self.chats.read(cx).has_backend(parent.id)
                        && !self
                            .chats
                            .update(cx, |chats, cx| chats.retire_idle_backend(parent.id, cx))
                    {
                        continue;
                    }
                    parent.delegation = None;
                    self.agents.update(cx, |a, cx| a.adopt_managed(parent, cx));
                }
                continue;
            }
            let safe = self.chats.read(cx).safe_for_delegation(parent.id);
            let job = if run.status == RunStatus::Preparing {
                (safe && !run.tasks.is_empty()).then(|| Job::Configure(run.clone(), parent.clone()))
            } else {
                self.choose_active(&run, &parent, safe, total, cx)
            };
            if let Some(job) = job {
                self.cursor = (index + 1) % n;
                if let Some(id) = job.reserved_agent() {
                    self.chats.update(cx, |chats, _| {
                        chats.delegation_reservations.insert(id);
                    });
                }
                return Some(job);
            }
        }
        None
    }

    fn choose_active(
        &self,
        run: &DelegationRun,
        parent: &AgentRecord,
        safe: bool,
        total: usize,
        cx: &mut Context<Self>,
    ) -> Option<Job> {
        for task in run.tasks.iter().filter(|t| t.status == TaskStatus::Queued) {
            if let Some(dependency) = task.plan.dependencies.iter().find(|key| {
                run.tasks.iter().any(|d| {
                    &d.plan.key == *key
                        && matches!(
                            d.status,
                            TaskStatus::Failed | TaskStatus::Cancelled | TaskStatus::Superseded
                        )
                })
            }) {
                return Some(Job::Failure(run.clone(),task.id,format!("Prerequisite {dependency} did not complete. Revise the dependency or cancel this assignment.")));
            }
        }
        for d in &run.deliveries {
            if d.status == DeliveryStatus::Dispatched
                && d.runtime_generation
                    .is_some_and(|g| self.settled.get(&d.target) == Some(&g))
            {
                return Some(Job::Acknowledge(
                    run.clone(),
                    d.target,
                    d.runtime_generation.unwrap(),
                ));
            }
        }
        for task in &run.tasks {
            let Some(a) = task.attempt() else {
                continue;
            };
            if matches!(task.status, TaskStatus::Cancelled | TaskStatus::Paused) {
                self.stop_runtime(a.child_agent_id, cx);
                continue;
            }
            let session = self.chats.read(cx).session(a.child_agent_id);
            let settled = self.settled.get(&a.child_agent_id).copied() == Some(a.generation);
            let pending = run
                .deliveries
                .iter()
                .any(|d| d.target == a.child_agent_id && d.status != DeliveryStatus::Acknowledged);
            if let Some(s) = session {
                if s.status == AgentChatStatus::Failed && task.status.occupies_slot() {
                    return Some(Job::Failure(
                        run.clone(),
                        task.id,
                        "Provider failed. Open the Bandmate conversation for details.".into(),
                    ));
                }
            }
            if task.status == TaskStatus::CompletionRequested
                && settled
                && !pending
                && self.chats.read(cx).safe_for_delegation(a.child_agent_id)
            {
                return Some(Job::Capture(run.clone(), task.id));
            }
            if task.status == TaskStatus::Running
                && settled
                && !pending
                && self.chats.read(cx).safe_for_delegation(a.child_agent_id)
            {
                return Some(Job::MissingReport(run.clone(), task.id));
            }
            if task.status == TaskStatus::Integrating && safe && !run.plan_mode {
                return Some(Job::Integrate(run.clone(), task.id));
            }
        }
        // Deliver questions and corrections before spending a safe boundary on
        // another workspace snapshot. Independent work can still run afterward.
        for d in &run.deliveries {
            if d.status != DeliveryStatus::Queued
                || !self.chats.read(cx).safe_for_delegation(d.target)
            {
                continue;
            }
            if d.target != parent.id
                && d.task_id.and_then(|id| run.task(id).ok()).is_some_and(|t| {
                    !matches!(
                        t.status,
                        TaskStatus::Running
                            | TaskStatus::WaitingForLead
                            | TaskStatus::CompletionRequested
                    )
                })
            {
                continue;
            }
            if d.target == parent.id && run.wait.is_some() && !run.wait_satisfied() {
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
            let agent = self.agents.read(cx).agent(d.target)?.clone();
            let ids = run
                .deliveries
                .iter()
                .filter(|other| other.target == d.target && other.status == DeliveryStatus::Queued)
                .map(|d| d.id)
                .collect();
            let mode = if run.plan_mode && agent.id == parent.id {
                AgentInteractionMode::Plan
            } else {
                AgentInteractionMode::Default
            };
            return Some(Job::Deliver(run.clone(), agent, ids, mode));
        }
        if safe {
            if let Some(id) = run.ready_tasks(total).first() {
                return Some(Job::Prepare(run.clone(), *id, parent.clone()));
            }
        }
        None
    }

    fn apply_effect(&mut self, run_id: Uuid, effect: Effect, cx: &mut Context<Self>) -> Result<()> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        if matches!(effect, Effect::None) {
            return Ok(());
        }
        if let Effect::Send(agent, _, _, _, ids) = &effect {
            let task_allowed = agent
                .delegation
                .as_ref()
                .and_then(|b| b.task_id.map(|id| (id, b.attempt_id)))
                .is_none_or(|(id, attempt)| {
                    run.task(id).is_ok_and(|t| {
                        matches!(
                            t.status,
                            TaskStatus::Running
                                | TaskStatus::WaitingForLead
                                | TaskStatus::CompletionRequested
                        ) && t.attempt().is_some_and(|a| Some(a.id) == attempt)
                    })
                });
            if !run.status.dispatchable() || !task_allowed {
                // This effect has not reached the provider. Preserve it as
                // queued input instead of inventing an uncertain execution.
                store.update_delegation(run_id, None, |r| {
                    for d in &mut r.deliveries {
                        if ids.contains(&d.id) && d.runtime_generation.is_none() {
                            d.status = DeliveryStatus::Queued;
                        }
                    }
                    Ok(())
                })?;
                return Ok(());
            }
        }
        ensure!(
            run.status.dispatchable() || run.status == RunStatus::Preparing,
            "This run stopped before dispatch. Work remains saved."
        );
        match effect {
            Effect::None => (),
            Effect::Adopt(agent) => {
                // Reconfiguration never substitutes a fresh provider session.
                if self.chats.read(cx).has_backend(agent.id) {
                    ensure!(
                        self.chats
                            .update(cx, |chats, cx| chats.retire_idle_backend(agent.id, cx)),
                        "The provider has not reached a safe reconfiguration boundary."
                    );
                }
                self.agents.update(cx, |records, cx| {
                    records.adopt_managed(agent, cx);
                    records.try_save_now()
                })?;
            }
            Effect::Send(agent, history, text, _, ids) => {
                let target = agent.id;
                // A lead can change mode while the background delivery is
                // loading. Apply the latest durable policy at dispatch time.
                let mode = if target == run.parent_agent_id && run.plan_mode {
                    AgentInteractionMode::Plan
                } else {
                    AgentInteractionMode::Default
                };
                if !self.chats.read(cx).safe_for_delegation(target) {
                    // No provider command has been sent. A queued user turn
                    // wins without turning a known non-delivery into uncertainty.
                    store.update_delegation(run_id, None, |r| {
                        for d in &mut r.deliveries {
                            if ids.contains(&d.id) && d.runtime_generation.is_none() {
                                d.status = DeliveryStatus::Queued;
                            }
                        }
                        Ok(())
                    })?;
                    return Ok(());
                }
                // Legacy records did not carry consultation scope. Reconfigure
                // only at this reserved idle boundary and preserve resume IDs.
                let policy_changed = self
                    .agents
                    .read(cx)
                    .agent(target)
                    .is_some_and(|stored| stored.delegation != agent.delegation);
                if policy_changed {
                    if self.chats.read(cx).has_backend(target) {
                        ensure!(
                            self.chats
                                .update(cx, |chats, cx| chats.retire_idle_backend(target, cx)),
                            "The bandmate has not reached a safe policy boundary."
                        );
                    }
                    self.agents.update(cx, |records, cx| {
                        records.adopt_managed(agent.clone(), cx);
                        records.try_save_now()
                    })?;
                }
                let agent = self
                    .agents
                    .read(cx)
                    .agent(target)
                    .cloned()
                    .context("The conversation was removed before dispatch.")?;
                self.settled.remove(&target);
                let generation = self.chats.update(cx, |chats, cx| {
                    super::chat_dispatch::managed_send(chats, agent, history, text, mode, cx)
                })?;
                self.agents
                    .update(cx, |a, cx| a.mark_started(target, None, cx));
                store.update_delegation(run_id, None, |r| {
                    for d in &mut r.deliveries {
                        if ids.contains(&d.id) {
                            d.runtime_generation = Some(generation);
                        }
                    }
                    for t in &mut r.tasks {
                        if let Some(a) = t.attempt_mut() {
                            if a.child_agent_id == target {
                                a.generation = generation;
                            }
                        }
                    }
                    Ok(())
                })?;
            }
        }
        Ok(())
    }

    pub fn cleanup_confirmed(
        &mut self,
        run_id: Uuid,
        paths: Vec<std::path::PathBuf>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let store = LocalStore::open_default()?;
        ensure!(
            store.delegation_cleanup_preview(run_id)? == paths,
            "Working-copy cleanup changed; review the paths again."
        );
        let run = store.load_delegation(run_id)?;
        let mut stops = Vec::new();
        for task in &run.tasks {
            if let Some(a) = task.attempt() {
                if let Some(signal) = self.chats.update(cx, |chats, cx| {
                    chats.stop_backend_for_lane_exit(a.child_agent_id, cx)
                }) {
                    stops.push(signal);
                }
                self.stop_runtime(a.child_agent_id, cx);
            }
        }
        cx.spawn(async move|this,cx|{
            let result=cx.background_executor().spawn(async move{
                let deadline=std::time::Instant::now()+Duration::from_secs(10);
                while stops.iter().any(|s|!s.is_stopped()) {
                    ensure!(std::time::Instant::now()<deadline,"A Bandmate process has not stopped. Its files were retained; try cleanup again after it exits.");
                    std::thread::sleep(Duration::from_millis(50));
                }
                store.cleanup_delegation_workspaces(run_id,&paths)?;store.load_delegations()
            }).await;
            let _=this.update(cx,|s,cx|{match result{Ok(runs)=>s.runs=runs,Err(e)=>s.error=Some(e.to_string())}cx.notify();});
        }).detach();
        Ok(())
    }

    /// UI-only durable Stop gate. Agent tools have no route to clear it.
    pub fn end_run(&mut self, run_id: Uuid, cx: &mut Context<Self>) -> Result<()> {
        self.pause(run_id, cx)?;
        let store = LocalStore::open_default()?;
        store.update_delegation(run_id, None, |r| {
            r.status = RunStatus::Cancelled;
            r.pause_reason = Some("Ended by user; conversations and files retained".into());
            for t in &mut r.tasks {
                if !t.status.terminal() {
                    t.status = TaskStatus::Cancelled;
                    t.reason = Some("User ended delegation".into());
                }
            }
            r.event(
                r.parent_agent_id,
                None,
                "cancelled",
                "User ended delegation; working copies retained",
            );
            Ok(())
        })?;
        let run = store.load_delegation(run_id)?;
        self.chats.update(cx, |chats, _| {
            chats.allow_managed_resume(run.parent_agent_id)
        });
        self.runs = store.load_delegations()?;
        cx.notify();
        Ok(())
    }
    pub fn pause(&mut self, run_id: Uuid, cx: &mut Context<Self>) -> Result<()> {
        let store = LocalStore::open_default()?;
        store.update_delegation(run_id, None, |r| {
            r.pause("Stopped by user", false);
            Ok(())
        })?;
        let run = store.load_delegation(run_id)?;
        self.stop_runtime(run.parent_agent_id, cx);
        self.stopped_parents.insert(run_id);
        for task in &run.tasks {
            if let Some(a) = task.attempt() {
                self.stop_runtime(a.child_agent_id, cx);
            }
        }
        self.runs = store.load_delegations()?;
        cx.notify();
        Ok(())
    }
    pub fn pause_task(
        &mut self,
        run_id: Uuid,
        task_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let store = LocalStore::open_default()?;
        let child=store.update_delegation(run_id,None,|r|{
            let t=r.task_mut(task_id)?;ensure!(!t.status.terminal(),"This Bandmate has already finished.");
            if t.status!=TaskStatus::Paused{t.paused_status=Some(t.status);t.status=TaskStatus::Paused;t.reason=Some("Stopped by user".into());}
            let child=t.attempt().map(|a|a.child_agent_id);
            for d in &mut r.deliveries{if Some(d.target)==child&&d.status==DeliveryStatus::Dispatched{d.status=DeliveryStatus::Uncertain;}}
            r.event(r.parent_agent_id,Some(task_id),"question","The user stopped this Bandmate. Other assignments may continue; this task needs explicit Resume.");
            r.queue(r.parent_agent_id,Some(task_id),format!("The user stopped task {task_id}. Continue independent work. Only the user can resume this Bandmate."));Ok(child)
        })?;
        if let Some(child) = child {
            self.stop_runtime(child, cx);
        }
        self.runs = store.load_delegations()?;
        cx.notify();
        Ok(())
    }
    pub fn resume_task(
        &mut self,
        run_id: Uuid,
        task_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        ensure!(run.status.dispatchable(), "Resume the parent task first.");
        let task = run.task(task_id)?;
        ensure!(
            task.status == TaskStatus::Paused,
            "This Bandmate is not paused."
        );
        let child = task.attempt().map(|a| a.child_agent_id);
        let mut received = Vec::new();
        if let Some(child) = child {
            let agent = self
                .agents
                .read(cx)
                .agent(child)
                .context("Bandmate chat is missing")?;
            validate_resume_session(agent, task.attempt().is_some_and(|a| a.generation > 0))?;
            let uncertain = run
                .deliveries
                .iter()
                .filter(|d| d.target == child && d.status == DeliveryStatus::Uncertain)
                .collect::<Vec<_>>();
            if !uncertain.is_empty() {
                let session = agent
                    .chat_session_id
                    .as_deref()
                    .or(agent.cli_session_id.as_deref())
                    .context(
                        "Provider session ID is missing. Inspect this task before retrying.",
                    )?;
                let history = ide_core::doc_assistant::read_chat_transcript_messages(
                    agent.provider,
                    agent.runtime_path(),
                    session,
                );
                for d in uncertain {
                    ensure!(history.iter().any(|m|m.role==ide_core::doc_assistant::DocAssistantRole::User&&m.text.contains(&format!("[Choro delivery {}]",d.id))),"A delivery's outcome is uncertain in provider history. Reconcile the saved conversation before resuming.");
                    received.push(d.id);
                }
            }
        }
        store.update_delegation(run_id,Some(run.revision),|r|{
            for d in &mut r.deliveries{if received.contains(&d.id){d.status=DeliveryStatus::Acknowledged;}}
            let t=r.task_mut(task_id)?;let previous=t.paused_status.take().unwrap_or(TaskStatus::Running);t.reason=None;t.status=previous;
            let continuation=matches!(previous,TaskStatus::Running|TaskStatus::CompletionRequested);
            if continuation{t.revision+=1;t.status=TaskStatus::Running;if let Some(a)=t.attempt_mut(){a.result=None;a.result_revision=None;a.report_requested=false;}}
            let revision=t.revision;if let Some(child)=child.filter(|_|continuation){r.queue(child,Some(task_id),format!("User resumed this Bandmate. Task revision is {revision}. Reconcile existing work, continue the remaining assignment, and report completion. Do not repeat external side effects."));}Ok(())
        })?;
        if let Some(child) = child {
            self.chats
                .update(cx, |chats, _| chats.allow_managed_resume(child));
        }
        self.runs = store.load_delegations()?;
        cx.notify();
        Ok(())
    }
    pub fn resume(&mut self, run_id: Uuid, cx: &mut Context<Self>) -> Result<()> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        let records = self.agents.read(cx).all_records();
        let parent = records
            .iter()
            .find(|a| a.id == run.parent_agent_id)
            .context("Lead record is missing")?;
        validate_resume_session(parent, parent.started_at.is_some())?;
        for task in &run.tasks {
            if let Some(attempt) = task.attempt().filter(|a| a.generation > 0) {
                if matches!(
                    task.status,
                    TaskStatus::Running
                        | TaskStatus::CompletionRequested
                        | TaskStatus::NeedsUser
                        | TaskStatus::WaitingForLead
                ) {
                    let agent = records.iter().find(|a| a.id == attempt.child_agent_id)
                        .context("A previously launched Bandmate record is missing. Restore its saved session before resuming.")?;
                    validate_resume_session(agent, true)?;
                }
            }
        }
        let mut received = Vec::new();
        for delivery in run
            .deliveries
            .iter()
            .filter(|d| d.status == DeliveryStatus::Uncertain)
        {
            let agent = records.iter().find(|a| a.id == delivery.target).context(
                "A delegated chat record is missing. Its assignment is retained for recovery.",
            )?;
            let session=agent.chat_session_id.as_deref().or(agent.cli_session_id.as_deref()).context("Provider session ID is missing. Open this Bandmate to inspect its saved work; automatic retry is unsafe.")?;
            let path = ide_core::agents::chat_transcript_path(
                agent.provider,
                agent.runtime_path(),
                session,
            )
            .context(
                "Provider history is unavailable. Restore the saved session before resuming.",
            )?;
            ensure!(path.is_file(),"The saved provider session no longer exists. Files and conversations are preserved.");
            let messages = ide_core::doc_assistant::read_chat_transcript_messages(
                agent.provider,
                agent.runtime_path(),
                session,
            );
            let marker = format!("[Choro delivery {}]", delivery.id);
            ensure!(messages.iter().any(|m|m.role==ide_core::doc_assistant::DocAssistantRole::User && m.text.contains(&marker)),"Delivery {} has an uncertain outcome in provider history. Open the Bandmate and reconcile it before retrying; Choro will not repeat it automatically.",delivery.id);
            received.push(delivery.id);
        }
        // Reconcile journals without touching files. Apply only after explicit
        // Resume and the next settled parent boundary.
        let mut journals = HashMap::new();
        for task in &run.tasks {
            validate_working_scope(&store, &run, task)?;
            if let Some(a) = task.attempt() {
                if task.status == TaskStatus::Preparing && a.snapshot.is_none() {
                    continue;
                }
                ensure!(
                    a.workspace.is_dir(),
                    "Bandmate working copy is missing: {}",
                    a.workspace.display()
                );
                if let Some(base) = &a.snapshot {
                    ensure!(
                        base.source.is_dir() && base.storage.is_dir(),
                        "Task snapshot or source repository is missing."
                    );
                }
            }
            if let Some(op) = &task.integration {
                if op.journal_path().is_file() {
                    let journal: workspace::IntegrationOperation =
                        serde_json::from_slice(&std::fs::read(op.journal_path())?)?;
                    ensure!(
                        journal.id == op.id && journal.source == op.source,
                        "Integration journal identity changed."
                    );
                    journals.insert(task.id, journal);
                }
            }
        }
        let parent = records
            .iter()
            .find(|a| a.id == run.parent_agent_id)
            .context("Lead record is missing")?;
        for task in &run.tasks {
            if let Some(a) = task.attempt().filter(|a| {
                a.snapshot.is_some() && !records.iter().any(|record| record.id == a.child_agent_id)
            }) {
                ensure!(a.generation==0,"A previously launched Bandmate record is missing; restoring its session is required.");
                let p = &task.expert.profile;
                let mut child = AgentRecord::new(
                    parent.project_id,
                    parent.project_path.clone(),
                    format!("{} · {}", p.name, task.plan.goal),
                    task.plan.brief.clone(),
                    p.provider,
                    p.model,
                    p.effort,
                    parent.access_mode,
                );
                child.id = a.child_agent_id;
                child.runtime = AgentRuntimeKind::Chat;
                child.repository_path = parent.repository_path.clone();
                child.expert_snapshot = Some(task.expert.clone());
                child.delegation = Some(DelegationBinding {
                    run_id,
                    parent_agent_id: parent.id,
                    task_id: Some(task.id),
                    attempt_id: Some(a.id),
                    workspace: Some(a.workspace.clone()),
                    task_kind: Some(task.plan.kind),
                });
                self.agents.update(cx, |records, cx| {
                    records.adopt_managed(child, cx);
                    records.try_save_now()
                })?;
            }
        }
        store.update_delegation(run_id,Some(run.revision),|r| {
            for d in &mut r.deliveries {if received.contains(&d.id){d.status=DeliveryStatus::Acknowledged;}}
            r.resume()?;
            let mut continuations=Vec::new();
            for task in &mut r.tasks {
                if task.status==TaskStatus::Paused{continue;}
                if task.status==TaskStatus::Preparing && task.attempt().is_some_and(|a|a.snapshot.is_none()){task.status=TaskStatus::Queued;continue;}
                if let Some(journal)=journals.remove(&task.id) {
                    if journal.status==workspace::IntegrationStatus::Applied {task.status=TaskStatus::Integrated;}
                    else if matches!(journal.status,workspace::IntegrationStatus::Applying|workspace::IntegrationStatus::Prepared|workspace::IntegrationStatus::NeedsAttention){task.status=TaskStatus::Integrating;}
                    task.integration=Some(journal);
                }
                if matches!(task.status,TaskStatus::Running|TaskStatus::CompletionRequested|TaskStatus::NeedsUser) {
                    if task.integration.is_some(){continue;}
                    task.revision+=1;task.status=TaskStatus::Running;
                    let revision=task.revision;
                    if let Some(a)=task.attempt_mut(){a.result=None;a.result_revision=None;a.report_requested=false;continuations.push((a.child_agent_id,task.id,revision));}
                }
            }
            for (child,task,revision) in continuations {r.queue(child,Some(task),format!("The user resumed this task. Continue from the existing session and working copy; do not repeat external side effects. Read current state for task {task}. Task revision is now {revision}. Reconcile what already happened, finish remaining work, and report the verified result."));}
            Ok(())
        })?;
        let run = store.load_delegation(run_id)?;
        self.chats.update(cx, |chats, _| {
            chats.allow_managed_resume(run.parent_agent_id);
            for task in &run.tasks {
                if task.status != TaskStatus::Paused {
                    if let Some(a) = task.attempt() {
                        chats.allow_managed_resume(a.child_agent_id);
                    }
                }
            }
        });
        self.stopped_parents.remove(&run_id);
        self.runs = store.load_delegations()?;
        cx.notify();
        Ok(())
    }
}

/// Historical runs must never retire or detach the lead of a newer run.
fn can_restore_lead_policy(run: &DelegationRun, parent: &AgentRecord) -> bool {
    run.status.terminal()
        && parent.id == run.parent_agent_id
        && parent
            .delegation
            .as_ref()
            .is_some_and(|binding| binding.run_id == run.id && binding.task_id.is_none())
}

/// Stop the lead once on entering a stopped run, not on every coordinator
/// poll. Child runtimes and automatic deliveries remain gated by run status.
fn parent_stop_required(stopped: &mut HashSet<Uuid>, run: &DelegationRun) -> bool {
    if run.status.stopped() {
        stopped.insert(run.id)
    } else {
        stopped.remove(&run.id);
        false
    }
}

fn validate_resume_session(agent: &AgentRecord, execution_started: bool) -> Result<()> {
    let session = agent
        .chat_session_id
        .as_deref()
        .or(agent.cli_session_id.as_deref());
    if !execution_started && session.is_none() {
        return Ok(());
    }
    let session = session.context("Cannot resume: the saved provider session ID is missing. Files and conversations are preserved; restore the original session before continuing.")?;
    let path = ide_core::agents::chat_transcript_path(agent.provider, agent.runtime_path(), session)
        .context("Cannot resume: the saved provider history is unavailable. Restore the original session before continuing.")?;
    ensure!(path.is_file(), "Cannot resume: the saved provider session no longer exists. Files and conversations are preserved.");
    Ok(())
}

fn integration_summary(
    op: &workspace::IntegrationOperation,
    root: &std::path::Path,
) -> Result<super::agent_chat::ChangedFilesSummary> {
    let mut files = Vec::new();
    for change in &op.changes {
        let before = workspace::bounded_text(&op.storage, change.before.as_ref())?;
        let after = workspace::bounded_text(&op.storage, change.after.as_ref())?;
        let (added, deleted) = before
            .as_deref()
            .zip(after.as_deref())
            .and_then(|(b, e)| super::agent_chat::bounded_line_diff_counts(b, e))
            .unwrap_or((0, 0));
        let absolute = op.source.join(&change.path);
        let path = absolute
            .strip_prefix(root)
            .unwrap_or(&absolute)
            .to_path_buf();
        files.push(
            super::agent_chat::FileChangeStat::new(path, added, deleted)
                .with_content_hashes(
                    change.before.as_ref().map(|f| f.hash.clone()),
                    change.after.as_ref().map(|f| f.hash.clone()),
                )
                .with_content_projection(before, after),
        );
    }
    Ok(super::agent_chat::ChangedFilesSummary::attributed(
        format!("delegation-integration-{}", op.id),
        files,
        vec![],
    ))
}

fn validate_working_scope(
    store: &LocalStore,
    run: &DelegationRun,
    task: &DelegationTask,
) -> Result<()> {
    let agents = store.load_agents()?;
    let parent = agents
        .iter()
        .find(|a| a.id == run.parent_agent_id)
        .context("Lead chat is missing")?;
    let repository = task.plan.repository.canonicalize()?;
    let allowed = parent.runtime_path().canonicalize()?;
    ensure!(
        repository.starts_with(&allowed),
        "Task repository {} is outside the lead's current working scope {}.",
        repository.display(),
        allowed.display()
    );
    let root = store
        .app_data_dir()
        .join("delegation")
        .join(run.id.to_string());
    if let Some(a) = task.attempt() {
        ensure!(
            a.workspace == root.join(format!("attempt-{}", a.id)),
            "Bandmate working-copy identity changed."
        );
        if a.workspace.exists() {
            ensure!(
                !std::fs::symlink_metadata(&a.workspace)?.is_symlink(),
                "Bandmate working copy cannot be a symlink."
            );
        }
        if let Some(base) = &a.snapshot {
            ensure!(
                base.source == repository && base.storage == root,
                "Snapshot source or storage identity changed."
            );
        }
        if let Some(result) = &a.completed_snapshot {
            ensure!(result.storage == root, "Result storage identity changed.");
        }
    }
    if let Some(op) = &task.integration {
        ensure!(
            op.source == repository
                && op.storage == root
                && op.resolution_dir == root.join(format!("integration-{}", op.id)),
            "Integration scope changed. Restore the recorded repository before resuming."
        );
    }
    Ok(())
}

fn execute(job: Job, owner: Uuid) -> Result<Effect> {
    let store = LocalStore::open_default()?;
    execute_in_store(&store, job, managed::preflight, Some(owner))
}
fn execute_in_store(
    store: &LocalStore,
    job: Job,
    preflight: fn(ide_core::AgentKind) -> Result<()>,
    lease: Option<Uuid>,
) -> Result<Effect> {
    if let Some(owner) = lease {
        ensure!(
            store.claim_delegation_coordinator(owner)?,
            "Coordinator lease was lost. Work is preserved."
        );
    }
    let id = job.run().id;
    let latest = store.load_delegation(id)?;
    if latest.revision != job.run().revision {
        return Ok(Effect::None);
    }
    ensure!(
        latest.status.dispatchable() || latest.status == RunStatus::Preparing,
        "The run is paused."
    );
    match job {
        Job::Configure(run, mut parent) => {
            preflight(parent.provider)?;
            for task in &run.tasks {
                preflight(task.expert.profile.provider)?;
            }
            parent.delegation = Some(DelegationBinding {
                run_id: id,
                parent_agent_id: parent.id,
                task_id: None,
                attempt_id: None,
                workspace: None,
                task_kind: None,
            });
            store.update_delegation(id,Some(run.revision),|r| { r.status=RunStatus::Active; r.queue(r.parent_agent_id,None,format!("Managed Bandmates are ready. Run {id}. Continue your share of the task, coordinate contracts and integrate results. Use delegation_wait and end the turn when there is no independent work.")); Ok(()) })?;
            Ok(Effect::Adopt(parent))
        }
        Job::Prepare(run, task_id, parent) => {
            let task = run.task(task_id)?.clone();
            let expert = &task.expert.profile;
            validate_working_scope(store, &run, &task)?;
            preflight(expert.provider)?;
            // Launch from the immutable package, including after import or a
            // changed/uninstalled source skill. The original path is provenance.
            task.expert
                .runtime_instructions(&store.root().join("expert-skill-cache"))?;
            let attempt_id = Uuid::new_v4();
            let root = store.app_data_dir().join("delegation").join(id.to_string());
            let workspace_path = root.join(format!("attempt-{attempt_id}"));
            let child_id = task
                .attempt()
                .map_or_else(Uuid::new_v4, |a| a.child_agent_id);
            store.update_delegation(id, Some(run.revision), |r| {
                let t = r.task_mut(task_id)?;
                t.status = TaskStatus::Preparing;
                t.attempts.push(DelegationAttempt {
                    id: attempt_id,
                    child_agent_id: child_id,
                    generation: 0,
                    workspace: workspace_path.clone(),
                    snapshot: None,
                    completed_snapshot: None,
                    archive_snapshot: None,
                    working_copy_cleaned: false,
                    result: None,
                    result_revision: None,
                    report_requested: false,
                    session_id: None,
                    progress: "Capturing current work".into(),
                    usage: None,
                });
                Ok(())
            })?;
            let snapshot = workspace::capture(&task.plan.repository, &root)?;
            workspace::materialize(&snapshot, &workspace_path)?;
            let mut child = if let Some(previous) =
                store.load_agents()?.into_iter().find(|a| a.id == child_id)
            {
                previous
            } else {
                AgentRecord::new(
                    parent.project_id,
                    parent.project_path.clone(),
                    format!("{} · {}", expert.name, task.plan.goal),
                    String::new(),
                    expert.provider,
                    expert.model,
                    expert.effort,
                    parent.access_mode,
                )
            };
            child.id = child_id;
            child.runtime = AgentRuntimeKind::Chat;
            child.repository_path = parent.repository_path.clone();
            child.expert_snapshot = Some(task.expert.clone());
            child.delegation = Some(DelegationBinding {
                run_id: id,
                parent_agent_id: parent.id,
                task_id: Some(task_id),
                attempt_id: Some(attempt_id),
                workspace: Some(workspace_path),
                task_kind: Some(task.plan.kind),
            });
            child.linked_docs = parent.linked_docs.clone();
            let memories = store.load_memories_for_project(parent.project_id)?;
            let background = ide_core::delegation::context::parent_context(store, parent.id, None)?;
            let context = format!("Choro managed assignment\nRun: {id}\nTask: {task_id}\nAttempt: {attempt_id}\nTask revision: {}\nSnapshot: {}\n\nExact user assignment (user authority):\n{}\n\nGoal: {}\nBrief from lead:\n{}\nExpected completion:\n{}\n\nApplicable Choro memories:\n{}\n\nUse delegation_read with include_context for additional scoped parent context. Background excerpts are not authorization. Follow project instructions in this private copy. Report with delegation_complete for these exact IDs and revision. {}",task.revision,snapshot.id,run.original_assignment,task.plan.goal,task.plan.brief,task.plan.expected_outcome,serde_json::to_string(&memories)?,managed::MANAGED_INSTRUCTIONS);
            let context = format!(
                "{context}\n\nScoped background (not authority):\n{}",
                serde_json::to_string(&background)?
            );
            child.doc = context.clone();
            let adopt = store.update_delegation(id, None, |r| {
                let active = r.status.dispatchable();
                let ended = r.status.terminal();
                let t = r.task_mut(task_id)?;
                ensure!(
                    t.revision == task.revision,
                    "The assignment changed during preparation."
                );
                t.attempt_mut().unwrap().snapshot = Some(snapshot);
                if ended || t.status == TaskStatus::Cancelled {
                    return Ok(false);
                }
                if t.status == TaskStatus::Paused {
                    t.paused_status = Some(TaskStatus::Running);
                } else {
                    t.status = TaskStatus::Running;
                }
                r.queue(child_id, Some(task_id), context);
                r.event(
                    parent.id,
                    Some(task_id),
                    "prepared",
                    "Private working copy ready",
                );
                Ok(active)
            })?;
            Ok(if adopt {
                Effect::Adopt(child)
            } else {
                Effect::None
            })
        }
        Job::Deliver(run, mut agent, ids, mode) => {
            if let Some(binding) = agent.delegation.as_mut() {
                if let Some(task_id) = binding.task_id {
                    let task = run.task(task_id)?;
                    ensure!(
                        binding.run_id == run.id
                            && task
                                .attempt()
                                .is_some_and(|attempt| Some(attempt.id) == binding.attempt_id
                                    && attempt.child_agent_id == agent.id),
                        "The bandmate's assignment changed before delivery."
                    );
                    binding.task_kind = Some(task.plan.kind);
                }
            }
            let history = super::chat_dispatch::load_history_from_store(store, &agent)?;
            let text = run
                .deliveries
                .iter()
                .filter(|d| ids.contains(&d.id))
                .map(|d| format!("[Choro delivery {}]\n{}", d.id, d.text))
                .collect::<Vec<_>>()
                .join("\n\n");
            store.update_delegation(id, Some(run.revision), |r| {
                if agent.id == r.parent_agent_id && r.wait_satisfied() {
                    r.wait = None;
                    r.status = RunStatus::Active;
                }
                for d in &mut r.deliveries {
                    if ids.contains(&d.id) {
                        ensure!(
                            d.status == DeliveryStatus::Queued,
                            "Delivery already dispatched."
                        );
                        d.status = DeliveryStatus::Dispatched;
                    }
                }
                Ok(())
            })?;
            let mode = managed::interaction_mode(&agent, mode);
            Ok(Effect::Send(agent, history, text, mode, ids))
        }
        Job::Acknowledge(run, target, generation) => {
            store.update_delegation(id, Some(run.revision), |r| {
                for d in &mut r.deliveries {
                    if d.target == target
                        && d.runtime_generation == Some(generation)
                        && d.status == DeliveryStatus::Dispatched
                    {
                        d.status = DeliveryStatus::Acknowledged;
                    }
                }
                Ok(())
            })?;
            Ok(Effect::None)
        }
        Job::Capture(run, task_id) => {
            let task = run.task(task_id)?;
            let a = task.attempt().context("Missing attempt")?;
            validate_working_scope(store, &run, task)?;
            let snapshot = workspace::capture(
                &a.workspace,
                &a.snapshot.as_ref().context("Missing baseline")?.storage,
            )?;
            // Freeze E independently from later corrections to the Expert chat.
            workspace::materialize(
                &snapshot,
                &snapshot.storage.join(format!("result-{}", snapshot.id)),
            )?;
            store.update_delegation(id,None,|r| {
                if !r.status.dispatchable(){return Ok(());}
                let t=r.task(task_id)?;
                if t.revision!=task.revision||t.status!=TaskStatus::CompletionRequested||t.attempt().is_none_or(|current|current.id!=a.id){return Ok(());}
                let t=r.task_mut(task_id)?; t.attempt_mut().unwrap().completed_snapshot=Some(snapshot); t.status=TaskStatus::ResultReady;
                let summary=t.attempt().and_then(|a|a.result.as_ref()).context("Missing completion report")?.summary.clone();
                r.event(r.parent_agent_id,Some(task_id),"result",summary.clone());
                r.queue(r.parent_agent_id,Some(task_id),format!("Bandmate result ready for task {task_id}: {summary}. Read the report and request delegation_integrate, or accept a consultation. Verify the combined outcome.")); Ok(())
            })?;
            Ok(Effect::None)
        }
        Job::MissingReport(run, task_id) => {
            store.update_delegation(id,Some(run.revision),|r| {
                let t=r.task_mut(task_id)?; let a=t.attempt_mut().context("Missing attempt")?;
                let child=a.child_agent_id;
                if a.report_requested { t.status=TaskStatus::Failed; t.reason=Some("Bandmate ended without a structured report after one reminder.".into()); r.event(child,Some(task_id),"failed","Bandmate did not report a result"); r.queue(r.parent_agent_id,Some(task_id),format!("Task {task_id} ended without a completion report after one reminder. Inspect its conversation; do not infer success.")); }
                else { a.report_requested=true; r.queue(child,Some(task_id),format!("Your turn ended without a result. If finished, read run {id} and call delegation_complete with your exact current attempt and task revision. If blocked, send delegation_message with blocking=true. Report only; do not restart implementation.")); } Ok(())
            })?;
            Ok(Effect::None)
        }
        Job::Failure(run, task_id, reason) => {
            store.update_delegation(id, Some(run.revision), |r| {
                let t = r.task_mut(task_id)?;
                t.status = TaskStatus::Failed;
                t.reason = Some(reason.clone());
                r.event(r.parent_agent_id, Some(task_id), "failed", reason.clone());
                r.queue(r.parent_agent_id, Some(task_id), reason);
                Ok(())
            })?;
            Ok(Effect::None)
        }
        Job::Integrate(run, task_id) => {
            let task = run.task(task_id)?;
            let attempt = task.attempt().context("Missing attempt")?;
            validate_working_scope(store, &run, task)?;
            ensure!(
                attempt.result_revision == Some(task.revision),
                "Cannot integrate an outdated result."
            );
            let baseline = attempt.snapshot.as_ref().context("Missing baseline")?;
            let completed = attempt
                .completed_snapshot
                .as_ref()
                .context("Result is not captured")?;
            let mut op = if let Some(existing) = &task.integration {
                existing.clone()
            } else {
                workspace::prepare_integration(
                    baseline,
                    &completed.storage.join(format!("result-{}", completed.id)),
                )?
            };
            let reprepare = op.status == workspace::IntegrationStatus::NeedsAttention
                || !op.matches_current_source()?;
            if reprepare {
                op = op.reprepare_remaining(
                    baseline,
                    &completed.storage.join(format!("result-{}", completed.id)),
                )?;
            }
            if op.status == workspace::IntegrationStatus::Conflict
                && !reprepare
                && task.reason.as_deref() == Some("resolutions_submitted")
            {
                op.accept_resolutions()?;
            }
            let needs_attention = op.status == workspace::IntegrationStatus::Conflict
                || op.needs_deletion_confirmation() && task.deletion_confirmation != Some(op.id);
            let accepted = store.update_delegation(id, None, |r| {
                if !r.status.dispatchable() || r.plan_mode {
                    return Ok(false);
                }
                let current = r.task(task_id)?;
                if current.revision != task.revision
                    || current.status != TaskStatus::Integrating
                    || current.attempt().is_none_or(|a| a.id != attempt.id)
                {
                    return Ok(false);
                }
                r.task_mut(task_id)?.integration = Some(op.clone());
                Ok(true)
            })?;
            if !accepted {
                return Ok(Effect::None);
            }
            if needs_attention {
                store.update_delegation(id,None,|r| {
                    let t=r.task_mut(task_id)?;
                    t.status=if op.status==workspace::IntegrationStatus::Conflict {TaskStatus::ResultReady} else {TaskStatus::NeedsUser};
                    let reason=if op.status==workspace::IntegrationStatus::Conflict {format!("Resolve conflicts {:?} in {}. Base and result manifests are available through delegation_read. Submit resolutions_ready when checked.",op.conflicts,op.resolution_dir.display())} else {"Integration proposes file deletions. User confirmation is required in Choro.".into()};
                    t.reason=Some(reason.clone());r.event(r.parent_agent_id,Some(task_id),"question",reason.clone());r.queue(r.parent_agent_id,Some(task_id),reason);Ok(())
                })?;
                return Ok(Effect::None);
            }
            op.apply_guarded(task.deletion_confirmation == Some(op.id), || {
                if let Some(owner) = lease {
                    ensure!(
                        store.claim_delegation_coordinator(owner)?,
                        "Coordinator lease was lost during integration."
                    );
                }
                let r = store.load_delegation(id)?;
                ensure!(
                    r.status.dispatchable()
                        && !r.plan_mode
                        && r.task(task_id)?.status == TaskStatus::Integrating
                        && r.task(task_id)?.revision == task.revision,
                    "Integration paused. Reconcile the preserved journal before continuing."
                );
                Ok(())
            })?;
            store.update_delegation(id,None,|r| {
                let active=r.status.dispatchable();
                let t=r.task_mut(task_id)?;t.integration=Some(op);
                if active && t.status==TaskStatus::Integrating {t.status=TaskStatus::Integrated;t.reason=None;}
                r.event(r.parent_agent_id,Some(task_id),"integrated","Bandmate changes integrated; verification is still required");r.queue(r.parent_agent_id,Some(task_id),format!("Task {task_id} changes are integrated. Source index is unchanged. Verify the combined behavior before delegation_finish."));Ok(())
            })?;
            Ok(Effect::None)
        }
    }
}
