//! The delegation card in a lead's conversation: one `chat_card` per run, one
//! row per assignment. State colour lives on the row glyph (amber while an
//! Expert works or needs someone, sage when integrated, rose on failure); the
//! text stays neutral, as everywhere else in the chat.
use super::*;
use crate::state::delegation::display::{self, DelegationActivity};
use crate::state::delegation::DelegationHandle;
use ide_core::delegation::{DelegationRun, DelegationTask, RunStatus, TaskStatus};

/// The status glyph for an assignment or a run: a live amber spinner while
/// working, an amber dot when a person is needed, a muted pause mark, and the
/// finished/failed marks for settled rows.
pub(super) fn delegation_glyph(
    activity: DelegationActivity,
    status: TaskStatus,
    seed: usize,
    cx: &App,
) -> gpui::AnyElement {
    let slot = |child: gpui::AnyElement| {
        div()
            .flex_none()
            .size(crate::ui::design::icon_md())
            .flex()
            .items_center()
            .justify_center()
            .child(child)
            .into_any_element()
    };
    // An Expert that has handed in its report is finished from the user's
    // point of view even while the lead still owes it integration.
    if status == TaskStatus::ResultReady
        && matches!(
            activity,
            DelegationActivity::Working | DelegationActivity::Idle
        )
    {
        return slot(
            Icon::new(IconName::CircleCheck)
                .size(crate::ui::design::icon_md())
                .text_color(crate::ui::design::sage(cx))
                .into_any_element(),
        );
    }
    match activity {
        DelegationActivity::Working => slot(crate::ui::logo_spinner::delegation_spinner(
            crate::ui::design::ICON_MD,
            "delegation-spinner",
            seed,
            crate::ui::design::amber(cx),
        )),
        DelegationActivity::Attention => {
            slot(crate::ui::design::indicator::dot(crate::ui::design::amber(cx)).into_any_element())
        }
        DelegationActivity::Paused => slot(
            crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::CirclePause,
                crate::ui::design::t3(cx),
                crate::ui::design::icon_md(),
            )
            .into_any_element(),
        ),
        DelegationActivity::Idle => {
            if matches!(status, TaskStatus::Integrated | TaskStatus::Accepted) {
                return slot(
                    Icon::new(IconName::CircleCheck)
                        .size(crate::ui::design::icon_md())
                        .text_color(crate::ui::design::sage(cx))
                        .into_any_element(),
                );
            }
            let (icon, color) = match status {
                TaskStatus::Failed => (lucide_icons::Icon::CircleX, crate::ui::design::rose(cx)),
                TaskStatus::Cancelled | TaskStatus::Superseded => {
                    (lucide_icons::Icon::CircleX, crate::ui::design::t4(cx))
                }
                _ => (lucide_icons::Icon::CircleDashed, crate::ui::design::t4(cx)),
            };
            slot(
                crate::ui::design::indicator::lucide_icon(
                    icon,
                    color,
                    crate::ui::design::icon_md(),
                )
                .into_any_element(),
            )
        }
    }
}

/// The one-word state shown beside an assignment.
pub(super) fn task_status_label(
    run_status: RunStatus,
    task: &DelegationTask,
    needs_user: bool,
) -> &'static str {
    if run_status == RunStatus::Blocked {
        "Needs attention"
    } else if matches!(run_status, RunStatus::Paused | RunStatus::Interrupted) {
        "Paused"
    } else if needs_user && !task.status.terminal() {
        "Needs your input"
    } else {
        task.status.label()
    }
}

fn run_title(run: &DelegationRun) -> &'static str {
    match run.status {
        RunStatus::Completed => "Delegated work complete",
        RunStatus::Cancelled => "Delegation ended",
        RunStatus::Interrupted => "Delegation interrupted",
        RunStatus::Paused => "Delegation paused",
        RunStatus::Blocked => "Delegation needs attention",
        RunStatus::Preparing => "Preparing assignments",
        RunStatus::Active | RunStatus::Waiting => "Delegated work",
    }
}

fn run_summary(run: &DelegationRun, working: usize) -> Option<String> {
    let total = run.tasks.len();
    if total == 0 {
        return None;
    }
    let noun = if total == 1 {
        "assignment"
    } else {
        "assignments"
    };
    Some(match run.status {
        RunStatus::Completed | RunStatus::Cancelled => format!("{total} {noun}"),
        _ if working > 0 => format!("{total} {noun} · {working} working"),
        _ => format!("{total} {noun}"),
    })
}

impl CenterArea {
    pub(super) fn render_delegation_group(
        &self,
        run: DelegationRun,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let run_id = run.id;
        let parent = run.parent_agent_id;
        let paused = matches!(
            run.status,
            RunStatus::Paused | RunStatus::Interrupted | RunStatus::Blocked
        );
        let needs_user = |child: Uuid| self.delegated_child_needs_user(child, cx);
        let activity = display::run_activity(&run, &needs_user);
        let working = run
            .tasks
            .iter()
            .filter(|t| {
                let asks = t.attempt().is_some_and(|a| needs_user(a.child_agent_id));
                display::task_activity(run.status, t, asks) == DelegationActivity::Working
                    && display::expert_is_working(t.status)
            })
            .count();
        let head_status = if run.status.terminal() {
            if run.status == RunStatus::Completed {
                TaskStatus::Accepted
            } else {
                TaskStatus::Cancelled
            }
        } else {
            TaskStatus::Running
        };

        let mut head = crate::ui::style::chat_card_head(cx)
            .flex_wrap()
            .child(delegation_glyph(
                activity,
                head_status,
                run_id.as_u128() as usize,
                cx,
            ))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child(run_title(&run)),
            )
            .when_some(run_summary(&run, working), |head, summary| {
                head.child(div().text_color(crate::ui::design::t4(cx)).child(summary))
            })
            .child(div().flex_1());
        if !run.status.terminal() {
            head = head.child(
                crate::ui::style::delegation_card_action_button(
                    ("delegation-control", run_id.as_u128() as u64),
                    if paused { "Resume" } else { "Stop" },
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(h) = cx.try_global::<DelegationHandle>().cloned() {
                        let result = h.0.update(cx, |s, cx| {
                            if paused {
                                s.resume(run_id, cx)
                            } else {
                                s.pause(run_id, cx)
                            }
                        });
                        if let Err(e) = result {
                            this.agent_start_errors.insert(parent, e.to_string());
                        }
                    }
                    cx.notify();
                })),
            );
        }

        let mut card = crate::ui::style::chat_card(cx).my_1().child(head);
        if paused {
            card = card.child(
                self.render_delegation_notice(
                    run.pause_reason.clone().unwrap_or_else(|| {
                        "Stopped. Resume to continue, or end delegation and keep the files.".into()
                    }),
                    Some(
                        crate::ui::style::delegation_card_action_button(
                            ("end-delegation", run_id.as_u128() as u64),
                            "End delegation, keep files",
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(h) = cx.try_global::<DelegationHandle>().cloned() {
                                if let Err(e) = h.0.update(cx, |s, cx| s.end_run(run_id, cx)) {
                                    this.agent_start_errors.insert(parent, e.to_string());
                                }
                            }
                            cx.notify();
                        }))
                        .into_any_element(),
                    ),
                    cx,
                ),
            );
        } else if let Some(reason) = run.pause_reason.clone() {
            card = card.child(self.render_delegation_notice(reason, None, cx));
        }
        if run.tasks.is_empty() {
            card = card.child(
                div()
                    .px(crate::ui::design::chat_card_row_pad_x())
                    .py(crate::ui::design::chat_card_row_pad_y())
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child("The lead is choosing Bandmates and writing their assignments."),
            );
        }
        for (index, task) in run.tasks.iter().enumerate() {
            card = card.child(self.render_delegation_task_row(&run, task, index > 0, cx));
        }
        if run.status.terminal() {
            if let Some(cleanup) = self.render_delegation_cleanup(run_id, parent, cx) {
                card = card.child(cleanup);
            }
        }
        card.into_any_element()
    }

    /// A quiet full-width notice inside the card (pause reason, cleanup).
    fn render_delegation_notice(
        &self,
        text: String,
        action: Option<gpui::AnyElement>,
        cx: &App,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .items_start()
            .gap(crate::ui::design::chat_card_row_gap())
            .px(crate::ui::design::chat_card_row_pad_x())
            .py(crate::ui::design::chat_card_row_pad_y())
            .border_b_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::amber(cx).opacity(0.06))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .whitespace_normal()
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.45))
                    .text_color(crate::ui::design::t2(cx))
                    .child(text),
            )
            .children(action)
            .into_any_element()
    }

    fn render_delegation_task_row(
        &self,
        run: &DelegationRun,
        task: &DelegationTask,
        divided: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let run_id = run.id;
        let parent = run.parent_agent_id;
        let task_id = task.id;
        let run_paused = matches!(
            run.status,
            RunStatus::Paused | RunStatus::Interrupted | RunStatus::Blocked
        );
        let child = task.attempt().map(|a| a.child_agent_id);
        let needs_user = child.is_some_and(|id| self.delegated_child_needs_user(id, cx));
        let activity = display::task_activity(run.status, task, needs_user);
        let model_label = child
            .and_then(|id| self.agents.read(cx).agent(id))
            .map(|a| a.model_label().to_string())
            .unwrap_or_else(|| task.expert.profile.model.label().to_string());
        let latest_step = child
            .and_then(|id| self.agent_chats.read(cx).session(id))
            .and_then(|s| s.work_log.last())
            .map(|e| e.title.clone())
            .or_else(|| {
                task.attempt()
                    .map(|a| a.progress.clone())
                    .filter(|p| !p.trim().is_empty())
            });
        let dependency = task.plan.dependencies.join(", ");
        let detail = if task.status == TaskStatus::Queued && !dependency.is_empty() {
            Some(format!("Waiting for {dependency}"))
        } else if display::report_state(task) == display::ReportState::Fresh
            && !display::expert_is_working(task.status)
        {
            task.attempt()
                .and_then(|a| a.result.as_ref())
                .map(|r| display::bounded_text(&r.summary, 180).0)
        } else if activity == DelegationActivity::Working {
            latest_step
        } else {
            None
        };
        let status = display::completion_stage(run.status, task.status)
            .filter(|_| !run_paused)
            .unwrap_or_else(|| task_status_label(run.status, task, needs_user));

        let mut actions = h_flex().flex_none().gap_1().items_center();
        if display::report_state(task) != display::ReportState::None {
            actions = actions.child(
                crate::ui::style::delegation_card_action_button(
                    ("expert-summary", task_id.as_u128() as u64),
                    "View summary",
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_completion_summary(run_id, task_id, window, cx);
                })),
            );
        }
        if let Some(child) = child {
            actions = actions.child(
                crate::ui::style::delegation_card_action_button(
                    ("open-expert", child.as_u128() as u64),
                    "Open",
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_delegated_task(child, window, cx);
                })),
            );
        }
        if !run.status.terminal() && !run_paused && !task.status.terminal() {
            let stopped = task.status == TaskStatus::Paused;
            actions = actions.child(
                crate::ui::style::delegation_card_action_button(
                    ("expert-stop-resume", task_id.as_u128() as u64),
                    if stopped { "Resume" } else { "Stop" },
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(h) = cx.try_global::<DelegationHandle>().cloned() {
                        if let Err(e) = h.0.update(cx, |s, cx| {
                            if stopped {
                                s.resume_task(run_id, task_id, cx)
                            } else {
                                s.pause_task(run_id, task_id, cx)
                            }
                        }) {
                            this.agent_start_errors.insert(parent, e.to_string());
                        }
                    }
                    cx.notify();
                })),
            );
        }

        let mut body = v_flex()
            .flex_1()
            .min_w(px(0.))
            .gap_0p5()
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.))
                    .gap_2()
                    .items_baseline()
                    .flex_wrap()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(task.expert.profile.name.clone()),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t4(cx))
                            .child(model_label),
                    )
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap_1p5()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(if activity == DelegationActivity::Attention {
                                crate::ui::design::amber(cx)
                            } else {
                                crate::ui::design::t3(cx)
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
                div()
                    .whitespace_normal()
                    .text_size(crate::ui::design::text_body())
                    .line_height(gpui::relative(1.4))
                    .text_color(crate::ui::design::t2(cx))
                    .child(task.plan.goal.clone()),
            );
        if let Some(detail) = detail {
            body = body.child(
                div()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(detail),
            );
        }
        if let Some(reason) = task.reason.clone() {
            body = body.child(
                div()
                    .whitespace_normal()
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.4))
                    .text_color(if task.status == TaskStatus::Failed {
                        crate::ui::design::rose(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .child(reason),
            );
        }
        if let Some(confirm) = self.render_deletion_confirmation(run, task, cx) {
            body = body.child(confirm);
        }

        h_flex()
            .w_full()
            .items_start()
            .gap(crate::ui::design::chat_card_row_gap())
            .px(crate::ui::design::chat_card_row_pad_x())
            .py(crate::ui::design::chat_card_row_pad_y())
            .when(divided, |row| {
                row.border_t_1().border_color(crate::ui::design::line(cx))
            })
            .when_some(display::bandmate_index(run, task_id), |row, index| {
                row.child(div().flex_none().pt(px(1.)).child(
                    crate::ui::design::indicator::bandmate_icon(
                        index,
                        crate::ui::design::amber(cx),
                        crate::ui::design::icon_md(),
                    ),
                ))
            })
            .child(body)
            .child(actions)
            .into_any_element()
    }

    fn render_deletion_confirmation(
        &self,
        run: &DelegationRun,
        task: &DelegationTask,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let op = task.integration.as_ref()?;
        if !op.needs_deletion_confirmation() || task.deletion_confirmation == Some(op.id) {
            return None;
        }
        let run_id = run.id;
        let parent = run.parent_agent_id;
        let op_id = op.id;
        let task_id = task.id;
        let paths = op
            .changes
            .iter()
            .filter(|c| c.after.is_none() && c.before.is_some())
            .map(|c| c.path.display().to_string())
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .mt_1()
                .gap_1p5()
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child("This integration removes these files:"),
                )
                .children(paths.into_iter().map(|path| {
                    div()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_size(crate::ui::design::text_file())
                        .text_color(crate::ui::design::t2(cx))
                        .child(path)
                }))
                .child(
                    h_flex().child(
                        crate::ui::style::danger_button_compact(
                            ("confirm-expert-deletions", task_id.as_u128() as u64),
                            "Confirm deletions and integrate",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let result = LocalStore::open_default().and_then(|s| {
                                s.update_delegation(run_id, None, |r| {
                                    anyhow::ensure!(
                                        r.status.dispatchable() && !r.plan_mode,
                                        "Resume this run before integrating."
                                    );
                                    let t = r.task_mut(task_id)?;
                                    anyhow::ensure!(
                                        t.integration.as_ref().is_some_and(|op| op.id == op_id),
                                        "Integration changed. Review the new deletion list."
                                    );
                                    t.deletion_confirmation = Some(op_id);
                                    t.status = TaskStatus::Integrating;
                                    Ok(())
                                })
                            });
                            if let Err(e) = result {
                                this.agent_start_errors.insert(parent, e.to_string());
                            }
                            cx.notify();
                        })),
                    ),
                )
                .into_any_element(),
        )
    }

    fn render_delegation_cleanup(
        &self,
        run_id: Uuid,
        parent: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let paths = LocalStore::open_default()
            .and_then(|s| s.delegation_cleanup_preview(run_id))
            .ok()?;
        if paths.is_empty() {
            return None;
        }
        let listed = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>();
        Some(
            v_flex()
                .w_full()
                .gap_1p5()
                .px(crate::ui::design::chat_card_row_pad_x())
                .py(crate::ui::design::chat_card_row_pad_y())
                .border_t_1()
                .border_color(crate::ui::design::line(cx))
                .child(
                    div()
                        .whitespace_normal()
                        .text_size(crate::ui::design::text_ui())
                        .line_height(gpui::relative(1.45))
                        .text_color(crate::ui::design::t2(cx))
                        .child(
                            "Working copies are kept. Cleanup removes only the folders below. \
                             Conversations, reports and snapshots stay.",
                        ),
                )
                .children(listed.into_iter().map(|path| {
                    div()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_size(crate::ui::design::text_file())
                        .text_color(crate::ui::design::t3(cx))
                        .child(path)
                }))
                .child(
                    h_flex().child(
                        crate::ui::style::danger_button_compact(
                            ("cleanup-experts", run_id.as_u128() as u64),
                            "Remove working copies",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(h) = cx.try_global::<DelegationHandle>().cloned() {
                                if let Err(e) = h.0.update(cx, |s, cx| {
                                    s.cleanup_confirmed(run_id, paths.clone(), cx)
                                }) {
                                    this.agent_start_errors.insert(parent, e.to_string());
                                }
                            }
                            cx.notify();
                        })),
                    ),
                )
                .into_any_element(),
        )
    }
}
