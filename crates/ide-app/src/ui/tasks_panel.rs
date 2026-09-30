mod connection_editor;
mod helpers;

pub use connection_editor::TaskTrackerConnectionsEditor;
use helpers::*;
pub(crate) use helpers::{provider_badge, task_status_color};

use gpui::{
    div, img, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::ButtonVariants,
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    tooltip::Tooltip,
    v_flex, Disableable, Icon, IconName, Sizable, WindowExt,
};
use ide_core::{
    IssueTrackerProvider, JiraUser, TaskTrackerClient, TaskTrackerConnection, TaskTrackerSource,
};
use uuid::Uuid;

use crate::state::{TasksState, Workspace};
use crate::ui::center::CenterArea;

pub struct TasksPanel {
    workspace: Entity<Workspace>,
    tasks: Entity<TasksState>,
    center: gpui::WeakEntity<CenterArea>,
    collapsed_columns: std::collections::HashSet<String>,
}

impl TasksPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        tasks: Entity<TasksState>,
        center: gpui::WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&tasks, |_, _, cx| cx.notify()).detach();
            Self {
                workspace,
                tasks,
                center,
                collapsed_columns: std::collections::HashSet::new(),
            }
        })
    }

    fn toggle_column(&mut self, name: String, cx: &mut Context<Self>) {
        if !self.collapsed_columns.remove(&name) {
            self.collapsed_columns.insert(name);
        }
        cx.notify();
    }

    fn select_task(
        &mut self,
        project: ide_core::ProjectId,
        reference: ide_core::TaskRef,
        cx: &mut Context<Self>,
    ) {
        self.tasks.update(cx, |tasks, cx| {
            tasks.select_task(project, reference.clone(), cx);
            tasks.ensure_detail(project, reference, cx);
        });
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| center.show_task_detail(cx));
        }
    }

    fn new_personal_task(
        &mut self,
        project: ide_core::ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_personal_task_editor(project, None, window, cx);
            });
        }
    }

    #[allow(
        dead_code,
        reason = "retained for a future contextual task-source entry point"
    )]
    fn open_add_source(&self, window: &mut Window, cx: &mut Context<Self>) {
        TaskTrackerConnectionsEditor::open_add(self.workspace.clone(), window, cx);
    }

    fn select_board(
        &self,
        project: ide_core::ProjectId,
        connection_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        self.tasks.update(cx, |tasks, cx| {
            tasks.select_connection(project, connection_id, cx);
            tasks.refresh_project(project, cx);
        });
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| center.show_board_overview(cx));
        }
    }
}

impl Render for TasksPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // "My Tasks" mode: the right sidebar shows all open tasks across every
        // project, grouped by project (a cross-project variant of this nav).
        let my_tasks_view = self
            .center
            .upgrade()
            .map(|center| center.read(cx).is_my_tasks_view())
            .unwrap_or(false);
        if my_tasks_view {
            return self.render_my_tasks_nav(cx);
        }

        let Some(project) = self.workspace.read(cx).active_project().cloned() else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child("Open a project to see tasks")
                .into_any_element();
        };
        let connections = self.tasks.read(cx).connections_for_project(project.id, cx);
        let active_connection = self.tasks.read(cx).active_connection_id(project.id, cx);
        let board = self.tasks.read(cx).board(project.id);
        let loading = self.tasks.read(cx).board_loading(project.id);
        let board_error = self.tasks.read(cx).board_error(project.id);
        let selected_ref = self.tasks.read(cx).selected_ref(project.id);

        let active = connections
            .iter()
            .find(|connection| Some(connection.id) == active_connection)
            .cloned();
        let active_is_personal = active
            .as_ref()
            .map(|connection| connection.provider == IssueTrackerProvider::Personal)
            .unwrap_or(false);
        let active_configured = active
            .as_ref()
            .map(|connection| {
                connection.provider == IssueTrackerProvider::Personal
                    || connection.has_selected_source()
            })
            .unwrap_or(false);

        // --- Canonical panel head + existing refresh/edit-source actions ---
        let head = crate::ui::design::header::panel_bar(cx)
            .child(crate::ui::design::header::panel_identity(None, "Tasks", cx))
            .child(div().flex_1())
            .child(
                crate::ui::style::refresh_icon_button("tasks-refresh", cx)
                    .tooltip("Refresh")
                    .disabled(loading)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tasks.update(cx, |tasks, cx| {
                            tasks.refresh_project(project.id, cx);
                        });
                    })),
            )
            .child(
                crate::ui::style::header_icon_button(
                    "tasks-manage-sources",
                    IconName::Settings,
                    cx,
                )
                .tooltip("Manage task sources")
                .on_click(cx.listener(|this, _, window, cx| {
                    TaskTrackerConnectionsEditor::open(this.workspace.clone(), window, cx);
                })),
            );

        // --- Source switcher: brand pills + add ---
        let switcher = h_flex()
            .w_full()
            .px_2()
            .py_2()
            .gap_1p5()
            .items_center()
            .flex_wrap()
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.5))
            .children(connections.iter().enumerate().map(|(ix, connection)| {
                let selected = active_connection == Some(connection.id);
                let label = if connection.name.trim().is_empty() {
                    connection.provider.label().to_string()
                } else {
                    connection.name.clone()
                };
                let id = connection.id;
                let tip = SharedString::from(board_display_name(connection));
                h_flex()
                    .id(("task-source-chip", ix))
                    .h(px(30.))
                    .pl(px(4.))
                    .pr_2()
                    .gap_1p5()
                    .items_center()
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .cursor_pointer()
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .when(selected, |chip| {
                        chip.bg(crate::ui::design::surface(cx))
                            .border_color(crate::ui::design::line(cx))
                            .text_color(crate::ui::design::t1(cx))
                    })
                    .when(!selected, |chip| {
                        chip.border_color(crate::ui::design::line(cx).opacity(0.))
                            .text_color(crate::ui::design::t3(cx))
                            .hover(|chip| chip.bg(crate::ui::design::hover(cx)))
                    })
                    .child(provider_badge(connection.provider, 22., cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(SharedString::from(label)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_board(project.id, id, cx);
                    }))
            }))
            .child(
                crate::ui::style::header_icon_button("tasks-add-source", IconName::Plus, cx)
                    .tooltip("Add connection")
                    .on_click(cx.listener(|this, _, window, cx| {
                        TaskTrackerConnectionsEditor::open_add(this.workspace.clone(), window, cx);
                    })),
            );

        // --- Body: grouped task list / states ---
        let body = self.render_nav_body(
            project.id,
            board.as_ref(),
            loading,
            board_error.as_deref(),
            active_configured,
            active_is_personal,
            selected_ref.as_ref(),
            cx,
        );

        v_flex()
            .size_full()
            .child(head)
            .child(switcher)
            .child(body)
            .into_any_element()
    }
}

impl TasksPanel {
    /// Cross-project "My Tasks": open (to-do + in-progress) tasks from every
    /// project, grouped under project names. Each row shows its source's brand
    /// icon and — on line two — the status. Selecting one opens its detail in
    /// the middle without leaving this view.
    fn render_my_tasks_nav(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entries = self.tasks.read(cx).my_tasks();
        let loading = self.tasks.read(cx).my_tasks_loading();
        let error = self.tasks.read(cx).my_tasks_error();

        let mut groups: Vec<(ide_core::ProjectId, String, Vec<ide_core::TaskSummary>)> = Vec::new();
        for entry in entries {
            if let Some(group) = groups
                .iter_mut()
                .find(|(project, _, _)| *project == entry.project)
            {
                group.2.push(entry.summary);
            } else {
                groups.push((
                    entry.project,
                    entry.project_name.clone(),
                    vec![entry.summary],
                ));
            }
        }
        let total: usize = groups.iter().map(|group| group.2.len()).sum();

        let head = crate::ui::design::header::panel_bar(cx)
            .child(crate::ui::design::header::panel_identity(
                None, "My Tasks", cx,
            ))
            .child(crate::ui::design::header::panel_meta(total.to_string(), cx))
            .child(div().flex_1())
            .child(
                crate::ui::style::refresh_icon_button("my-tasks-nav-refresh", cx)
                    .tooltip("Refresh")
                    .disabled(loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.tasks
                            .update(cx, |tasks, cx| tasks.refresh_my_tasks(cx));
                    })),
            );

        let mut body = v_flex()
            .id("my-tasks-nav-body")
            .flex_1()
            .min_h(px(0.))
            .gap_0p5()
            .px_1()
            .py_2()
            .overflow_y_scroll();

        if total == 0 {
            return v_flex()
                .size_full()
                .child(head)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .px_4()
                        .text_color(crate::ui::design::t3(cx))
                        .child(Icon::new(IconName::CircleCheck).size(crate::ui::design::icon_xl()))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .child(if loading {
                                    "Loading your tasks…"
                                } else {
                                    "You're all caught up"
                                }),
                        ),
                )
                .into_any_element();
        }

        let selected_ref = self
            .workspace
            .read(cx)
            .active_project()
            .map(|project| project.id)
            .and_then(|project| self.tasks.read(cx).selected_ref(project));

        for (gi, (project, name, tasks)) in groups.into_iter().enumerate() {
            body = body.child(
                h_flex()
                    .id(("my-tasks-group", gi))
                    .w_full()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .child(
                        div()
                            .flex_1()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .truncate()
                            .text_color(crate::ui::design::t3(cx))
                            .child(SharedString::from(name.to_uppercase())),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(tasks.len().to_string()),
                    ),
            );

            for summary in tasks {
                let reference = summary.reference.clone();
                let selected = selected_ref
                    .as_ref()
                    .is_some_and(|current| current.same_issue(&reference));
                let provider = summary.reference.provider;
                let dot = status_dot_color(&summary, cx);
                let title = summary.reference.title.clone();
                let status = summary.status.clone();
                let key = summary.reference.issue_key.clone();

                body = body.child(
                    h_flex()
                        .id(SharedString::from(format!(
                            "my-task-{}",
                            summary.reference.issue_id
                        )))
                        .w_full()
                        .px_2()
                        .py_1p5()
                        .gap_2()
                        .items_start()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .when(selected, |row| {
                            row.bg(crate::ui::design::surface_2(cx).opacity(0.58))
                                .border_1()
                                .border_color(crate::ui::design::t3(cx).opacity(0.08))
                        })
                        .hover(|row| row.bg(crate::ui::design::hover(cx)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(center) = this.center.upgrade() {
                                center.update(cx, |center, cx| {
                                    center.open_my_task(project, reference.clone(), cx);
                                });
                            }
                        }))
                        .child(provider_badge(provider, 20., cx))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .truncate()
                                        .text_color(crate::ui::style::focus_text(cx))
                                        .child(SharedString::from(title)),
                                )
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(div().size(px(6.)).rounded_full().bg(dot))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(SharedString::from(status)),
                                        )
                                        .child(
                                            div()
                                                .font_family(crate::ui::design::FONT_MONO)
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx).opacity(0.7))
                                                .child(SharedString::from(key)),
                                        ),
                                ),
                        ),
                );
            }
        }

        v_flex()
            .size_full()
            .child(head)
            .when_some(error, |view, error| {
                view.child(
                    div()
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(SharedString::from(format!("Some sources failed: {error}"))),
                )
            })
            .child(body)
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_nav_body(
        &self,
        project: ide_core::ProjectId,
        board: Option<&ide_core::TaskBoard>,
        loading: bool,
        board_error: Option<&str>,
        active_configured: bool,
        active_is_personal: bool,
        selected_ref: Option<&ide_core::TaskRef>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut body = v_flex()
            .id("task-nav-body")
            .flex_1()
            .min_h(px(0.))
            .gap_0p5()
            .px_1()
            .py_2()
            .overflow_y_scroll();

        if !active_configured {
            return body
                .items_center()
                .justify_center()
                .gap_2()
                .px_4()
                .text_color(crate::ui::design::t3(cx))
                .child(Icon::new(IconName::Settings).size(crate::ui::design::icon_xl()))
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("Finish setting up this source"),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_center()
                        .child("Pick a board or team to load its tasks."),
                )
                .into_any_element();
        }

        if let Some(error) = board_error {
            return body
                .gap_2()
                .px_2()
                .child(
                    v_flex()
                        .gap_1()
                        .p_3()
                        .rounded(crate::ui::design::r_sm())
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.35))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::rose(cx))
                                .child("Couldn't load tasks"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(SharedString::from(error.to_string())),
                        ),
                )
                .into_any_element();
        }

        let Some(board) = board else {
            return body
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(Icon::new(IconName::Loader).size(crate::ui::design::icon_lg()))
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .child("Loading tasks…"),
                )
                .into_any_element();
        };

        if board.issues.is_empty() {
            return body
                .items_center()
                .justify_center()
                .gap_2()
                .px_4()
                .text_color(crate::ui::design::t3(cx))
                .child(Icon::new(IconName::CircleCheck).size(crate::ui::design::icon_xl()))
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("No tasks here yet"),
                )
                .when(loading, |body| {
                    body.child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .child("Refreshing…"),
                    )
                })
                .into_any_element();
        }

        for column in &board.columns {
            let issues = board
                .issues
                .iter()
                .filter(|issue| issue.column == column.name)
                .collect::<Vec<_>>();
            if issues.is_empty() {
                continue;
            }
            let collapsed = self.collapsed_columns.contains(&column.name);
            let column_name = column.name.clone();
            let group_dot = column_dot_color(&column.name, issues.first().copied(), cx);

            body = body.child(
                h_flex()
                    .id(SharedString::from(format!("task-col-{}", column.name)))
                    .w_full()
                    .px_2()
                    .py_1()
                    .mt_1()
                    .gap_1p5()
                    .items_center()
                    .cursor_pointer()
                    .rounded(crate::ui::design::r_sm())
                    .hover(|row| row.bg(crate::ui::design::hover(cx)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_column(column_name.clone(), cx);
                    }))
                    .child(
                        Icon::new(if collapsed {
                            IconName::ChevronRight
                        } else {
                            IconName::ChevronDown
                        })
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::t3(cx)),
                    )
                    .child(div().size(px(6.)).rounded_full().bg(group_dot))
                    .child(
                        div()
                            .flex_1()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child(SharedString::from(column.name.to_uppercase())),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(issues.len().to_string()),
                    ),
            );

            if collapsed {
                continue;
            }

            for issue in issues {
                let reference = issue.reference.clone();
                let selected = selected_ref.is_some_and(|current| current.same_issue(&reference));
                let key = issue.reference.issue_key.clone();
                let title = issue.reference.title.clone();
                let meta = task_meta_line(issue);

                body = body.child(
                    h_flex()
                        .id(SharedString::from(format!(
                            "task-row-{}",
                            issue.reference.issue_id
                        )))
                        .w_full()
                        .px_2()
                        .py_1p5()
                        .ml_2()
                        .gap_2()
                        .items_start()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .when(selected, |row| {
                            row.bg(crate::ui::design::surface_2(cx).opacity(0.58))
                                .border_1()
                                .border_color(crate::ui::design::t3(cx).opacity(0.08))
                        })
                        .hover(|row| row.bg(crate::ui::design::hover(cx)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.select_task(project, reference.clone(), cx);
                        }))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w(px(0.))
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .truncate()
                                        .text_color(crate::ui::style::focus_text(cx))
                                        .child(SharedString::from(title)),
                                )
                                .child(
                                    h_flex()
                                        .gap_1p5()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(SharedString::from(key)),
                                        )
                                        .when(!meta.is_empty(), |row| {
                                            row.child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .truncate()
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(SharedString::from(meta)),
                                            )
                                        }),
                                ),
                        ),
                );
            }
        }

        if active_is_personal {
            body = body.child(
                h_flex()
                    .id("task-new-personal")
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .mt_1()
                    .gap_2()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .text_color(crate::ui::design::t3(cx))
                    .hover(|row| {
                        row.bg(crate::ui::design::hover(cx))
                            .text_color(crate::ui::design::t1(cx))
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.new_personal_task(project, window, cx);
                    }))
                    .child(Icon::new(IconName::Plus).size(crate::ui::design::icon_md()))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .child("New task"),
                    ),
            );
        }

        body.into_any_element()
    }
}
