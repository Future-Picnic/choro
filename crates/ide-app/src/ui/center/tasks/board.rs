#![allow(dead_code, reason = "retained alternate task-board presentation")]

use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn render_tasks_header(
        &self,
        project: ProjectId,
        board: &ide_core::TaskBoard,
        loading: bool,
        has_selection: bool,
        detail_open: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use crate::ui::design::header;
        header::bar(cx)
            .child(
                header::title_col(cx)
                    .child(header::title(
                        SharedString::from(board.board_name.clone()),
                        cx,
                    ))
                    .child(header::subtitle(
                        SharedString::from(task_board_header_subtitle(board)),
                        cx,
                    )),
            )
            .child(
                header::actions()
                    .when(has_selection, |header| {
                        header.child(
                            style::secondary_button_compact(
                                "tasks-view-toggle",
                                if detail_open { "Board" } else { "Task" },
                            )
                            .icon(if detail_open {
                                IconName::CircleCheck
                            } else {
                                IconName::BookOpen
                            })
                            .tooltip(if detail_open {
                                "Board overview"
                            } else {
                                "Task detail"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.tasks_detail_collapsed = !this.tasks_detail_collapsed;
                                cx.notify();
                            })),
                        )
                    })
                    .when(
                        board.provider == ide_core::IssueTrackerProvider::Personal,
                        |header| {
                            header.child(
                                style::accent_button_compact("tasks-new-personal", "New Task", cx)
                                    .icon(IconName::Plus)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_personal_task_editor(project, None, window, cx);
                                    })),
                            )
                        },
                    )
                    .child(
                        style::secondary_button_compact("tasks-configure", "Boards")
                            .icon(IconName::Settings)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                TaskTrackerConnectionsEditor::open(
                                    this.workspace.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        style::refresh_button(
                            "tasks-refresh",
                            if loading { "Loading" } else { "Refresh" },
                            cx,
                        )
                        .disabled(loading)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.refresh_task_board(project, cx);
                        })),
                    )
                    .child(
                        style::secondary_button_compact("tasks-close", "Close")
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_code(cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_task_board(
        &mut self,
        project: ProjectId,
        board: &ide_core::TaskBoard,
        selected: Option<&TaskSummary>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let columns = task_board_columns(board);
        if board.issues.is_empty() {
            return style::empty_state(
                IconName::CircleCheck,
                "No tasks on this board",
                "Refresh after the board has active work.",
                cx,
            )
            .into_any_element();
        }

        div()
            .size_full()
            .overflow_x_scrollbar()
            .p_4()
            .child(
                h_flex()
                    .items_start()
                    .gap_3()
                    .children(columns.iter().enumerate().map(|(index, column)| {
                        let tasks = board
                            .issues
                            .iter()
                            .filter(|task| task.column == *column)
                            .cloned()
                            .collect::<Vec<_>>();
                        self.render_task_column(project, index, column, tasks, selected, cx)
                    })),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_task_column(
        &mut self,
        project: ProjectId,
        index: usize,
        column: &str,
        tasks: Vec<TaskSummary>,
        selected: Option<&TaskSummary>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let accent = task_column_accent(column, cx);
        let count = tasks.len();
        v_flex()
            .id(("task-column", index))
            .w(px(304.))
            .min_h(px(120.))
            .flex_none()
            .gap_2p5()
            .p_2p5()
            .rounded(px(style::RADIUS_LG))
            .border_1()
            .border_color(style::border(cx))
            .bg(style::surface(cx))
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .min_w(px(0.))
                            .child(div().size(px(7.)).flex_none().rounded_full().bg(accent))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .truncate()
                                    .child(SharedString::from(column.to_string())),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .min_w(px(20.))
                            .px_1p5()
                            .py(px(1.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(crate::ui::design::surface(cx))
                            .border_1()
                            .border_color(style::border(cx))
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child(count.to_string()),
                    ),
            )
            .child(v_flex().gap_2().children(tasks.into_iter().enumerate().map(
                |(task_index, task)| {
                    let is_selected = selected
                        .is_some_and(|selected| selected.reference.same_issue(&task.reference));
                    self.render_task_card(project, index, task_index, task, is_selected, cx)
                },
            )))
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_task_card(
        &mut self,
        project: ProjectId,
        column_index: usize,
        task_index: usize,
        task: TaskSummary,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let reference = task.reference.clone();
        let accent = task_status_accent(&task, cx);
        let linked_agent = self
            .agents
            .read(cx)
            .active_task_implementor(project, &task.reference);
        if let Some(agent) = linked_agent.as_ref() {
            if !self.agent_ship_pr_targets.contains_key(&agent.id) {
                if let (Some(repo_path), Some(branch)) = (
                    agent.ship_pr_repo_path.clone(),
                    agent.ship_pr_branch.clone(),
                ) {
                    self.track_agent_ship_pr_branch(agent.id, repo_path, branch, cx);
                }
            }
            self.sync_agent_ship_pull_request(agent.id, cx);
        }
        let linked_ship = linked_agent.as_ref().and_then(|agent| {
            if let Some(pr) = self.agent_ship_prs.get(&agent.id) {
                let (status, accent) = crate::ui::git::git_panel::pull_request_status_style(pr, cx);
                Some((
                    SharedString::from(format!("PR #{} {}", pr.number, pr.title)),
                    Some(status),
                    Some(pr.url.clone()),
                    accent,
                ))
            } else {
                agent.ship_pr_branch.as_ref().map(|branch| {
                    (
                        SharedString::from(format!("PR branch {branch}")),
                        None,
                        None,
                        crate::ui::design::t3(cx),
                    )
                })
            }
        });
        v_flex()
            .id(("task-card", column_index * 1000 + task_index))
            .gap_2()
            .p_3()
            .rounded(px(style::RADIUS_LG))
            .border_1()
            .border_l_2()
            .border_color(if selected {
                accent.opacity(0.55)
            } else {
                style::border(cx)
            })
            .bg(if selected {
                accent.opacity(0.10)
            } else {
                crate::ui::design::base(cx)
            })
            .cursor_pointer()
            .hover(|card| {
                card.bg(if selected {
                    accent.opacity(0.14)
                } else {
                    crate::ui::design::surface(cx).opacity(0.5)
                })
            })
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tasks_detail_collapsed = false;
                this.tasks.update(cx, |tasks, cx| {
                    tasks.select_task(project, reference.clone(), cx)
                });
            }))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_ui())
                            .text_color(accent)
                            .child(SharedString::from(task.reference.issue_key.clone())),
                    )
                    .when_some(task.issue_type.clone(), |row, issue_type| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(SharedString::from(issue_type)),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .line_height(gpui::relative(1.28))
                    .text_color(crate::ui::design::t1(cx))
                    .child(SharedString::from(task.reference.title.clone())),
            )
            .child(
                h_flex().gap_1().flex_wrap().children(
                    task_card_meta(&task)
                        .into_iter()
                        .map(|label| style::tag(SharedString::from(label), cx).into_any_element()),
                ),
            )
            .when_some(linked_agent, |card, agent| {
                card.child(
                    h_flex()
                        .id(("task-card-agent-link", column_index * 1000 + task_index))
                        .gap_1p5()
                        .items_center()
                        .rounded(px(style::RADIUS_SM))
                        .border_1()
                        .border_color(crate::ui::design::accent(cx).opacity(0.28))
                        .bg(crate::ui::design::accent(cx).opacity(0.08))
                        .px_2()
                        .py_1()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t1(cx))
                        .child(
                            Icon::new(IconName::Bot)
                                .size(crate::ui::design::icon_md())
                                .text_color(crate::ui::design::accent(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .child(SharedString::from(agent.title)),
                        ),
                )
            })
            .when_some(linked_ship, |card, (label, status, url, accent)| {
                let chip_id = ("task-card-ship-link", column_index * 1000 + task_index);
                if let Some(url) = url {
                    card.child(
                        h_flex()
                            .id(chip_id)
                            .gap_1p5()
                            .items_center()
                            .rounded(px(style::RADIUS_SM))
                            .border_1()
                            .border_color(accent.opacity(0.32))
                            .bg(accent.opacity(0.08))
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t1(cx))
                            .cursor_pointer()
                            .hover(move |chip| chip.bg(accent.opacity(0.15)))
                            .on_click(move |_, _, _| {
                                crate::ui::git::git_panel::open_url(&url);
                            })
                            .child(pr_icon(accent))
                            .child(div().flex_1().min_w(px(0.)).truncate().child(label))
                            .when_some(status, |chip, status| {
                                chip.child(
                                    div()
                                        .flex_none()
                                        .text_color(accent)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(status),
                                )
                            }),
                    )
                } else {
                    card.child(
                        h_flex()
                            .id(chip_id)
                            .gap_1p5()
                            .items_center()
                            .rounded(px(style::RADIUS_SM))
                            .border_1()
                            .border_color(accent.opacity(0.32))
                            .bg(accent.opacity(0.08))
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t1(cx))
                            .child(pr_icon(accent))
                            .child(div().flex_1().min_w(px(0.)).truncate().child(label))
                            .when_some(status, |chip, status| {
                                chip.child(
                                    div()
                                        .flex_none()
                                        .text_color(accent)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child(status),
                                )
                            }),
                    )
                }
            })
            .into_any_element()
    }
}
