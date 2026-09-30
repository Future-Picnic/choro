use super::*;

// ── Post-ship task actions: comment + status suggestion ──────────────────────
//
// When an agent ships a PR for a linked task, the ship card offers two opt-in
// actions — post a comment and move the task's status. Nothing is sent to the
// third-party tracker until the user hits Apply. `ShipTaskSeed` is the immutable
// snapshot captured at render time so click handlers stay self-sufficient;
// `ShipTaskUi` is the transient (unpersisted) editing state.

/// Everything a ship card's click handlers need to seed and apply task actions.
#[derive(Clone)]
pub(in crate::ui::center) struct ShipTaskSeed {
    pub id: String,
    pub agent_id: Uuid,
    pub project: ProjectId,
    pub task: TaskRef,
    pub comment_body: String,
    pub suggested_status: Option<String>,
}

/// Transient editing state for one ship card's task actions (not persisted).
pub(in crate::ui::center) struct ShipTaskUi {
    pub comment_enabled: bool,
    /// Chosen status by display name; `None` means "don't change".
    pub selected_status: Option<String>,
    /// Authoritative status list from the provider; empty until loaded.
    pub statuses: Vec<ide_core::TaskStatusOption>,
    pub statuses_loading: bool,
    pub applying: bool,
    pub error: Option<String>,
}

impl ShipTaskUi {
    pub(in crate::ui::center) fn from_seed(seed: &ShipTaskSeed) -> Self {
        Self {
            comment_enabled: true,
            selected_status: seed.suggested_status.clone(),
            statuses: Vec::new(),
            statuses_loading: false,
            applying: false,
            error: None,
        }
    }
}

/// The comment text posted to the task when a PR ships.
pub(in crate::ui::center) fn ship_task_comment_body(
    pr_title: &Option<String>,
    pr_url: &str,
) -> String {
    match pr_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
    {
        Some(title) => format!("Shipped a pull request for this task — {title}\n{pr_url}"),
        None => format!("Shipped a pull request for this task: {pr_url}"),
    }
}

fn friendly_error(error: &anyhow::Error) -> String {
    error
        .to_string()
        .lines()
        .next()
        .unwrap_or("Something went wrong")
        .trim()
        .to_string()
}

/// Runs the actual writes on a background thread. For personal tasks this hits
/// the local store; for external trackers it goes through `TaskTrackerClient`.
fn apply_ship_task_actions_blocking(
    seed: &ShipTaskSeed,
    connection: Option<ide_core::TaskTrackerConnection>,
    comment_enabled: bool,
    selected_status: Option<String>,
) -> anyhow::Result<crate::state::agent_chat::ShipTaskApplied> {
    let mut commented = false;
    let mut status_name = None;

    if seed.task.provider == ide_core::IssueTrackerProvider::Personal {
        let task_id = uuid::Uuid::parse_str(&seed.task.issue_id)
            .map_err(|_| anyhow::anyhow!("invalid personal task id"))?;
        let store = ide_core::local_store::LocalStore::open_default()?;
        if comment_enabled {
            store.add_personal_task_comment(task_id, "Agent", seed.comment_body.clone())?;
            commented = true;
        }
        if let Some(name) = selected_status.as_deref() {
            let status = ide_core::PersonalTaskStatus::ALL
                .into_iter()
                .find(|status| status.label().eq_ignore_ascii_case(name.trim()))
                .ok_or_else(|| anyhow::anyhow!("unknown status \"{name}\""))?;
            store.set_personal_task_status(task_id, status)?;
            status_name = Some(status.label().to_string());
        }
    } else {
        let connection =
            connection.ok_or_else(|| anyhow::anyhow!("no connection found for this task"))?;
        let client = ide_core::TaskTrackerClient::new(connection)?;
        if comment_enabled {
            client.add_comment(&seed.task, &seed.comment_body)?;
            commented = true;
        }
        if let Some(name) = selected_status.as_deref() {
            status_name = Some(client.set_status_by_name(&seed.task, name)?);
        }
    }

    Ok(crate::state::agent_chat::ShipTaskApplied {
        commented,
        status_name,
        at: unix_now_secs(),
    })
}

impl CenterArea {
    pub(in crate::ui::center) fn ship_task_ui(&self, ship_id: &str) -> Option<&ShipTaskUi> {
        self.ship_task_ui.get(ship_id)
    }

    /// Ensure transient state exists and warm up the status list. Idempotent.
    pub(in crate::ui::center) fn seed_ship_task_ui(
        &mut self,
        seed: &ShipTaskSeed,
        cx: &mut Context<Self>,
    ) {
        self.ship_task_ui
            .entry(seed.id.clone())
            .or_insert_with(|| ShipTaskUi::from_seed(seed));
        self.ensure_ship_task_statuses(seed, cx);
    }

    pub(in crate::ui::center) fn ensure_ship_task_statuses(
        &mut self,
        seed: &ShipTaskSeed,
        cx: &mut Context<Self>,
    ) {
        // Personal statuses are known synchronously; Asana status isn't supported.
        match seed.task.provider {
            ide_core::IssueTrackerProvider::Personal => {
                if let Some(ui) = self.ship_task_ui.get_mut(&seed.id) {
                    if ui.statuses.is_empty() {
                        ui.statuses = ide_core::PersonalTaskStatus::options();
                    }
                }
                return;
            }
            ide_core::IssueTrackerProvider::Asana => return,
            _ => {}
        }
        let already = self
            .ship_task_ui
            .get(&seed.id)
            .map(|ui| !ui.statuses.is_empty() || ui.statuses_loading)
            .unwrap_or(false);
        if already {
            return;
        }
        let Some(connection) =
            self.tasks
                .read(cx)
                .connection_object_for_ref(seed.project, &seed.task, cx)
        else {
            return;
        };
        if let Some(ui) = self.ship_task_ui.get_mut(&seed.id) {
            ui.statuses_loading = true;
        }
        let ship_id = seed.id.clone();
        let task = seed.task.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    ide_core::TaskTrackerClient::new(connection)?.available_statuses(&task)
                })
                .await;
            this.update(cx, |center, cx| {
                if let Some(ui) = center.ship_task_ui.get_mut(&ship_id) {
                    ui.statuses_loading = false;
                    match result {
                        Ok(statuses) => ui.statuses = statuses,
                        Err(error) => ui.error = Some(friendly_error(&error)),
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(in crate::ui::center) fn toggle_ship_task_comment(
        &mut self,
        seed: &ShipTaskSeed,
        cx: &mut Context<Self>,
    ) {
        let ui = self
            .ship_task_ui
            .entry(seed.id.clone())
            .or_insert_with(|| ShipTaskUi::from_seed(seed));
        ui.comment_enabled = !ui.comment_enabled;
        ui.error = None;
        cx.notify();
    }

    pub(in crate::ui::center) fn set_ship_task_status(
        &mut self,
        seed: &ShipTaskSeed,
        status: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let ui = self
            .ship_task_ui
            .entry(seed.id.clone())
            .or_insert_with(|| ShipTaskUi::from_seed(seed));
        ui.selected_status = status;
        ui.error = None;
        cx.notify();
    }

    pub(in crate::ui::center) fn skip_ship_task_actions(
        &mut self,
        seed: &ShipTaskSeed,
        cx: &mut Context<Self>,
    ) {
        self.ship_task_ui.remove(&seed.id);
        self.set_ship_result_applied(
            seed.agent_id,
            &seed.id,
            crate::state::agent_chat::ShipTaskApplied {
                commented: false,
                status_name: None,
                at: unix_now_secs(),
            },
            cx,
        );
    }

    pub(in crate::ui::center) fn apply_ship_task_actions(
        &mut self,
        seed: &ShipTaskSeed,
        cx: &mut Context<Self>,
    ) {
        let (comment_enabled, selected_status) = match self.ship_task_ui.get(&seed.id) {
            Some(ui) => (ui.comment_enabled, ui.selected_status.clone()),
            None => (true, seed.suggested_status.clone()),
        };
        if !comment_enabled && selected_status.is_none() {
            self.skip_ship_task_actions(seed, cx);
            return;
        }
        if let Some(ui) = self.ship_task_ui.get_mut(&seed.id) {
            ui.applying = true;
            ui.error = None;
        }
        cx.notify();

        let connection =
            self.tasks
                .read(cx)
                .connection_object_for_ref(seed.project, &seed.task, cx);
        let seed = seed.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn({
                    let seed = seed.clone();
                    async move {
                        apply_ship_task_actions_blocking(
                            &seed,
                            connection,
                            comment_enabled,
                            selected_status,
                        )
                    }
                })
                .await;
            this.update(cx, |center, cx| match outcome {
                Ok(applied) => {
                    center.ship_task_ui.remove(&seed.id);
                    center.set_ship_result_applied(seed.agent_id, &seed.id, applied, cx);
                    center.tasks.update(cx, |tasks, cx| {
                        tasks.refresh_project(seed.project, cx);
                    });
                }
                Err(error) => {
                    if let Some(ui) = center.ship_task_ui.get_mut(&seed.id) {
                        ui.applying = false;
                        ui.error = Some(friendly_error(&error));
                    }
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(in crate::ui::center) fn set_ship_result_applied(
        &mut self,
        agent_id: Uuid,
        ship_id: &str,
        applied: crate::state::agent_chat::ShipTaskApplied,
        cx: &mut Context<Self>,
    ) {
        let mut timeline_to_persist = None;
        self.agent_chats.update(cx, |chats, cx| {
            let Some(session) = chats.sessions.get_mut(&agent_id) else {
                return;
            };
            for item in session.timeline.iter_mut() {
                if let AgentChatTimelineItem::ShipResult(result) = item {
                    if result.id == ship_id {
                        result.applied = Some(applied.clone());
                        timeline_to_persist = Some(session.timeline.clone());
                        break;
                    }
                }
            }
            cx.notify();
        });
        if let Some(timeline) = timeline_to_persist {
            if let Err(error) = persist_timeline_snapshot(agent_id, &timeline) {
                eprintln!("failed to persist ship task update: {error:#}");
            }
        }
    }
}
