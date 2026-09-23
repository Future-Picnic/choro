//! The delegation surfaces: the assignments overview (a chooser that opens
//! the exact child conversation) and an Expert's completion summary.
//!
//! Both open only on an explicit click, never on activity, and both read the
//! current durable run on every render. Nothing here summarises text; the completion
//! dialog shows the Expert's own structured report, bounded at first sight.
use super::experts_cards::delegation_glyph;
use super::*;
use crate::state::delegation::display::{self, AssignmentEntry, DelegationActivity, ReportState};
use crate::state::delegation::DelegationHandle;
use gpui::{Div, WeakEntity};
use gpui_component::WindowExt;

/// Characters of the outcome shown before "Show full report".
const SUMMARY_PREVIEW_CHARS: usize = 420;
/// List items shown per section before "Show full report".
const LIST_PREVIEW_ITEMS: usize = 4;

/// Current report data, derived again when durable state changes.
#[derive(Clone)]
struct CompletionSummary {
    expert: String,
    bandmate_index: usize,
    model: String,
    goal: String,
    stage: &'static str,
    report: ReportState,
    summary: String,
    addressed: Vec<String>,
    unresolved: Vec<String>,
    checks: Vec<String>,
    lead_verification: Option<String>,
    child_agent_id: Option<Uuid>,
}

/// The dialog keeps assignment identity and presentation state; the report stays live.
pub(super) struct CompletionSummaryDialog {
    run_id: Uuid,
    task_id: Uuid,
    expanded: bool,
}

impl Render for CompletionSummaryDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(data) = completion_summary(self.run_id, self.task_id, cx) else {
            return div().p_3().text_size(crate::ui::design::text_ui())
                .child("The updated report is still being prepared. The conversation remains available.").into_any_element();
        };
        let expanded = self.expanded;
        let (summary, summary_cut) = if expanded {
            (data.summary.trim().to_string(), false)
        } else {
            display::bounded_text(&data.summary, SUMMARY_PREVIEW_CHARS)
        };
        let hidden_items = |items: &[String]| {
            if expanded {
                0
            } else {
                items.len().saturating_sub(LIST_PREVIEW_ITEMS)
            }
        };
        let more = summary_cut
            || hidden_items(&data.addressed) > 0
            || hidden_items(&data.unresolved) > 0
            || hidden_items(&data.checks) > 0
            || [&data.addressed, &data.unresolved, &data.checks]
                .into_iter()
                .flatten()
                .any(|v| display::bounded_text(v, 240).1)
            || data
                .lead_verification
                .as_ref()
                .is_some_and(|v| display::bounded_text(v, 240).1);
        let stage_color = if data.stage == "Task complete" {
            crate::ui::design::sage(cx)
        } else {
            crate::ui::design::amber(cx)
        };

        v_flex()
            .id("expert-summary-body")
            .w_full()
            .max_h(px(520.))
            .overflow_y_scroll()
            .gap_3()
            .pt_1()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(crate::ui::design::indicator::bandmate_icon(
                                data.bandmate_index,
                                crate::ui::design::amber(cx),
                                crate::ui::design::icon_md(),
                            ))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(data.expert.clone()),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child(data.model.clone()),
                            ),
                    )
                    .child(
                        div()
                            .whitespace_normal()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.45))
                            .text_color(crate::ui::design::t2(cx))
                            .child(data.goal.clone()),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(crate::ui::design::indicator::status(
                        data.stage,
                        stage_color,
                        cx,
                    ))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(match &data.lead_verification {
                                Some(text) if !text.trim().is_empty() => {
                                    format!(
                                        "Lead verification: {}",
                                        if expanded {
                                            text.trim().to_owned()
                                        } else {
                                            display::bounded_text(text, 240).0
                                        }
                                    )
                                }
                                _ => "Lead verification: not yet reported".to_string(),
                            }),
                    )
                    .when(data.report == ReportState::Stale, |col| {
                        col.child(
                            div()
                                .whitespace_normal()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::amber(cx))
                                .child(
                                    "This report answers an earlier wording of the assignment. \
                                     The Bandmate has since been corrected.",
                                ),
                        )
                    }),
            )
            .child(report_section(
                "Outcome reported by the bandmate",
                vec![summary],
                None,
                cx,
            ))
            .child(report_section(
                "Requirements addressed",
                preview(&data.addressed, expanded),
                Some("Nothing listed"),
                cx,
            ))
            .child(report_section(
                "Unfinished or blocked",
                preview(&data.unresolved, expanded),
                Some("Nothing reported as unfinished"),
                cx,
            ))
            .child(report_section(
                "Checks performed",
                preview(&data.checks, expanded),
                Some("No checks reported"),
                cx,
            ))
            .when(more || expanded, |col| {
                col.child(
                    h_flex().child(
                        crate::ui::style::dialog_neutral_button(
                            "expert-summary-expand",
                            if expanded {
                                "Show less"
                            } else {
                                "Show full report"
                            },
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.expanded = !this.expanded;
                            cx.notify();
                        })),
                    ),
                )
            })
            .into_any_element()
    }
}

fn preview(items: &[String], expanded: bool) -> Vec<String> {
    let shown = if expanded {
        items.len()
    } else {
        items.len().min(LIST_PREVIEW_ITEMS)
    };
    let mut out: Vec<String> = items
        .iter()
        .take(shown)
        .map(|v| {
            if expanded {
                v.clone()
            } else {
                display::bounded_text(v, 240).0
            }
        })
        .collect();
    if items.len() > shown {
        out.push(format!("… and {} more", items.len() - shown));
    }
    out
}

fn report_section(
    title: &'static str,
    items: Vec<String>,
    empty: Option<&'static str>,
    cx: &App,
) -> Div {
    let items: Vec<String> = items.into_iter().filter(|i| !i.trim().is_empty()).collect();
    v_flex()
        .gap_1()
        .child(
            div()
                .text_size(crate::ui::design::text_ui())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t3(cx))
                .child(title),
        )
        .when(items.is_empty(), |col| {
            col.child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t4(cx))
                    .child(empty.unwrap_or("Nothing reported")),
            )
        })
        .children(items.into_iter().map(|item| {
            div()
                .whitespace_normal()
                .text_size(crate::ui::design::text_body())
                .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
                .text_color(crate::ui::design::chat_body(cx))
                .child(item)
        }))
}

impl CenterArea {
    /// The chooser behind the composer's Experts chip: every assignment of
    /// this lead, live ones first, each opening its own conversation.
    pub(super) fn open_assignment_overview(
        &mut self,
        parent: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let _ = window;
        self.web_host.update(cx, |h, _| h.set_intent(None));
        self.delegated_panel = None;
        self.delegated_overview = Some(parent);
        cx.notify();
    }

    pub(super) fn render_assignment_overview_panel(
        &mut self,
        parent: Uuid,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let runs = self.delegation_runs(parent, cx);
        let needs_user = |child: Uuid| self.delegated_child_needs_user(child, cx);
        let mut entries = display::assignment_overview(&runs, parent, &needs_user);
        for entry in &mut entries {
            if !entry.finished && display::expert_is_working(entry.status) {
                if let Some(step) = entry
                    .child_agent_id
                    .and_then(|id| self.agent_chats.read(cx).session(id))
                    .and_then(|s| s.work_log.last())
                {
                    entry.detail = step.title.clone();
                }
            }
        }
        let center = cx.entity().downgrade();
        let live: Vec<_> = entries.iter().filter(|e| !e.finished).collect();
        let finished: Vec<_> = entries.iter().filter(|e| e.finished).collect();
        v_flex()
            .size_full()
            .min_w(px(0.))
            .justify_start()
            .text_left()
            .gap_2()
            .child(
                h_flex()
                    .px_4()
                    .py_2()
                    .gap_2()
                    .child(
                        Icon::new(IconName::Bot)
                            .size(crate::ui::design::icon_md())
                            .text_color(crate::ui::design::amber(cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Band"),
                    )
                    .child(
                        crate::ui::style::header_icon_button(
                            "expert-overview-close",
                            IconName::Close,
                            cx,
                        )
                        .tooltip("Close band · Return to conversation")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.delegated_overview = None;
                            this.delegated_panel = None;
                            cx.notify();
                        })),
                    ),
            )
            .child(
                v_flex()
                    .id("expert-overview-body")
                    .w_full()
                    .flex_1()
                    .min_h(px(0.))
                    .justify_start()
                    .overflow_y_scroll()
                    .px_2()
                    .gap_1()
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!("Active · {}", live.len())),
                    )
                    .when(live.is_empty(), |col| {
                        col.child(
                            div()
                                .px_2()
                                .py_2()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t4(cx))
                                .child("No active assignments"),
                        )
                    })
                    .children(live.into_iter().map(|e| {
                        let control = runs.iter().find(|r| r.id == e.run_id).and_then(|run| {
                            run.tasks
                                .iter()
                                .find(|t| t.id == e.task_id)
                                .and_then(|task| {
                                    self.render_delegation_task_control(
                                        run,
                                        task,
                                        "band-overview",
                                        cx,
                                    )
                                })
                        });
                        v_flex()
                            .w_full()
                            .min_w(px(0.))
                            .child(overview_row(e, false, center.clone(), cx))
                            .when_some(control, |row, control| {
                                row.child(h_flex().pl(px(32.)).pb_1().child(control))
                            })
                    }))
                    .child(
                        div()
                            .px_2()
                            .pt_3()
                            .pb_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!("Finished · {}", finished.len())),
                    )
                    .children(
                        finished
                            .into_iter()
                            .map(|e| overview_row(e, true, center.clone(), cx)),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .px_4()
                    .pb_2()
                    .children(runs.iter().filter(|run| run.status.stopped()).map(|run| {
                        self.render_delegation_recovery(run.id, parent, "band-overview", cx)
                    }))
                    .children(self.render_delegation_panel_error(parent, cx)),
            )
            .into_any_element()
    }

    /// The Expert's own completion report for exactly this assignment.
    pub(super) fn open_completion_summary(
        &mut self,
        run_id: Uuid,
        task_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(data) = completion_summary(run_id, task_id, cx) else {
            return;
        };
        let body = cx.new(|cx| {
            if let Some(handle) = cx.try_global::<DelegationHandle>().cloned() {
                cx.observe(&handle.0, |_, _, cx| cx.notify()).detach();
            }
            CompletionSummaryDialog {
                run_id,
                task_id,
                expanded: false,
            }
        });
        let center = cx.entity().downgrade();
        let title = "Bandmate summary";
        let badge_color = crate::ui::design::amber(cx);
        window.open_dialog(cx, move |dialog, _, cx| {
            let center = center.clone();
            let child = data.child_agent_id;
            dialog
                .w(px(560.))
                .overlay_closable(true)
                .title(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(crate::ui::confirm::icon_badge(
                            IconName::CircleCheck,
                            badge_color,
                            cx,
                        ))
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title)),
                )
                .child(body.clone())
                .footer(move |_, _, _, cx| {
                    let mut actions = vec![];
                    if let Some(child) = child {
                        let center = center.clone();
                        actions.push(
                            crate::ui::style::dialog_neutral_button(
                                "expert-summary-open",
                                "Open conversation",
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = center.update(cx, |this, cx| {
                                    let child = completion_summary(run_id, task_id, cx)
                                        .and_then(|data| data.child_agent_id)
                                        .unwrap_or(child);
                                    this.open_delegated_task(child, window, cx);
                                });
                            }),
                        );
                    }
                    actions.push(
                        crate::ui::style::dialog_neutral_button(
                            "expert-summary-close",
                            "Close",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    );
                    actions
                })
        });
    }
}

fn completion_summary(run_id: Uuid, task_id: Uuid, cx: &App) -> Option<CompletionSummary> {
    let run = cx
        .try_global::<DelegationHandle>()?
        .0
        .read(cx)
        .runs
        .iter()
        .find(|r| r.id == run_id)?;
    let task = run.task(task_id).ok()?;
    let report = display::report_state(task);
    if report == ReportState::None {
        return None;
    }
    let result = task.attempt()?.result.as_ref()?;
    Some(CompletionSummary {
        expert: task.expert.profile.name.clone(),
        bandmate_index: display::bandmate_index(run, task_id)?,
        model: task.expert.profile.model.label().to_string(),
        goal: task.plan.goal.clone(),
        stage: if report == ReportState::Stale {
            "Earlier report"
        } else {
            display::completion_stage(run.status, task.status).unwrap_or(task.status.label())
        },
        report,
        summary: result.summary.clone(),
        addressed: result.addressed.clone(),
        unresolved: result.unresolved.clone(),
        checks: result.checks.clone(),
        lead_verification: run
            .verification
            .clone()
            .filter(|_| run.status == ide_core::delegation::RunStatus::Completed),
        child_agent_id: task.attempt().map(|a| a.child_agent_id),
    })
}

fn overview_row(
    entry: &AssignmentEntry,
    finished: bool,
    center: WeakEntity<CenterArea>,
    cx: &App,
) -> gpui::AnyElement {
    let child = entry.child_agent_id;
    let run_id = entry.run_id;
    let task_id = entry.task_id;
    let status = match entry.activity {
        DelegationActivity::Attention => "Needs your input",
        DelegationActivity::Paused => "Paused",
        _ => entry.stage.unwrap_or(entry.status.label()),
    };
    let summary_center = center.clone();
    let content = h_flex()
        .w_full()
        .min_w(px(0.))
        .items_start()
        .gap_2()
        .child(crate::ui::design::indicator::bandmate_icon(
            entry.bandmate_index,
            if finished {
                crate::ui::design::t3(cx)
            } else {
                crate::ui::design::amber(cx)
            },
            crate::ui::design::icon_md(),
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap_1()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(crate::ui::design::text_body())
                        .text_color(if finished {
                            crate::ui::design::t2(cx)
                        } else {
                            crate::ui::design::t1(cx)
                        })
                        .child(entry.goal.clone()),
                )
                .when(!entry.detail.is_empty(), |col| {
                    col.child(crate::ui::style::delegation_activity_preview(
                        &entry.detail,
                        cx,
                    ))
                })
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            div()
                                .text_color(crate::ui::design::t4(cx))
                                .child(format!("{} · {}", entry.label, entry.model)),
                        )
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .text_color(if entry.activity == DelegationActivity::Attention {
                                    crate::ui::design::amber(cx)
                                } else {
                                    crate::ui::design::t3(cx)
                                })
                                .child(delegation_glyph(
                                    entry.activity,
                                    entry.status,
                                    task_id.as_u128() as usize,
                                    cx,
                                ))
                                .child(status),
                        ),
                ),
        )
        .child(
            Icon::new(IconName::ChevronRight)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t4(cx)),
        )
        .into_any_element();
    v_flex()
        .w_full()
        .min_w(px(0.))
        .gap_0p5()
        .child(
            crate::ui::style::delegation_row_button(
                ("expert-overview-row", task_id.as_u128() as u64),
                content,
                cx,
            )
            .disabled(child.is_none())
            .tooltip(entry.goal.clone())
            .on_click(move |_, window, cx| {
                if let Some(child) = child {
                    let _ =
                        center.update(cx, |this, cx| this.open_delegated_task(child, window, cx));
                }
            }),
        )
        .when(entry.report != ReportState::None, |row| {
            row.child(
                h_flex().pl(px(30.)).pb_1().child(
                    crate::ui::style::delegation_card_action_button(
                        ("expert-overview-summary", task_id.as_u128() as u64),
                        if entry.report == ReportState::Stale {
                            "Earlier report"
                        } else {
                            "View summary"
                        },
                        cx,
                    )
                    .on_click(move |_, window, cx| {
                        let _ = summary_center.update(cx, |this, cx| {
                            this.open_completion_summary(run_id, task_id, window, cx)
                        });
                    }),
                ),
            )
        })
        .into_any_element()
}
