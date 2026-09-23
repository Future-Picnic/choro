use super::experts_cards::{delegation_glyph, task_status_label};
use super::*;
use crate::state::delegation::display::{self, DelegationActivity};
use crate::state::delegation::DelegationHandle;
use ide_core::{
    delegation::{DelegationRun, DelegationTask, RunStatus, TaskKind},
    experts::ExpertSnapshot,
};

pub(super) fn profiles() -> Vec<ide_core::experts::ExpertProfile> {
    if !ide_core::delegation::enabled() {
        return vec![];
    }
    LocalStore::open_default()
        .and_then(|s| s.load_experts())
        .unwrap_or_default()
        .into_iter()
        .filter(|p| p.enabled && !p.archived)
        .collect()
}
pub(super) fn capabilities(provider: AgentKind) -> Vec<AgentCapability> {
    let mut items = profiles()
        .into_iter()
        .map(|p| AgentCapability {
            expert_id: Some(p.id),
            skill_path: None,
            provider: p.provider,
            source: AgentCapabilitySource::Expert,
            name: p.name.clone(),
            title: p.name,
            invocation: String::new(),
            description: Some(p.description),
            instructions: None,
            orbit_module_id: None,
            enabled: true,
        })
        .collect::<Vec<_>>();
    items.push(AgentCapability {
        expert_id: None,
        skill_path: None,
        provider,
        source: AgentCapabilitySource::Delegate,
        name: "delegate".into(),
        title: "Delegate work".into(),
        invocation: String::new(),
        description: Some("Create on-demand teammates or choose a saved bandmate".into()),
        instructions: None,
        orbit_module_id: None,
        enabled: true,
    });
    items
}
pub(super) fn snapshot(id: Uuid) -> anyhow::Result<ExpertSnapshot> {
    profiles()
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Bandmate is disabled or archived. Choose another Bandmate in Settings."
            )
        })?
        .snapshot()
}

/// Called only with the literal text submitted by the user, before context,
/// skills, attachments or handoff messages are added to the provider prompt.
pub(super) fn authorize(
    parent: Uuid,
    text: &str,
    explicit: &[Uuid],
    plan: bool,
) -> anyhow::Result<Option<Uuid>> {
    if !ide_core::delegation::enabled() {
        return Ok(None);
    }
    LocalStore::open_default()?.prepare_delegation_submission(
        parent,
        Uuid::new_v4(),
        text,
        explicit,
        plan,
    )
}

/// The last path component of a repository, for compact metadata.
fn repository_name(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
}

impl CenterArea {
    /// A new explicit user request may resume a stopped Band through the
    /// normal reconciliation path. Tool output and ordinary chat cannot do so.
    pub(super) fn resume_delegation_for_submission(
        &mut self,
        run: Option<Uuid>,
        text: &str,
        explicit: bool,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<bool> {
        if !explicit && !ide_core::experts::requests_delegation(text) {
            return Ok(false);
        }
        let Some(run) = run else {
            return Ok(false);
        };
        if !LocalStore::open_default()?
            .load_delegation(run)?
            .status
            .stopped()
        {
            return Ok(false);
        }
        let handle = cx
            .try_global::<DelegationHandle>()
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Band coordinator is unavailable. Your request is retained.")
            })?;
        handle.0.update(cx, |s, cx| s.resume(run, cx))?;
        Ok(true)
    }

    pub(super) fn can_delegate_from(&self, parent: Uuid, cx: &App) -> bool {
        ide_core::delegation::enabled()
            && (self.workspace.read(cx).beta_features.delegation
                || self
                    .delegation_runs(parent, cx)
                    .iter()
                    .any(|run| !run.status.terminal()))
    }

    pub(super) fn set_expert_plan_mode(
        &mut self,
        agent_id: Uuid,
        mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) -> bool {
        if self
            .agents
            .read(cx)
            .agent(agent_id)
            .and_then(|a| a.delegation.as_ref())
            .is_some_and(|b| b.task_id.is_some())
        {
            return mode == AgentInteractionMode::Default;
        }
        if !ide_core::delegation::enabled() {
            return true;
        }
        let result=LocalStore::open_default().and_then(|s|{
            for run in s.load_delegations()?.into_iter().filter(|r|r.parent_agent_id==agent_id&&!r.status.terminal()) {
                s.update_delegation(run.id,None,|r|{
                    r.plan_mode=mode==AgentInteractionMode::Plan;
                    if r.plan_mode&&r.tasks.iter().any(|t|t.plan.kind==ide_core::delegation::TaskKind::Implementation&&!t.status.terminal()) {r.pause("Implementation paused in Plan mode. Return to implementation mode and choose Resume to continue.",false);}
                    Ok(())
                })?;
            }Ok(())
        });
        if let Err(e) = result {
            self.agent_start_errors.insert(agent_id, e.to_string());
            cx.notify();
            false
        } else {
            true
        }
    }

    /// Whether a child conversation is waiting on the user. Passed into the
    /// pure display state so real permission and question attention survives.
    pub(super) fn delegated_child_needs_user(&self, child: Uuid, cx: &App) -> bool {
        self.agent_chats.read(cx).session(child).is_some_and(|s| {
            s.pending_approval.is_some()
                || s.pending_user_input.is_some()
                || s.status == AgentChatStatus::PlanReady
        })
    }

    pub(super) fn expert_bandmate_index(&self, agent: &AgentRecord, cx: &App) -> Option<usize> {
        let binding = agent.delegation.as_ref()?;
        let task_id = binding.task_id?;
        let handle = cx.try_global::<DelegationHandle>()?;
        let run = handle.0.read(cx).runs.iter().find(|run| {
            run.id == binding.run_id && run.parent_agent_id == binding.parent_agent_id
        })?;
        display::bandmate_index(run, task_id)
    }

    /// A managed child's header follows its assignment, not the ordinary chat's
    /// manually selected status. This never marks records Done or starts work.
    pub(super) fn render_expert_chat_status(
        &self,
        agent: &AgentRecord,
        cx: &App,
    ) -> Option<gpui::AnyElement> {
        use ide_core::delegation::TaskStatus;

        let binding = agent.delegation.as_ref()?;
        let task_id = binding.task_id?;
        let runs = self.delegation_runs(binding.parent_agent_id, cx);
        let current = runs.iter().find_map(|run| {
            (run.id == binding.run_id)
                .then(|| run.tasks.iter().find(|task| task.id == task_id))
                .flatten()
                .map(|task| (run, task))
        });
        let (label, activity, status) = if let Some((run, task)) = current {
            if task
                .attempt()
                .is_some_and(|attempt| attempt.child_agent_id != agent.id)
            {
                (
                    "Superseded",
                    DelegationActivity::Idle,
                    TaskStatus::Superseded,
                )
            } else if run.status == RunStatus::Cancelled && !task.status.satisfied() {
                ("Cancelled", DelegationActivity::Idle, TaskStatus::Cancelled)
            } else {
                let needs_user = self.delegated_child_needs_user(agent.id, cx);
                let activity = display::task_activity(run.status, task, needs_user);
                let label = if matches!(
                    activity,
                    DelegationActivity::Attention | DelegationActivity::Paused
                ) {
                    task_status_label(run.status, task, needs_user)
                } else {
                    display::completion_stage(run.status, task.status)
                        .unwrap_or_else(|| task_status_label(run.status, task, needs_user))
                };
                (label, activity, task.status)
            }
        } else {
            (
                "Bandmate status unavailable",
                DelegationActivity::Idle,
                TaskStatus::Queued,
            )
        };
        Some(
            h_flex()
                .items_center()
                .gap_1p5()
                .h(crate::ui::design::control_h_xs())
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t2(cx))
                .child(delegation_glyph(
                    activity,
                    status,
                    agent.id.as_u128() as usize,
                    cx,
                ))
                .child(label)
                .into_any_element(),
        )
    }

    /// Open one delegated assignment's conversation in the Expert panel beside
    /// its parent. Used by explicit clicks only (cards, composer chip, sidebar);
    /// activity never opens anything on its own.
    pub(crate) fn open_delegated_task(
        &mut self,
        child: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.agents.read(cx).agent(child).cloned() else {
            return;
        };
        let Some(parent) = agent.delegation.as_ref().map(|b| b.parent_agent_id) else {
            return;
        };
        let parent_open = self
            .active_project(cx)
            .and_then(|(project, _)| self.agents.read(cx).selected_agent_id(project))
            == Some(parent);
        if !parent_open {
            self.open_agent(parent, window, cx);
        }
        self.web_host.update(cx, |h, _| h.set_intent(None));
        self.delegated_overview = None;
        self.delegated_panel = Some(child);
        self.delegated_preview = false;
        self.schedule_agent_chat_hydration(agent, cx);
        cx.notify();
    }

    pub(super) fn render_delegated_panel(
        &mut self,
        child: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(agent) = self.agents.read(cx).agent(child).cloned() else {
            return div()
                .child("Bandmate conversation unavailable")
                .into_any_element();
        };
        let Some(binding) = agent.delegation.clone() else {
            return div().into_any_element();
        };
        let Some(task_id) = binding.task_id else {
            return div().into_any_element();
        };
        let parent = binding.parent_agent_id;
        let title = self
            .agents
            .read(cx)
            .agent(parent)
            .map(|a| a.title.clone())
            .unwrap_or_else(|| "Parent task".into());
        let run = self
            .delegation_runs(parent, cx)
            .into_iter()
            .find(|r| r.id == binding.run_id);
        let task = run
            .as_ref()
            .and_then(|r| r.tasks.iter().find(|t| t.id == task_id).cloned());
        let has_preview = task.as_ref().is_some_and(|t| t.preview.is_some());
        let body = if self.delegated_preview && has_preview {
            let host = self.web_host.clone();
            gpui::canvas(
                move |bounds, window, cx| host.update(cx, |h, _| h.place(bounds, window)),
                |_, _, _, _| {},
            )
            .size_full()
            .into_any_element()
        } else {
            self.render_agent_chat_body_for_surface(
                &agent,
                AgentChatSurface::Delegated {
                    parent,
                    task: task_id,
                },
                window,
                cx,
            )
        };
        let assignment = match (&run, task) {
            (Some(run), Some(task)) => {
                Some(self.render_assignment_card(&agent, run, &task, title.clone(), cx))
            }
            _ => None,
        };
        v_flex()
            .size_full()
            .min_w(px(0.))
            .gap_2()
            .child(
                h_flex()
                    .gap_1p5()
                    .px_3()
                    .pt_2()
                    .flex_wrap()
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "expert-return",
                            "← All bandmates",
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.open_assignment_overview(parent, window, cx);
                                cx.notify();
                            },
                        )),
                    )
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "expert-full-chat",
                            "Full chat",
                            cx,
                        )
                        .on_click(cx.listener(
                            move |this, _, window, cx| {
                                this.web_host.update(cx, |h, _| h.set_intent(None));
                                this.delegated_panel = None;
                                this.delegated_overview = None;
                                this.open_agent(child, window, cx);
                                cx.notify();
                            },
                        )),
                    )
                    .when(has_preview, |header| {
                        header.child(
                            crate::ui::style::dialog_neutral_button(
                                "expert-preview",
                                if self.delegated_preview {
                                    "Conversation"
                                } else {
                                    "Preview"
                                },
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.delegated_preview = !this.delegated_preview;
                                cx.notify();
                            })),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::dialog_neutral_button("expert-close", "Close", cx)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.web_host.update(cx, |h, _| h.set_intent(None));
                                this.delegated_panel = None;
                                cx.notify();
                            })),
                    ),
            )
            .when_some(assignment, |panel, card| {
                panel.child(div().px_3().child(card))
            })
            .child(div().flex_1().min_h(px(0.)).child(body))
            .into_any_element()
    }

    /// The assignment as a compact card: the goal up front, the run's context
    /// as one metadata line, and the verbatim brief behind a disclosure. All
    /// text is the stored plan; nothing is summarised here.
    fn render_assignment_card(
        &self,
        agent: &AgentRecord,
        run: &DelegationRun,
        task: &DelegationTask,
        parent_title: String,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let task_id = task.id;
        let expanded = self.delegated_brief_expanded.contains(&task_id);
        let needs_user = self.delegated_child_needs_user(agent.id, cx);
        let activity = display::task_activity(run.status, task, needs_user);
        let status = task_status_label(run.status, task, needs_user);
        let model = agent.model_label().to_string();
        let mut meta = vec![format!("From {parent_title}")];
        if let Some(repo) = repository_name(&task.plan.repository) {
            meta.push(repo);
        }
        if task.plan.kind == TaskKind::Consultation {
            meta.push("Consultation".into());
        }
        if !task.plan.dependencies.is_empty() {
            meta.push(format!("After {}", task.plan.dependencies.join(", ")));
        }
        let expected = task.plan.expected_outcome.trim().to_string();
        let original = run.original_assignment.trim().to_string();
        let brief = task.plan.brief.trim().to_string();

        let section = |label: &'static str, text: String, cx: &App| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t3(cx))
                        .child(label),
                )
                .child(
                    div()
                        .whitespace_normal()
                        .text_size(crate::ui::design::text_body())
                        .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
                        .text_color(crate::ui::design::chat_body(cx))
                        .child(text),
                )
        };

        crate::ui::style::chat_card(cx)
            .child(
                crate::ui::style::chat_card_head(cx)
                    .flex_wrap()
                    .when_some(display::bandmate_index(run, task_id), |head, index| {
                        head.child(crate::ui::design::indicator::bandmate_icon(
                            index,
                            crate::ui::design::amber(cx),
                            crate::ui::design::icon_md(),
                        ))
                    })
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(task.expert.profile.name.clone()),
                    )
                    .child(div().text_color(crate::ui::design::t4(cx)).child(model))
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
                            .text_color(if activity == DelegationActivity::Attention {
                                crate::ui::design::amber(cx)
                            } else {
                                crate::ui::design::t2(cx)
                            })
                            .child(delegation_glyph(
                                activity,
                                task.status,
                                task_id.as_u128() as usize,
                                cx,
                            ))
                            .child(status),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .py(px(12.))
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.45))
                            .text_color(crate::ui::design::t1(cx))
                            .child(task.plan.goal.clone()),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(meta.join("  ·  ")),
                    )
                    .child(
                        crate::ui::style::delegation_card_action_button(
                            ("assignment-brief-toggle", task_id.as_u128() as u64),
                            if expanded {
                                "Hide full brief"
                            } else {
                                "Show full brief"
                            },
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.delegated_brief_expanded.remove(&task_id) {
                                this.delegated_brief_expanded.insert(task_id);
                            }
                            cx.notify();
                        })),
                    ),
            )
            .child(
                v_flex()
                    .gap_1p5()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .pb_2()
                    .when(run.status.stopped(), |col| {
                        col.child(self.render_delegation_recovery(
                            run.id,
                            run.parent_agent_id,
                            "bandmate-panel",
                            cx,
                        ))
                    })
                    .children(self.render_delegation_task_control(run, task, "bandmate-panel", cx))
                    .children(self.render_delegation_panel_error(run.parent_agent_id, cx)),
            )
            .when(expanded, |card| {
                card.child(
                    v_flex()
                        .id(("assignment-brief", task_id.as_u128() as u64))
                        .w_full()
                        .max_h(px(300.))
                        .overflow_y_scroll()
                        .gap_3()
                        .px(crate::ui::design::chat_card_body_pad_x())
                        .py(px(12.))
                        .border_t_1()
                        .border_color(crate::ui::design::line(cx))
                        .child(section("Brief from the lead", brief, cx))
                        .when(!expected.is_empty(), |col| {
                            col.child(section("Expected outcome", expected, cx))
                        })
                        .when(!original.is_empty(), |col| {
                            col.child(section("Original request", original, cx))
                        }),
                )
            })
            .into_any_element()
    }

    pub(super) fn delegation_runs(&self, parent: Uuid, cx: &App) -> Vec<DelegationRun> {
        cx.try_global::<DelegationHandle>()
            .map(|h| {
                h.0.read(cx)
                    .runs
                    .iter()
                    .filter(|r| r.parent_agent_id == parent)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    fn delegation_indicator(&self, parent: Uuid, cx: &App) -> Option<display::DelegationIndicator> {
        let handle = cx.try_global::<DelegationHandle>()?;
        display::delegation_indicator(&handle.0.read(cx).runs, parent, &|child| {
            self.delegated_child_needs_user(child, cx)
        })
    }
    pub(super) fn delegated_preview_intent(
        &self,
        project: ProjectId,
        cx: &App,
    ) -> Option<web_preview::WebPreviewIntent> {
        if !self.delegated_preview {
            return None;
        }
        let agent = self.agents.read(cx).agent(self.delegated_panel?)?;
        if agent.project_id != project {
            return None;
        }
        let binding = agent.delegation.as_ref()?;
        let run = self
            .delegation_runs(binding.parent_agent_id, cx)
            .into_iter()
            .find(|r| r.id == binding.run_id)?;
        let preview = run.task(binding.task_id?).ok()?.preview.as_ref()?;
        Some(web_preview::WebPreviewIntent::ProjectPreview {
            project_id: agent.project_id,
            url: preview.url.clone(),
            revision: preview.revision,
        })
    }
    pub(super) fn select_expert(&mut self, id: Uuid, cx: &mut Context<Self>) {
        match snapshot(id) {
            Ok(expert) => {
                if let Some(c) = &mut self.new_agent_composer {
                    c.provider = expert.profile.provider;
                    c.model = expert.profile.model;
                    c.effort = expert.profile.effort;
                    c.runtime = AgentRuntimeKind::Chat;
                    c.expert_snapshot = Some(expert);
                    c.error = None;
                }
            }
            Err(e) => {
                if let Some(c) = &mut self.new_agent_composer {
                    c.error = Some(e.to_string());
                }
            }
        }
        cx.notify();
    }

    /// The composer's delegation strip. There is deliberately no standing
    /// "Delegate" button: `/delegate`, a slash-picked Expert, or plain language
    /// start delegation, and this strip only reflects what is already chosen
    /// or already running.
    pub(super) fn render_expert_composer(
        &mut self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let id = agent.id;
        let mut row = h_flex().gap_2().flex_wrap().items_center();
        if let Some(binding) = agent.delegation.as_ref().filter(|b| b.task_id.is_some()) {
            let parent = binding.parent_agent_id;
            let pause_hint = cx.try_global::<DelegationHandle>().and_then(|h| {
                let run = h.0.read(cx).runs.iter().find(|r| r.id == binding.run_id)?;
                if run.status.stopped() {
                    Some("Messages queue until the Band resumes")
                } else if run.task(binding.task_id?).ok()?.status
                    == ide_core::delegation::TaskStatus::Paused
                {
                    Some("Sending a message resumes this bandmate")
                } else {
                    None
                }
            });
            if let Some(hint) = pause_hint {
                row = row.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(hint),
                );
            }
            row = row.child(
                crate::ui::style::dialog_neutral_button(
                    ("expert-parent", id.as_u128() as u64),
                    "Return to parent",
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.web_host.update(cx, |h, _| h.set_intent(None));
                    this.delegated_panel = None;
                    this.open_agent(parent, window, cx);
                    cx.notify();
                })),
            );
            let queued = cx
                .try_global::<DelegationHandle>()
                .and_then(|handle| {
                    handle
                        .0
                        .read(cx)
                        .runs
                        .iter()
                        .find(|run| run.id == binding.run_id)
                        .map(|run| {
                            run.deliveries
                                .iter()
                                .filter(|delivery| {
                                    delivery.target == id
                                        && delivery.status
                                            == ide_core::delegation::DeliveryStatus::Queued
                                })
                                .count()
                        })
                })
                .unwrap_or(0);
            if queued > 0 {
                row = row.child(crate::ui::design::indicator::indicator(
                    IconName::Loader,
                    format!(
                        "{queued} queued message{}",
                        if queued == 1 { "" } else { "s" }
                    ),
                    crate::ui::design::amber(cx),
                    cx,
                ));
            }
        }
        if let Some(selection) = self.delegation_selection.get(&id).copied() {
            row = row.child(self.render_delegation_recipient(id, selection, cx));
        }
        if let Some(chip) = self.render_delegation_activity_chip(id, cx) {
            row = row.child(chip);
        }
        let stopped_run = cx.try_global::<DelegationHandle>().and_then(|handle| {
            handle
                .0
                .read(cx)
                .runs
                .iter()
                .find(|run| run.parent_agent_id == id && run.status.stopped())
                .map(|run| run.id)
        });
        if let Some(run_id) = stopped_run {
            row = row.child(self.render_delegation_recovery(run_id, id, "composer", cx));
        }
        row.into_any_element()
    }

    pub(super) fn render_delegation_panel_error(
        &self,
        parent: Uuid,
        cx: &App,
    ) -> Option<gpui::AnyElement> {
        self.agent_start_errors.get(&parent).map(|error| {
            div()
                .w_full()
                .whitespace_normal()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::rose(cx))
                .child(error.clone())
                .into_any_element()
        })
    }

    pub(super) fn render_delegation_recovery(
        &self,
        run_id: Uuid,
        parent: Uuid,
        surface: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(
                crate::ui::style::primary_button_compact(
                    SharedString::from(format!("{surface}-resume-delegation-{run_id}")),
                    "Resume bandmates",
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(h) = cx.try_global::<DelegationHandle>().cloned() {
                        match h.0.update(cx, |s, cx| s.resume(run_id, cx)) {
                            Ok(()) => {
                                this.agent_start_errors.remove(&parent);
                            }
                            Err(error) => {
                                this.agent_start_errors.insert(parent, error.to_string());
                            }
                        }
                    }
                    cx.notify();
                })),
            )
            .child(
                crate::ui::style::dialog_neutral_button(
                    SharedString::from(format!("{surface}-end-delegation-{run_id}")),
                    "End delegation · Keep files",
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(h) = cx.try_global::<DelegationHandle>().cloned() {
                        match h.0.update(cx, |s, cx| s.end_run(run_id, cx)) {
                            Ok(()) => {
                                this.agent_start_errors.remove(&parent);
                            }
                            Err(error) => {
                                this.agent_start_errors.insert(parent, error.to_string());
                            }
                        }
                    }
                    cx.notify();
                })),
            )
            .into_any_element()
    }

    /// A second entry point for the same Band panel, outside the composer.
    /// It follows the coordinator's display state, never the lead's runtime.
    pub(super) fn render_band_header_toggle(
        &self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !ide_core::delegation::enabled()
            || agent.runtime != AgentRuntimeKind::Chat
            || agent
                .delegation
                .as_ref()
                .is_some_and(|b| b.task_id.is_some())
        {
            return None;
        }
        let parent = agent.id;
        let indicator = self.delegation_indicator(parent, cx)?;
        let active = self.delegated_overview == Some(parent)
            || self.delegated_panel.is_some_and(|id| {
                self.agents.read(cx).agent(id).is_some_and(|child| {
                    child
                        .delegation
                        .as_ref()
                        .is_some_and(|b| b.parent_agent_id == parent)
                })
            });
        let leading = delegation_glyph(
            indicator.activity,
            indicator.status,
            parent.as_u128() as usize,
            cx,
        );
        Some(
            crate::ui::style::header_activity_toggle_button(
                ("agent-band-toggle", parent.as_u128() as u64),
                leading,
                SharedString::from(indicator.header_label().to_owned()),
                active,
                cx,
            )
            .tooltip(format!(
                "{} — {} Band sidebar",
                indicator.label,
                if active { "Hide" } else { "Open" }
            ))
            .on_click(cx.listener(move |this, _, window, cx| {
                if active {
                    this.web_host.update(cx, |h, _| h.set_intent(None));
                    this.delegated_overview = None;
                    this.delegated_panel = None;
                    this.delegated_preview = false;
                    cx.notify();
                } else {
                    this.open_assignment_overview(parent, window, cx);
                }
            }))
            .into_any_element(),
        )
    }

    /// The aggregate Expert status. Clicking always opens all assignments.
    fn render_delegation_activity_chip(
        &self,
        parent: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let indicator = self.delegation_indicator(parent, cx)?;
        let leading = delegation_glyph(
            indicator.activity,
            indicator.status,
            parent.as_u128() as usize,
            cx,
        );
        Some(
            crate::ui::style::delegation_activity_chip(
                ("experts-status", parent.as_u128() as u64),
                leading,
                indicator.label,
                indicator.live,
                cx,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_assignment_overview(parent, window, cx);
            }))
            .into_any_element(),
        )
    }
}
