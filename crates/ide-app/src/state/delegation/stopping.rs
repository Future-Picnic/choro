use super::*;

impl DelegationCoordinator {
    pub(super) fn request_stop(
        &mut self,
        run_id: Uuid,
        end: bool,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        ensure!(
            !self.stop_in_flight.contains(&run_id),
            "Stop is still being saved. Try again shortly."
        );
        let run = self
            .runs
            .iter()
            .find(|run| run.id == run_id)
            .cloned()
            .context("Bandmate state is still loading. Try again shortly.")?;
        self.stop_requested.insert(run_id);
        self.stop_in_flight.insert(run_id);
        self.stop_runtime(run.parent_agent_id, cx);
        self.stopped_parents.insert(run_id);
        for task in &run.tasks {
            if let Some(attempt) = task.attempt() {
                self.stop_runtime(attempt.child_agent_id, cx);
            }
        }
        if let Some(run) = self.runs.iter_mut().find(|r| r.id == run_id) {
            run.pause("Stopping — saving your request", false);
        }
        self.notify_run_changed(run_id, cx);
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let store = LocalStore::open_default()?;
                // Safe to retry: Stop is idempotent and sends no provider work.
                for attempt in 0..3 {
                    let result = store.update_delegation(run_id, None, |run| {
                        run.pause("Stopped by user", false);
                        if end {
                            run.status = RunStatus::Cancelled;
                            run.pause_reason = Some("Ended by user; conversations and files retained".into());
                            for task in &mut run.tasks {
                                if !task.status.terminal() {
                                    task.status = TaskStatus::Cancelled;
                                    task.reason = Some("User ended delegation".into());
                                }
                            }
                            run.event(run.parent_agent_id, None, "cancelled", "User ended delegation; shared project edits retained");
                        }
                        Ok(())
                    });
                    match result {
                        Err(error) if attempt < 2 && ide_core::local_store::is_database_busy(&error) => {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        result => { result?; break; }
                    }
                }
                store.load_delegation(run_id)
            }).await;
            let _ = this.update(cx, |s, cx| {
                s.stop_in_flight.remove(&run_id);
                match result {
                    Ok(run) => {
                        if end {
                            s.chats.update(cx, |chats, _| chats.allow_managed_resume(run.parent_agent_id));
                        }
                        if let Some(current) = s.runs.iter_mut().find(|r| r.id == run_id) { *current = run; }
                        s.stop_requested.remove(&run_id);
                        s.error = None;
                    }
                    Err(error) => {
                        // Keep the local dispatch gate closed on save failure.
                        // A user can retry Stop; a stale poll cannot restart work.
                        s.error = Some(format!("Bandmates stopped locally, but Stop could not be saved. Retry Stop: {error:#}"));
                        eprintln!("{}", s.error.as_deref().unwrap());
                    }
                }
                s.notify_run_changed(run_id, cx);
            });
        }).detach();
        Ok(())
    }
}
