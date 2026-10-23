//! Applying background work must not put database waits back on the UI thread.
use super::super::agent_chat::{load_file_ledger_from_store, load_latest_plan_from_store};
use super::*;

fn may_send(run: &DelegationRun, agent: &AgentRecord) -> bool {
    run.status.dispatchable()
        && agent
            .delegation
            .as_ref()
            .and_then(|b| b.task_id.map(|id| (id, b.attempt_id)))
            .is_none_or(|(id, attempt)| {
                run.task(id).is_ok_and(|task| {
                    matches!(
                        task.status,
                        TaskStatus::Running
                            | TaskStatus::WaitingForLead
                            | TaskStatus::CompletionRequested
                    ) && task.attempt().is_some_and(|a| Some(a.id) == attempt)
                })
            })
}

pub(super) enum DispatchResult {
    Requeue(Vec<Uuid>),
    Sent {
        target: Uuid,
        generation: u64,
        ids: Vec<Uuid>,
    },
}

pub(super) fn persist_dispatch(
    store: &LocalStore,
    run_id: Uuid,
    result: DispatchResult,
) -> Result<()> {
    match result {
        DispatchResult::Requeue(ids) => store.update_delegation(run_id, None, |run| {
            for delivery in &mut run.deliveries {
                if ids.contains(&delivery.id) && delivery.runtime_generation.is_none() {
                    // No provider send occurred. Do not reopen cancelled deliveries.
                    if matches!(
                        delivery.status,
                        DeliveryStatus::Dispatched | DeliveryStatus::Uncertain
                    ) {
                        delivery.status = DeliveryStatus::Queued;
                    }
                }
            }
            Ok(())
        }),
        DispatchResult::Sent {
            target,
            generation,
            ids,
        } => store.update_delegation(run_id, None, |run| {
            for delivery in &mut run.deliveries {
                if ids.contains(&delivery.id) {
                    delivery.runtime_generation = Some(generation);
                }
            }
            for task in &mut run.tasks {
                if let Some(attempt) = task.attempt_mut().filter(|a| a.child_agent_id == target) {
                    attempt.generation = generation;
                    attempt.progress = "Working".into();
                }
            }
            Ok(())
        }),
    }
}

impl DelegationCoordinator {
    pub(super) async fn finish_job(
        this: gpui::WeakEntity<Self>,
        job: Job,
        result: Result<Effect>,
        cx: &mut gpui::AsyncApp,
    ) {
        let run_id = job.run().id;
        let result: Result<()> = async {
            let effect = result?;
            if matches!(effect, Effect::None) {
                return Ok(());
            }
            let run = cx
                .background_executor()
                .spawn(async move { LocalStore::open_default()?.load_delegation(run_id) })
                .await?;
            let snapshot = this.update(cx, |s, cx| s.stage_effect(&run, &effect, cx))??;
            if snapshot.is_none() {
                if let Effect::Send(_, _, _, _, ids) = effect {
                    cx.background_executor()
                        .spawn(async move {
                            persist_dispatch(
                                &LocalStore::open_default()?,
                                run_id,
                                DispatchResult::Requeue(ids),
                            )
                        })
                        .await?;
                }
                return Ok(());
            }
            if let Some((revision, snapshot)) = snapshot {
                cx.background_executor()
                    .spawn(async move {
                        super::super::agents::persist_agent_store_snapshot(revision, snapshot)
                    })
                    .await?;
            }
            if let Effect::Send(agent, history, text, _, ids) = effect {
                let target = agent.id;
                // Refresh after the save barrier; Stop, pause and corrections
                // can arrive while disk I/O is pending.
                let (run, files, plan) = cx
                    .background_executor()
                    .spawn(async move {
                        let store = LocalStore::open_default()?;
                        let files = load_file_ledger_from_store(&store, target)?;
                        let plan = load_latest_plan_from_store(&store, target)?;
                        Ok::<_, anyhow::Error>((store.load_delegation(run_id)?, files, plan))
                    })
                    .await?;
                let outcome = this.update(cx, |s, cx| {
                    if !may_send(&run, &agent)
                        || s.stopped_parents.contains(&run_id)
                        || s.stop_requested.contains(&run_id)
                        || !delivery_still_current(&run, job.run(), &agent, &ids)
                        || !s.safe_reserved(target, cx)
                    {
                        return Ok(DispatchResult::Requeue(ids));
                    }
                    let current = s
                        .agents
                        .read(cx)
                        .agent(target)
                        .cloned()
                        .context("The conversation was removed before dispatch")?;
                    // A newer policy/attempt must not be sent using the old history.
                    if current.delegation != agent.delegation {
                        return Ok(DispatchResult::Requeue(ids));
                    }
                    let mode = if target == run.parent_agent_id && run.plan_mode {
                        AgentInteractionMode::Plan
                    } else {
                        AgentInteractionMode::Default
                    };
                    s.settled.remove(&target);
                    let generation = s.chats.update(cx, |chats, cx| {
                        chats.delegation_reservations.remove(&target);
                        super::super::chat_dispatch::managed_send(
                            chats,
                            current,
                            history,
                            Some(files),
                            plan,
                            text,
                            mode,
                            cx,
                        )
                    })?;
                    s.agents
                        .update(cx, |a, cx| a.mark_started(target, None, cx));
                    Ok::<_, anyhow::Error>(DispatchResult::Sent {
                        target,
                        generation,
                        ids,
                    })
                })??;
                cx.background_executor()
                    .spawn(async move {
                        persist_dispatch(&LocalStore::open_default()?, run_id, outcome)
                    })
                    .await?;
            }
            Ok(())
        }
        .await;
        let error = if let Err(error) = result {
            let failed_job = job.clone();
            let message = format!("{error:#}");
            eprintln!("Bandmate job {run_id} failed: {message}");
            cx.background_executor()
                .spawn(async move {
                    LocalStore::open_default().and_then(|store| {
                        preparation::record_job_failure(&store, &failed_job, &error)
                    })
                })
                .await
                .err()
                .map(|error| format!("{error:#}"))
        } else {
            None
        };
        let _ = this.update(cx, |s, cx| {
            if let Some(error) = error {
                s.error = Some(error);
                cx.emit(DelegationChanged {
                    parents: vec![job.run().parent_agent_id],
                    error_changed: true,
                });
                cx.notify();
            }
            if let Some(id) = job.reserved_agent() {
                s.chats
                    .update(cx, |chats, cx| chats.release_delegation_reservation(id, cx));
            }
            s.request_reload();
        });
    }

    /// Snapshot creation and model updates only; the caller owns the save barrier.
    pub(super) fn stage_effect(
        &mut self,
        run: &DelegationRun,
        effect: &Effect,
        cx: &mut Context<Self>,
    ) -> Result<Option<(u64, ide_core::AgentStoreFile)>> {
        let agent = match effect {
            Effect::None => return Ok(None),
            Effect::Adopt(agent) => {
                if let Some(binding) = agent.delegation.as_ref().filter(|b| b.task_id.is_some()) {
                    if run
                        .task(binding.task_id.unwrap())?
                        .attempt()
                        .is_none_or(|a| Some(a.id) != binding.attempt_id)
                    {
                        return Ok(None);
                    }
                } else if run.status.stopped() || run.status.terminal() {
                    return Ok(None);
                }
                agent
            }
            Effect::Send(agent, ..) => {
                if !may_send(run, agent) || !self.safe_reserved(agent.id, cx) {
                    return Ok(None);
                }
                agent
            }
        };
        let changed = self
            .agents
            .read(cx)
            .agent(agent.id)
            .is_none_or(|stored| stored.delegation != agent.delegation);
        if matches!(effect, Effect::Adopt(_)) || changed {
            if self.chats.read(cx).has_backend(agent.id) {
                ensure!(
                    self.chats
                        .update(cx, |chats, cx| chats.retire_idle_backend(agent.id, cx)),
                    "The provider has not reached a safe reconfiguration boundary."
                );
            }
            self.agents
                .update(cx, |records, cx| records.adopt_managed(agent.clone(), cx));
        }
        // A concurrent adoption can still be saving. Every send must cross a
        // durable snapshot barrier before it can reach a provider.
        Ok(Some(
            self.agents
                .update(cx, |records, _| records.durable_snapshot()),
        ))
    }

    fn safe_reserved(&mut self, id: Uuid, cx: &mut Context<Self>) -> bool {
        self.chats.update(cx, |chats, _| {
            let reserved = chats.delegation_reservations.remove(&id);
            let safe = chats.safe_for_delegation(id);
            if reserved {
                chats.delegation_reservations.insert(id);
            }
            safe
        })
    }

    pub(super) fn queue_telemetry(
        &mut self,
        run: Uuid,
        task: Uuid,
        attempt: Uuid,
        session: Option<String>,
        usage: Option<serde_json::Value>,
        cx: &mut Context<Self>,
    ) {
        if !self.telemetry_in_flight.insert((run, task)) {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default()?
                        .update_delegation_telemetry(run, task, attempt, session, usage)
                })
                .await;
            if let Err(error) = result {
                eprintln!("Could not save Bandmate telemetry: {error:#}");
            }
            let _ = this.update(cx, |s, _| {
                s.telemetry_in_flight.remove(&(run, task));
            });
        })
        .detach();
    }
}

pub(super) fn delivery_still_current(
    run: &DelegationRun,
    prepared: &DelegationRun,
    agent: &AgentRecord,
    ids: &[Uuid],
) -> bool {
    let task_current = agent
        .delegation
        .as_ref()
        .and_then(|b| b.task_id)
        .is_none_or(|id| {
            run.task(id)
                .ok()
                .zip(prepared.task(id).ok())
                .is_some_and(|(now, before)| {
                    now.revision == before.revision
                        && now.attempt().map(|a| a.id) == before.attempt().map(|a| a.id)
                })
        });
    task_current
        && ids.iter().all(|id| {
            run.deliveries.iter().any(|delivery| {
                delivery.id == *id
                    && delivery.target == agent.id
                    && delivery.status == DeliveryStatus::Dispatched
                    && delivery.runtime_generation.is_none()
            })
        })
}
