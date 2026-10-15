//! Application-owned orchestration. Views observe this entity; MCP writes the
//! durable commands it consumes. No selected-chat state drives scheduling.
pub mod display;
mod preparation;
#[cfg(test)]
mod tests;
use super::agent_chat::protocol::managed;
use super::agent_chat::{AgentChatEvent, AgentChatStatus, AgentInteractionMode};
use super::{AgentChatState, AgentRecords, TerminalManager};
use anyhow::{ensure, Context as _, Result};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Global};
use sha2::{Digest, Sha256};
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
    wake: async_channel::Sender<()>,
    reload_epoch: u64,
    snapshot: Vec<RunStamp>,
    /// Slow work never owns the refresh/scheduling loop. One job per task;
    /// durable task state still decides whether any result may be dispatched.
    in_flight: HashMap<(Uuid, Uuid), Job>,
}

#[derive(Clone, PartialEq, Eq)]
struct RunStamp { id: Uuid, parent: Uuid, digest: [u8; 32] }

fn run_stamps(runs: &[DelegationRun]) -> Result<Vec<RunStamp>> {
    runs.iter().map(|run| Ok(RunStamp { id: run.id, parent: run.parent_agent_id,
        digest: Sha256::digest(serde_json::to_vec(run)?).into() })).collect()
}

pub(crate) struct DelegationChanged {
    pub parents: Vec<Uuid>,
    pub error_changed: bool,
}
impl EventEmitter<DelegationChanged> for DelegationCoordinator {}

#[derive(Clone)]
enum Job {
    Configure(DelegationRun, AgentRecord),
    Provision(DelegationRun, Uuid, AgentRecord),
    BeginPrepare(DelegationRun, Uuid, AgentRecord),
    Prepare(DelegationRun, Uuid, AgentRecord),
    Deliver(DelegationRun, AgentRecord, Vec<Uuid>, AgentInteractionMode),
    Finalize(DelegationRun, Uuid),
    Acknowledge(DelegationRun, Uuid, u64),
    MissingReport(DelegationRun, Uuid),
    Failure(DelegationRun, Uuid, String),
}
impl Job {
    fn task_id(&self) -> Option<Uuid> {
        match self {
            Self::Provision(_, id, _)
            | Self::BeginPrepare(_, id, _)
            | Self::Prepare(_, id, _)
            | Self::Finalize(_, id)
            | Self::MissingReport(_, id)
            | Self::Failure(_, id, _) => Some(*id),
            Self::Deliver(_, a, _, _) => a.delegation.as_ref().and_then(|b| b.task_id),
            _ => None,
        }
    }
    fn run(&self) -> &DelegationRun {
        match self {
            Self::Configure(r, _)
            | Self::Provision(r, _, _)
            | Self::BeginPrepare(r, _, _)
            | Self::Prepare(r, _, _)
            | Self::Deliver(r, _, _, _)
            | Self::Finalize(r, _)
            | Self::Acknowledge(r, _, _)
            | Self::MissingReport(r, _)
            | Self::Failure(r, _, _) => r,
        }
    }
    fn reserved_agent(&self) -> Option<Uuid> {
        match self {
            Self::Configure(r, _) => {
                Some(r.parent_agent_id)
            }
            Self::Deliver(_, a, _, _) => Some(a.id),
            Self::Finalize(r, t) => r.task(*t).ok()?.attempt().map(|a| a.child_agent_id),
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
        let (wake, wake_receiver) = async_channel::bounded(1);
        let entity = cx.new(|cx| {
            cx.subscribe(&chats, |this: &mut Self, _, event, cx| {
                if let AgentChatEvent::WorkFinished { agent_id } = event {
                    this.settled
                        .insert(*agent_id, this.chats.read(cx).backend_generation(*agent_id));
                }
                if matches!(event, AgentChatEvent::WorkFinished { .. })
                    || matches!(event, AgentChatEvent::SessionChanged(change) if change.categories.navigation || change.categories.identity) {
                    this.request_reload();
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
                wake,
                reload_epoch: 0,
                snapshot: Vec::new(),
                in_flight: HashMap::new(),
            }
        });
        cx.set_global(DelegationHandle(entity.clone()));
        let owner = Uuid::new_v4();
        entity.update(cx, |_, cx| {
            // Renewal is independent of provider preparation and report finalization.
            cx.spawn(async move |this,cx| loop {
                cx.background_executor().timer(Duration::from_secs(4)).await;
                if this.upgrade().is_none(){break;}
                let _=cx.background_executor().spawn(async move {LocalStore::open_default().and_then(|s|s.claim_delegation_coordinator(owner))}).await;
            }).detach();
            cx.spawn(async move |this, cx| {
                let mut recovered = false;
                loop {
                    // A bounded wake coalesces reload hints, never business events.
                    // The existing poll remains a backstop for lost IPC messages.
                    let timer = cx.background_executor().timer(Duration::from_millis(750));
                    let _ = futures_util::future::select(Box::pin(wake_receiver.recv()), Box::pin(timer)).await;
                    let Ok(epoch) = this.update(cx, |s, _| s.reload_epoch) else { break; };
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
                        let runs = store.load_delegations()?;
                        // Include telemetry and reports, which can change without a run revision.
                        let snapshot = run_stamps(&runs)?;
                        Ok::<_, anyhow::Error>((runs, snapshot))
                    }).await;
                    let (runs, snapshot) = match loaded {
                        Ok(result) => { recovered = true; result },
                        Err(error) => {
                            if this.update(cx, |s,cx| {
                                s.accept_error(epoch, error.to_string(), cx);
                            }).is_err() { break; }
                            continue;
                        },
                    };
                    let job = match this.update(cx, |s,cx| {
                        if !s.accept_snapshot(epoch, runs, snapshot, cx) { return None; }
                        // Scheduling proceeds even when presentation is unchanged.
                        s.next_job(cx)
                    }) { Ok(job) => job, Err(_) => break };
                    let Some(job) = job else { continue; };
                    if matches!(job, Job::Prepare(..) | Job::Finalize(..)) {
                        let key = (job.run().id, job.task_id().expect("task job"));
                        if this.update(cx, |s, _| { s.in_flight.insert(key, job.clone()); }).is_err() { break; }
                        let weak = this.clone();
                        cx.spawn(async move |cx| {
                            let work = job.clone();
                            let result = cx.background_executor().spawn(async move {
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| execute(work, owner)))
                                    .unwrap_or_else(|_| Err(anyhow::anyhow!("Bandmate preparation stopped unexpectedly. Retry the assignment.")))
                            }).await;
                            let _ = weak.update(cx, |s, cx| {
                                s.in_flight.remove(&key);
                                s.finish_job(job, result, cx);
                            });
                        }).detach();
                        continue;
                    }
                    let work = job.clone();
                    let result = cx.background_executor().spawn(async move { execute(work,owner) }).await;
                    if this.update(cx, |s,cx| s.finish_job(job, result, cx)).is_err() { break; }
                }
            }).detach();
        });
        entity
    }

    fn accept_snapshot(&mut self, epoch: u64, runs: Vec<DelegationRun>, snapshot: Vec<RunStamp>, cx: &mut Context<Self>) -> bool {
        if self.reload_epoch != epoch { let _ = self.wake.try_send(()); return false; }
        let error_changed = self.error.is_some();
        let changed = self.snapshot != snapshot || error_changed;
        let parents = if changed {
            let old_by_id = self.snapshot.iter().map(|stamp| (stamp.id, stamp)).collect::<HashMap<_, _>>();
            let new_by_id = snapshot.iter().map(|stamp| (stamp.id, stamp)).collect::<HashMap<_, _>>();
            self.snapshot.iter().filter(|old| new_by_id.get(&old.id).copied() != Some(*old))
                .chain(snapshot.iter().filter(|new| old_by_id.get(&new.id).copied() != Some(*new)))
                .map(|stamp| stamp.parent).collect::<HashSet<_>>().into_iter().collect()
        } else { vec![] };
        self.runs = runs;
        self.snapshot = snapshot;
        self.error = None;
        if changed { cx.emit(DelegationChanged { parents, error_changed }); cx.notify(); }
        true
    }

    fn finish_job(&mut self, job: Job, result: Result<Effect>, cx: &mut Context<Self>) {
        if let Some(id) = job.reserved_agent() {
            self.chats.update(cx, |chats, _| { chats.delegation_reservations.remove(&id); });
        }
        let result = result.and_then(|effect| self.apply_effect(job.run().id, effect, cx));
        if let Err(error) = result {
            if let Err(error) = LocalStore::open_default()
                .and_then(|store| preparation::record_job_failure(&store, &job, &error.to_string())) {
                self.error = Some(error.to_string());
                cx.emit(DelegationChanged { parents: vec![job.run().parent_agent_id], error_changed: true });
                cx.notify();
            }
        }
        if let Some(id) = job.reserved_agent() {
            self.chats.update(cx, |chats, cx| chats.release_delegation_reservation(id, cx));
        }
        self.request_reload();
    }

    fn accept_error(&mut self, epoch: u64, error: String, cx: &mut Context<Self>) {
        if self.reload_epoch != epoch { return; }
        let error = Some(error);
        if self.error != error { self.error = error; cx.emit(DelegationChanged { parents: vec![], error_changed: true }); cx.notify(); }
    }

    fn notify_run_changed(&mut self, run_id: Uuid, cx: &mut Context<Self>) {
        self.request_reload();
        let parents = self.runs.iter().filter(|run| run.id == run_id).map(|run| run.parent_agent_id).collect();
        cx.emit(DelegationChanged { parents, error_changed: false });
        cx.notify();
    }

    /// A reload hint invalidates in-flight snapshots; storage remains authoritative.
    pub(crate) fn request_reload(&mut self) {
        self.reload_epoch = self.reload_epoch.wrapping_add(1);
        let _ = self.wake.try_send(());
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
                            chats.publish_change(parent.id, super::agent_chat::ChatChangeCategories::CONVERSATION, cx);
                        }
                    }
                });

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
            // A conversation exists even for queued, held, failed or stopped
            // assignments. Creating its record does not launch a provider.
            if let Some(task) = run.tasks.iter().filter(|_| !run.status.terminal()).find(|task| {
                task.attempt().is_none_or(|a| self.agents.read(cx).agent(a.child_agent_id).is_none())
            }) {
                return Some(Job::Provision(run.clone(), task.id, parent.clone()));
            }
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
                for task in &run.tasks {
                    if let Some(attempt) = task.attempt() { self.stop_runtime(attempt.child_agent_id, cx); }
                }
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
                (safe && !run.tasks.is_empty() && !self.in_flight.keys().any(|(id, _)| *id == run.id))
                    .then(|| Job::Configure(run.clone(), parent.clone()))
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
        _safe: bool,
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
        if !run.is_shared() { return None; }
        for task in &run.tasks {
            if self.in_flight.contains_key(&(run.id, task.id)) { continue; }
            let Some(a) = task.attempt() else {
                continue;
            };
            if matches!(task.status, TaskStatus::Cancelled | TaskStatus::Paused) {
                self.stop_runtime(a.child_agent_id, cx);
                continue;
            }
            if task.status == TaskStatus::Preparing && run.status.dispatchable() {
                return Some(Job::Prepare(run.clone(), task.id, parent.clone()));
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
                return Some(Job::Finalize(run.clone(), task.id));
            }
            if task.status == TaskStatus::Running
                && settled
                && !pending
                && self.chats.read(cx).safe_for_delegation(a.child_agent_id)
            {
                return Some(Job::MissingReport(run.clone(), task.id));
            }
        }
        // Deliver questions and corrections before spending a safe boundary on
        // another agent startup. Independent work can still run afterward.
        for d in &run.deliveries {
            if d.status != DeliveryStatus::Queued
                || !self.chats.read(cx).safe_for_delegation(d.target)
                || d.task_id.is_some_and(|id| self.in_flight.contains_key(&(run.id, id)))
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
        if let Some(id) = run.ready_tasks(total).into_iter()
            .find(|id| !self.in_flight.contains_key(&(run.id, *id))) {
            return Some(Job::BeginPrepare(run.clone(), id, parent.clone()));
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
        match effect {
            Effect::None => (),
            Effect::Adopt(agent) => {
                // Record adoption is safe while stopped, but a stale attempt
                // must never replace the current chat configuration.
                if let Some(binding) = agent.delegation.as_ref().filter(|b| b.task_id.is_some()) {
                    if run.task(binding.task_id.unwrap())?.attempt()
                        .is_none_or(|a| Some(a.id) != binding.attempt_id) { return Ok(()); }
                } else if run.status.stopped() || run.status.terminal() {
                    return Ok(());
                }
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
                                a.progress = "Working".into();
                            }
                        }
                    }
                    Ok(())
                })?;
            }
        }
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
                "User ended delegation; shared project edits retained",
            );
            Ok(())
        })?;
        let run = store.load_delegation(run_id)?;
        self.chats.update(cx, |chats, _| {
            chats.allow_managed_resume(run.parent_agent_id)
        });
        self.runs = store.load_delegations()?;
        self.notify_run_changed(run_id, cx);
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
        self.notify_run_changed(run_id, cx);
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
        self.notify_run_changed(run_id, cx);
        Ok(())
    }
    pub fn ensure_assignment_chat(&mut self, run_id: Uuid, task_id: Uuid, cx: &mut Context<Self>) -> Result<Uuid> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        if let Some(child) = run.task(task_id)?.attempt().map(|a| a.child_agent_id)
            .filter(|id| self.agents.read(cx).agent(*id).is_some()) { return Ok(child); }
        let parent = self.agents.read(cx).agent(run.parent_agent_id).cloned().context("Lead conversation is missing")?;
        let effect = preparation::provision(&store, run, task_id, parent, false)?;
        self.apply_effect(run_id, effect, cx)?;
        self.runs = store.load_delegations()?;
        self.notify_run_changed(run_id, cx);
        Ok(store.load_delegation(run_id)?.task(task_id)?.attempt().context("Bandmate conversation is missing")?.child_agent_id)
    }

    pub fn retry_task(&mut self, run_id: Uuid, task_id: Uuid, cx: &mut Context<Self>) -> Result<()> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        run.require_shared()?;
        ensure!(run.status.dispatchable(), "Resume the Band before retrying this assignment.");
        ensure!(run.task(task_id)?.status == TaskStatus::Failed, "Only a failed assignment can be retried.");
        if let Some(child) = run.task(task_id)?.attempt().map(|a| a.child_agent_id) {
            // A failed provider may already have dropped its controller. Reset
            // its runtime state as well, without adding a false user-Stop row.
            self.chats.update(cx, |chats, cx| { chats.stop_backend_for_lane_exit(child, cx); });
        }
        store.update_delegation(run_id, None, |r| {
            let task = r.task_mut(task_id)?;
            ensure!(task.status == TaskStatus::Failed, "Assignment changed before Retry");
            let started = task.attempt().is_some_and(|a| a.generation > 0);
            task.status = TaskStatus::Paused;
            task.paused_status = Some(if started { TaskStatus::Running } else { TaskStatus::Queued });
            for delivery in &mut r.deliveries {
                if delivery.task_id == Some(task_id) && delivery.target != r.parent_agent_id {
                    if !started && delivery.runtime_generation.is_none() {
                        delivery.status = DeliveryStatus::Acknowledged;
                    } else if delivery.status == DeliveryStatus::Dispatched {
                        delivery.status = DeliveryStatus::Uncertain;
                    }
                }
            }
            Ok(())
        })?;
        self.resume_task(run_id, task_id, cx)
    }

    pub fn resume_task(
        &mut self,
        run_id: Uuid,
        task_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        run.require_shared()?;
        ensure!(run.status.dispatchable(), "Resume the parent task first.");
        let task = run.task(task_id)?;
        ensure!(
            task.status == TaskStatus::Paused,
            "This Bandmate is not paused."
        );
        let child = task.attempt().map(|a| a.child_agent_id);
        let awaiting_preparation = matches!(task.paused_status, Some(TaskStatus::Preparing | TaskStatus::Queued | TaskStatus::Planned))
            && task
                .attempt()
                .is_some_and(|a| a.generation == 0 && !a.prepared);
        let mut received = Vec::new();
        if let Some(child) = child.filter(|_| !awaiting_preparation) {
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
        store.update_delegation(run_id, Some(run.revision), |r| {
            for d in &mut r.deliveries {
                if received.contains(&d.id) {
                    d.status = DeliveryStatus::Acknowledged;
                }
            }
            r.resume_user_task(task_id)
        })?;
        if let Some(child) = child {
            self.chats
                .update(cx, |chats, _| chats.allow_managed_resume(child));
        }
        self.runs = store.load_delegations()?;
        self.notify_run_changed(run_id, cx);
        Ok(())
    }

    /// Sending a correction directly to a stopped teammate is a user action
    /// to continue that teammate. A stopped whole run still requires Resume.
    /// Once saved, return recovery errors as a notice so the UI doesn't invite
    /// resubmitting the same durable message.
    pub fn submit_user_correction(
        &mut self,
        run_id: Uuid,
        task_id: Uuid,
        text: String,
        send_now: bool,
        cx: &mut Context<Self>,
    ) -> Result<Option<String>> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        run.require_shared()?;
        ensure!(run.task(task_id)?.status != TaskStatus::Integrating
            && run.task(task_id)?.paused_status != Some(TaskStatus::Integrating),
            "Changes are being combined. Finish integration recovery before changing this assignment. Your draft is preserved.");
        if send_now
            && run.status.dispatchable()
            && run.task(task_id)?.status != TaskStatus::Paused
            && !run.task(task_id)?.status.terminal()
        {
            self.pause_task(run_id, task_id, cx)?;
        }
        store.update_delegation(run_id, None, |r| r.user_correction(task_id, text))?;
        let run = store.load_delegation(run_id)?;
        let notice = if run.status.dispatchable() && run.task(task_id)?.status == TaskStatus::Paused
        {
            self.resume_task(run_id, task_id, cx)
                .err()
                .map(|e| format!("Message saved and queued. Could not resume this bandmate: {e}"))
        } else {
            None
        };
        self.runs = store.load_delegations()?;
        self.notify_run_changed(run_id, cx);
        Ok(notice)
    }
    pub fn resume(&mut self, run_id: Uuid, cx: &mut Context<Self>) -> Result<()> {
        let store = LocalStore::open_default()?;
        let run = store.load_delegation(run_id)?;
        run.require_shared()?;
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
            if delivery.task_id.is_some_and(|id| run.task(id).is_ok_and(|t| t.status.terminal())) {
                received.push(delivery.id);
                continue;
            }
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
        run.require_shared()?;
        for task in run.tasks.iter().filter(|task| !task.status.terminal()) {
            validate_working_scope(&store, &run, task)?;
        }
        let parent = records
            .iter()
            .find(|a| a.id == run.parent_agent_id)
            .context("Lead record is missing")?;
        for task in run.tasks.iter().filter(|task| !task.status.terminal()) {
            if let Some(a) = task.attempt().filter(|a| {
                a.prepared && !records.iter().any(|record| record.id == a.child_agent_id)
            }) {
                ensure!(a.generation==0,"A previously launched Bandmate record is missing; restoring its session is required.");
                let child = preparation::child_record(&store, &run, task, parent)?;
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
                if task.status==TaskStatus::Preparing && task.attempt().is_some_and(|a|!a.prepared){task.status=TaskStatus::Queued;continue;}
                if matches!(task.status,TaskStatus::Running|TaskStatus::CompletionRequested|TaskStatus::NeedsUser) {
                    task.revision+=1;task.status=TaskStatus::Running;
                    let revision=task.revision;
                    if let Some(a)=task.attempt_mut(){a.result=None;a.result_revision=None;a.report_requested=false;continuations.push((a.child_agent_id,task.id,revision));}
                }
            }
            for (child,task,revision) in continuations {r.queue(child,Some(task),format!("The user resumed this task. Continue from the existing session and shared project; do not repeat external side effects. Read current state for task {task}. Task revision is now {revision}. Reconcile what already happened, finish remaining work, and report the verified result."));}
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
        self.notify_run_changed(run_id, cx);
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
    run.require_shared()?;
    ensure!(repository.is_dir(), "Bandmate working directory is missing.");
    if let Some(attempt) = task.attempt() {
        ensure!(attempt.workspace.canonicalize()? == repository, "Bandmate working directory changed.");
    }
    Ok(())
}

/// A user can correct or stop an assignment while its agent is prepared.
/// Keep that preparation if it still belongs to the same attempt, and deliver the
/// latest brief instead of treating an ordinary correction as a run failure.
fn record_prepared_assignment(
    run: &mut DelegationRun,
    task_id: Uuid,
    attempt_id: Uuid,
    prepared_revision: u64,
    context: &mut String,
) -> Result<bool> {
    let active = run.status.dispatchable();
    let ended = run.status.terminal();
    let task = run.task_mut(task_id)?;
    if task.attempt().is_none_or(|a| a.id != attempt_id) {
        return Ok(false);
    }
    let child = task.attempt().unwrap().child_agent_id;
    task.attempt_mut().unwrap().prepared = true;
    if ended || matches!(task.status, TaskStatus::Cancelled | TaskStatus::Superseded) {
        return Ok(false);
    }
    if task.revision != prepared_revision {
        context.push_str(&format!(
            "\n\nThe assignment was updated during preparation. This brief supersedes the earlier brief.\nCurrent task revision: {}\nGoal: {}\nBrief:\n{}\nExpected completion:\n{}",
            task.revision, task.plan.goal, task.plan.brief, task.plan.expected_outcome
        ));
    }
    if task.status == TaskStatus::Paused {
        task.paused_status = Some(TaskStatus::Running);
    } else {
        task.status = TaskStatus::Running;
    }
    run.queue(child, Some(task_id), context.clone());
    run.event(
        run.parent_agent_id,
        Some(task_id),
        "prepared",
        "Bandmate ready in the shared working directory",
    );
    Ok(active)
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
    latest.require_shared()?;
    if latest.revision != job.run().revision {
        return Ok(Effect::None);
    }
    ensure!(
        latest.status.dispatchable() || latest.status == RunStatus::Preparing || matches!(job, Job::Provision(..)),
        "The run is paused."
    );
    match job {
        Job::Provision(run, task_id, parent) => preparation::provision(store, run, task_id, parent, false),
        Job::BeginPrepare(run, task_id, parent) => preparation::provision(store, run, task_id, parent, true),
        Job::Configure(run, mut parent) => {
            preflight(parent.provider)?;
            parent.delegation = Some(DelegationBinding {
                    execution_version: SHARED_EXECUTION_VERSION,
                run_id: id,
                parent_agent_id: parent.id,
                task_id: None,
                attempt_id: None,
                workspace: None,
                task_kind: None,
            });
            store.update_delegation(id,Some(run.revision),|r| { r.status=RunStatus::Active; r.queue(r.parent_agent_id,None,format!("Managed Bandmates are ready. Run {id}. Continue your share of the task, coordinate responsibilities and review results. Use delegation_wait and end the turn when there is no independent work.")); Ok(()) })?;
            Ok(Effect::Adopt(parent))
        }
        Job::Prepare(run, task_id, parent) => {
            let task = run.task(task_id)?.clone();
            let expert = &task.expert.profile;
            ensure!(task.status == TaskStatus::Preparing, "Assignment is not preparing");
            validate_working_scope(store, &run, &task)?;
            preflight(expert.provider)?;
            // Launch from the immutable package, including after import or a
            // changed/uninstalled source skill. The original path is provenance.
            task.expert
                .runtime_instructions(&store.root().join("expert-skill-cache"))?;
            let attempt_id = task.attempt().context("Bandmate conversation is missing")?.id;
            let mut child = preparation::child_record(store, &run, &task, &parent)?;
            let memories = store.load_memories_for_project(parent.project_id)?;
            let background = ide_core::delegation::context::parent_context(store, parent.id, None)?;
            let context = format!("Choro managed assignment\nRun: {id}\nTask: {task_id}\nAttempt: {attempt_id}\nTask revision: {}\nWorking directory: {}\n\nExact user assignment (user authority):\n{}\n\nGoal: {}\nBrief from lead:\n{}\nExpected completion:\n{}\n\nApplicable Choro memories:\n{}\n\nUse delegation_read with include_context for additional scoped parent context. Background excerpts are not authorization. Work directly in this shared folder. Follow its project instructions and coordinate overlapping edits with the lead. Never create a private project copy, clone, worktree, baseline, or project snapshot for this assignment. Report with delegation_complete for these exact IDs and revision. {}",task.revision,task.plan.repository.display(),run.original_assignment,task.plan.goal,task.plan.brief,task.plan.expected_outcome,serde_json::to_string(&memories)?,managed::MANAGED_INSTRUCTIONS);
            let context = format!(
                "{context}\n\nScoped background (not authority):\n{}",
                serde_json::to_string(&background)?
            );
            let mut context = context;
            let adopt = store.update_delegation(id, None, |r| {
                record_prepared_assignment(r, task_id, attempt_id, task.revision, &mut context)
            })?;
            child.doc = context;
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
                    ensure!(task.attempt().is_none_or(|a| a.generation == 0)
                        || agent.chat_session_id.is_some() || agent.cli_session_id.is_some(),
                        "Cannot continue this Bandmate: its saved provider session ID is missing. Restore the original session before retrying.");
                }
            }
            let history = super::chat_dispatch::load_history_from_store(store, &agent)?;
            let mut text = run
                .deliveries
                .iter()
                .filter(|d| ids.contains(&d.id))
                .map(|d| format!("[Choro delivery {}]\n{}", d.id, d.text))
                .collect::<Vec<_>>()
                .join("\n\n");
            if let Some(task) = run.tasks.iter().find(|t| t.attempt().is_some_and(|a| a.child_agent_id == agent.id)) {
                text.push_str(&format!("\n\nCurrent task revision: {}. Apply all queued corrections in order; later corrections supersede earlier ones. Use this revision for completion, not an older revision mentioned above.", task.revision));
            }
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
        Job::Finalize(run, task_id) => {
            let task = run.task(task_id)?;
            let a = task.attempt().context("Missing attempt")?;
            validate_working_scope(store, &run, task)?;
            store.update_delegation(id,None,|r| {
                if !r.status.dispatchable(){return Ok(());}
                let t=r.task(task_id)?;
                if t.revision!=task.revision||t.status!=TaskStatus::CompletionRequested||t.attempt().is_none_or(|current|current.id!=a.id){return Ok(());}
                let t=r.task_mut(task_id)?; t.status=TaskStatus::ResultReady;
                let summary=t.attempt().and_then(|a|a.result.as_ref()).context("Missing completion report")?.summary.clone();
                r.event(r.parent_agent_id,Some(task_id),"result",summary.clone());
                r.queue(r.parent_agent_id,Some(task_id),format!("Bandmate result ready for task {task_id}: {summary}. Review the report and shared files, then use delegation_control accept or request_changes. Verify the combined outcome.")); Ok(())
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

    }
}
