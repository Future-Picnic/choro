use super::super::effects::{delivery_still_current, persist_dispatch, DispatchResult};
use super::*;

#[cfg(feature = "ui-layout-tests")]
#[gpui::test]
fn pending_or_failed_stop_survives_active_poll_and_allows_manual_lead_message(
    cx: &mut gpui::TestAppContext,
) {
    let mut fixture = Fixture::new();
    fixture.configure();
    let run = fixture.load();
    let parent = fixture.parent.clone();
    cx.update(|cx| {
        let agents = cx.new(|_| AgentRecords::in_memory(vec![parent.clone()]));
        let chats = cx.new(|_| AgentChatState::new());
        let terminals = cx.new(|_| TerminalManager::new());
        let (wake, _) = async_channel::bounded(1);
        let coordinator = cx.new(|_| DelegationCoordinator {
            runs: vec![run.clone()],
            error: None,
            agents,
            chats,
            terminals,
            settled: HashMap::new(),
            cursor: 0,
            stopped_parents: HashSet::new(),
            wake,
            reload_epoch: 0,
            snapshot: vec![],
            in_flight: HashMap::new(),
            telemetry_in_flight: HashSet::new(),
            stop_requested: HashSet::from([run.id]),
            stop_in_flight: HashSet::from([run.id]),
        });
        coordinator.update(cx, |state, cx| {
            for saving in [true, false] {
                if !saving {
                    state.stop_in_flight.clear();
                    state.error = Some("Stop save failed".into());
                }
                assert!(state.accept_snapshot(
                    0,
                    vec![run.clone()],
                    run_stamps(&[run.clone()]).unwrap(),
                    cx
                ));
                assert!(state.runs[0].status.stopped());
                assert!(state.next_job(cx).is_none());
                assert!(state.prepare_parent_message(parent.id, cx).unwrap());
                assert!(state.stopped_parents.contains(&run.id));
                if !saving {
                    assert_eq!(state.error.as_deref(), Some("Stop save failed"));
                }
            }
        });
    });
}

#[test]
fn stopped_unsent_delivery_requeues_but_a_sent_one_is_never_replayed() {
    let mut f = Fixture::new();
    f.configure();
    let run = f.load();
    let delivery = run.deliveries[0].id;
    f.store
        .update_delegation(f.run, None, |r| {
            r.deliveries[0].status = DeliveryStatus::Dispatched;
            r.pause("Stop during save", false);
            Ok(())
        })
        .unwrap();
    persist_dispatch(&f.store, f.run, DispatchResult::Requeue(vec![delivery])).unwrap();
    let stopped = f.load();
    assert!(stopped.status.stopped());
    assert_eq!(stopped.deliveries[0].status, DeliveryStatus::Queued);
    f.store
        .update_delegation(f.run, None, |r| {
            r.deliveries[0].status = DeliveryStatus::Uncertain;
            Ok(())
        })
        .unwrap();
    persist_dispatch(
        &f.store,
        f.run,
        DispatchResult::Sent {
            target: f.parent.id,
            generation: 7,
            ids: vec![delivery],
        },
    )
    .unwrap();
    persist_dispatch(&f.store, f.run, DispatchResult::Requeue(vec![delivery])).unwrap();
    assert_eq!(f.load().deliveries[0].status, DeliveryStatus::Uncertain);
    assert_eq!(f.load().deliveries[0].runtime_generation, Some(7));
}

#[test]
fn acknowledgement_or_send_during_a_save_invalidates_prepared_delivery() {
    let mut f = Fixture::new();
    f.configure();
    let prepared = f.load();
    let mut current = prepared.clone();
    let delivery = current.deliveries[0].id;
    current.deliveries[0].status = DeliveryStatus::Dispatched;
    assert!(delivery_still_current(
        &current,
        &prepared,
        &f.parent,
        &[delivery]
    ));
    current.deliveries[0].status = DeliveryStatus::Acknowledged;
    assert!(!delivery_still_current(
        &current,
        &prepared,
        &f.parent,
        &[delivery]
    ));
    current.deliveries[0].status = DeliveryStatus::Dispatched;
    current.deliveries[0].runtime_generation = Some(1);
    assert!(!delivery_still_current(
        &current,
        &prepared,
        &f.parent,
        &[delivery]
    ));
}

#[test]
fn correction_during_save_prevents_sending_the_old_task_revision() {
    let mut f = Fixture::new();
    f.configure();
    let task = f.load().tasks[0].id;
    let agent = f.prepare(task);
    let prepared = f.load();
    let mut current = prepared.clone();
    let delivery = current
        .deliveries
        .iter_mut()
        .find(|d| d.target == agent.id)
        .unwrap();
    delivery.status = DeliveryStatus::Dispatched;
    let id = delivery.id;
    assert!(delivery_still_current(&current, &prepared, &agent, &[id]));
    current.task_mut(task).unwrap().revision += 1;
    assert!(!delivery_still_current(&current, &prepared, &agent, &[id]));
}
