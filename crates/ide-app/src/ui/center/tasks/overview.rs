use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn render_tasks_section(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let configured = self
            .tasks
            .read(cx)
            .configured_connection(project, cx)
            .filter(|connection| {
                connection.provider == ide_core::IssueTrackerProvider::Personal
                    || connection.has_selected_source()
            });
        if configured.is_none() {
            return style::empty_state(
                IconName::CircleCheck,
                "No task board selected",
                "Add or select a task source from the right sidebar.",
                cx,
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        style::secondary_button("configure-jira-empty", "Add Connection")
                            .icon(IconName::Plus)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                TaskTrackerConnectionsEditor::open_add(
                                    this.workspace.clone(),
                                    window,
                                    cx,
                                );
                            })),
                    )
                    .child(
                        style::secondary_button("tasks-empty-close", "Close")
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_code(cx);
                            })),
                    ),
            )
            .into_any_element();
        }

        let loading = self.tasks.read(cx).board_loading(project);
        let board = self.tasks.read(cx).board(project);
        let error = self.tasks.read(cx).board_error(project);

        if board.is_none() && !loading && error.is_none() {
            self.refresh_task_board(project, cx);
        }

        if board.is_none() && loading {
            return style::loading_state(
                "Loading tasks",
                "Fetching the selected board.",
                project.0.as_u128() as usize,
                cx,
            )
            .child(
                style::secondary_button("tasks-loading-close", "Close")
                    .icon(IconName::Close)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_code(cx);
                    })),
            )
            .into_any_element();
        }

        if let Some(error) = error.filter(|_| board.is_none()) {
            return style::empty_state(
                IconName::CircleCheck,
                "Could not load tasks",
                SharedString::from(error),
                cx,
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        style::refresh_button("tasks-error-refresh", "Refresh", cx).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.refresh_task_board(project, cx);
                            }),
                        ),
                    )
                    .child(
                        style::secondary_button("tasks-error-configure", "Edit Boards")
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
                        style::secondary_button("tasks-error-close", "Close")
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_code(cx);
                            })),
                    ),
            )
            .into_any_element();
        }

        let Some(board) = board else {
            return style::empty_state(
                IconName::CircleCheck,
                "No tasks on this board",
                "Refresh after this board has active work.",
                cx,
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        style::refresh_button("tasks-empty-refresh", "Refresh", cx).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.refresh_task_board(project, cx);
                            }),
                        ),
                    )
                    .child(
                        style::secondary_button("tasks-no-board-close", "Close")
                            .icon(IconName::Close)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_code(cx);
                            })),
                    ),
            )
            .into_any_element();
        };

        let selected = self.tasks.read(cx).selected_summary(project);
        if let Some(summary) = selected.clone() {
            let reference = summary.reference.clone();
            self.tasks
                .update(cx, |tasks, cx| tasks.ensure_detail(project, reference, cx));
        }

        let has_selection = selected.is_some();
        let show_detail = has_selection && !self.tasks_detail_collapsed;

        let content = if show_detail {
            let detail = self.render_task_detail(project, selected.clone(), window, cx);
            div()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .overflow_hidden()
                .bg(crate::ui::design::base(cx))
                .child(detail)
                .into_any_element()
        } else {
            self.render_board_overview(project, &board, loading, selected.as_ref(), cx)
        };

        v_flex()
            .size_full()
            .overflow_hidden()
            .child(content)
            .into_any_element()
    }

    /// Board overview: the whole board as a status-grouped list (design's `.brow`).
    pub(in crate::ui::center) fn render_board_overview(
        &mut self,
        project: ProjectId,
        board: &ide_core::TaskBoard,
        loading: bool,
        selected: Option<&TaskSummary>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected_ref = selected.map(|summary| summary.reference.clone());
        let top = h_flex()
            .w_full()
            .items_center()
            .gap_2p5()
            .px_5()
            .py_2p5()
            .child(crate::ui::tasks_panel::provider_badge(
                board.provider,
                20.,
                cx,
            ))
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .items_center()
                    .gap_1p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .truncate()
                            .text_color(style::focus_text(cx))
                            .child(SharedString::from(board.board_name.clone())),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(SharedString::from(format!(
                                "· {} issues",
                                board.issues.len()
                            ))),
                    ),
            )
            .child(
                style::refresh_button(
                    "board-overview-refresh",
                    if loading { "Loading" } else { "Refresh" },
                    cx,
                )
                .disabled(loading)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.refresh_task_board(project, cx);
                })),
            );

        if board.provider == ide_core::IssueTrackerProvider::Personal && board.issues.is_empty() {
            let empty = v_flex()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .w(px(160.))
                        .h(px(110.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::ui::illustrations::illustration(
                                crate::ui::illustrations::Illustration::Tasks,
                                cx,
                            )
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_title())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child("Create your first task"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Add a task to start organizing your Personal Board."),
                        ),
                )
                .child(
                    style::secondary_button("tasks-empty-new-personal-center", "New task")
                        .icon(IconName::Plus)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open_personal_task_editor(project, None, window, cx);
                        })),
                );

            return v_flex()
                .flex_1()
                .min_h(px(0.))
                .w_full()
                .overflow_hidden()
                .bg(crate::ui::design::base(cx))
                .child(top)
                .child(empty)
                .into_any_element();
        }

        let mut list = v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_y_scrollbar();
        let inner = v_flex().w_full().max_w(px(760.)).px_5().py_4().gap_0();
        let mut inner = inner;
        for column in &board.columns {
            let issues = board
                .issues
                .iter()
                .filter(|issue| issue.column == column.name)
                .collect::<Vec<_>>();
            if issues.is_empty() {
                continue;
            }
            inner = inner.child(
                div()
                    .mt_4()
                    .mb_1p5()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t3(cx))
                    .child(SharedString::from(format!(
                        "{} · {}",
                        column.name,
                        issues.len()
                    ))),
            );
            for issue in issues {
                let reference = issue.reference.clone();
                let is_selected = selected_ref
                    .as_ref()
                    .is_some_and(|current| current.same_issue(&reference));
                let dot = crate::ui::tasks_panel::task_status_color(issue, cx);
                let title = issue.reference.title.clone();
                let priority = issue.priority.clone();
                let assignee = issue.assignee.clone();
                let indicators = self.render_task_indicators(project, &issue.reference, cx);

                inner = inner.child(
                    h_flex()
                        .id(SharedString::from(format!(
                            "brow-{}",
                            issue.reference.issue_id
                        )))
                        .w_full()
                        .items_center()
                        .gap_2p5()
                        .px_3()
                        .py_2()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .when(is_selected, |row| row.bg(crate::ui::design::surface(cx)))
                        .hover(|row| row.bg(crate::ui::design::hover(cx)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.tasks_detail_collapsed = false;
                            this.tasks.update(cx, |tasks, cx| {
                                tasks.select_task(project, reference.clone(), cx);
                                tasks.ensure_detail(project, reference.clone(), cx);
                            });
                            cx.notify();
                        }))
                        .child(div().size(px(6.)).rounded_full().bg(dot))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(crate::ui::design::text_body())
                                .truncate()
                                .text_color(style::focus_text(cx))
                                .child(SharedString::from(title)),
                        )
                        .children(indicators)
                        .when_some(priority, |row, priority| {
                            row.child(priority_pill(&priority, cx))
                        })
                        .when_some(assignee, |row, assignee| row.child(avatar_badge(&assignee))),
                );
            }
        }
        list = list.child(h_flex().w_full().justify_center().child(inner));

        v_flex()
            .flex_1()
            .min_h(px(0.))
            .w_full()
            .overflow_hidden()
            .bg(crate::ui::design::base(cx))
            .child(top)
            .child(list)
            .into_any_element()
    }
}
