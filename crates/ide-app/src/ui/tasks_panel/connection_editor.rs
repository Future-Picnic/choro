use super::*;

pub(super) struct TaskConnectionRow {
    id: Uuid,
    provider: IssueTrackerProvider,
    show_help: bool,
    name: Entity<InputState>,
    site_url: Entity<InputState>,
    email: Entity<InputState>,
    api_token: Entity<InputState>,
    board_id: Entity<InputState>,
    board_name: Entity<InputState>,
    pr_done_status: Entity<InputState>,
    pub(super) assignee_filter: Option<String>,
    pub(super) assignee_account_id: Option<String>,
    pub(super) assignee_display_name: Option<String>,
    sources: Vec<TaskTrackerSource>,
    assignees: Vec<JiraUser>,
    loading_boards: bool,
    loading_assignees: bool,
    error: Option<String>,
}

pub struct TaskTrackerConnectionsEditor {
    rows: Vec<TaskConnectionRow>,
    mode: TaskTrackerEditorMode,
    active_row: Option<usize>,
    choosing_provider: bool,
}

const TASK_CONNECTION_EDITOR_CONTENT_WIDTH: f32 = 576.;

#[derive(Clone, Copy)]
enum TaskTrackerEditorMode {
    Add,
    #[allow(
        dead_code,
        reason = "retained for a future contextual single-source edit action"
    )]
    Edit(Uuid),
    ManageAll,
}

impl TaskTrackerConnectionsEditor {
    #[allow(
        dead_code,
        reason = "retained for a future contextual task-source entry point"
    )]
    pub fn open_add(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
        Self::open_with_mode(workspace, TaskTrackerEditorMode::Add, window, cx);
    }

    #[allow(
        dead_code,
        reason = "retained for a future contextual single-source edit action"
    )]
    pub fn open_edit(
        workspace: Entity<Workspace>,
        connection_id: Uuid,
        window: &mut Window,
        cx: &mut App,
    ) {
        Self::open_with_mode(
            workspace,
            TaskTrackerEditorMode::Edit(connection_id),
            window,
            cx,
        );
    }

    pub fn open(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
        Self::open_with_mode(workspace, TaskTrackerEditorMode::ManageAll, window, cx);
    }

    fn open_with_mode(
        workspace: Entity<Workspace>,
        mode: TaskTrackerEditorMode,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(project) = workspace.read(cx).active_project() else {
            return;
        };
        let project_id = project.id;
        let project_name = project.name.clone();
        let existing_connections = project.task_tracker_connections.clone();
        let connections = match mode {
            TaskTrackerEditorMode::Add => Vec::new(),
            TaskTrackerEditorMode::Edit(id) => existing_connections
                .iter()
                .find(|connection| connection.id == id)
                .cloned()
                .into_iter()
                .collect(),
            TaskTrackerEditorMode::ManageAll => existing_connections.clone(),
        };
        if connections.is_empty() && matches!(mode, TaskTrackerEditorMode::Edit(_)) {
            return;
        }

        let editor = cx.new(|cx| {
            let rows: Vec<_> = connections
                .iter()
                .map(|conn| Self::row_from(conn, window, cx))
                .collect();

            // A request made with the default `${JIRA_API_TOKEN}` can finish
            // after the user has pasted a literal token. Clear that obsolete
            // diagnostic as soon as any credential field changes.
            for row in &rows {
                Self::clear_error_on_credential_change(row, cx);
            }

            Self {
                rows,
                mode,
                active_row: matches!(mode, TaskTrackerEditorMode::Edit(_)).then_some(0),
                choosing_provider: matches!(mode, TaskTrackerEditorMode::Add),
            }
        });

        let footer_editor = editor.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let save_editor = footer_editor.clone();
            let save_workspace = workspace.clone();
            let original_connections = existing_connections.clone();
            dialog
                .w(px(624.))
                .title(SharedString::from(match mode {
                    TaskTrackerEditorMode::Add => format!("Add Connection — {project_name}"),
                    TaskTrackerEditorMode::Edit(_) => format!("Edit Connection — {project_name}"),
                    TaskTrackerEditorMode::ManageAll => {
                        format!("Task Connections — {project_name}")
                    }
                }))
                .child(footer_editor.clone())
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let workspace = save_workspace.clone();
                    let original_connections = original_connections.clone();
                    vec![
                        crate::ui::style::primary_button_compact("save-task-trackers", "Save", cx)
                            .icon(IconName::Check)
                            .on_click(move |_, window, cx| {
                                let collected = editor.read(cx).collect(cx);
                                let connections = match editor.read(cx).mode {
                                    TaskTrackerEditorMode::Add => {
                                        let mut next = original_connections.clone();
                                        next.extend(collected);
                                        next
                                    }
                                    TaskTrackerEditorMode::Edit(id) => original_connections
                                        .iter()
                                        .cloned()
                                        .map(|connection| {
                                            if connection.id == id {
                                                collected.first().cloned().unwrap_or(connection)
                                            } else {
                                                connection
                                            }
                                        })
                                        .collect(),
                                    TaskTrackerEditorMode::ManageAll => collected,
                                };
                                workspace.update(cx, |workspace, cx| {
                                    workspace.update_task_tracker_connections(
                                        project_id,
                                        connections,
                                        cx,
                                    );
                                });
                                window.close_dialog(cx);
                            }),
                        crate::ui::style::ghost_button_compact("cancel-task-trackers", "Cancel")
                            .custom(crate::ui::style::dialog_neutral_variant(cx))
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn row_from(
        conn: &TaskTrackerConnection,
        window: &mut Window,
        cx: &mut App,
    ) -> TaskConnectionRow {
        let source_id = conn
            .selected_source_id()
            .filter(|_| conn.provider != IssueTrackerProvider::Jira)
            .unwrap_or_else(|| conn.board_id.map(|id| id.to_string()).unwrap_or_default());
        let source_name = conn
            .selected_source_name()
            .filter(|_| conn.provider != IssueTrackerProvider::Jira)
            .unwrap_or_else(|| conn.board_name.clone().unwrap_or_default());
        TaskConnectionRow {
            id: conn.id,
            provider: conn.provider,
            show_help: false,
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(conn.provider.label())
                    .default_value(conn.name.clone())
            }),
            site_url: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_site_placeholder(conn.provider))
                    .default_value(conn.site_url.clone())
            }),
            email: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Jira email")
                    .default_value(conn.email.clone())
            }),
            api_token: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_token_placeholder(conn.provider))
                    .default_value(conn.api_token.clone())
                    .masked(true)
            }),
            board_id: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_source_id_placeholder(conn.provider))
                    .default_value(source_id)
            }),
            board_name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_source_name_placeholder(conn.provider))
                    .default_value(source_name)
            }),
            pr_done_status: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("e.g. In Review")
                    .default_value(conn.pr_done_status().unwrap_or_default())
            }),
            assignee_filter: conn.assignee_filter.clone(),
            assignee_account_id: conn.assignee_account_id.clone(),
            assignee_display_name: conn.assignee_display_name.clone(),
            sources: Vec::new(),
            assignees: Vec::new(),
            loading_boards: false,
            loading_assignees: false,
            error: None,
        }
    }

    fn clear_error_on_credential_change(row: &TaskConnectionRow, cx: &mut Context<Self>) {
        let row_id = row.id;
        for input in [&row.site_url, &row.email, &row.api_token] {
            cx.subscribe(input, move |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    if let Some(row) = this.rows.iter_mut().find(|row| row.id == row_id) {
                        row.error = None;
                    }
                    cx.notify();
                }
            })
            .detach();
        }
    }

    fn add_connection(
        &mut self,
        provider: IssueTrackerProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let connection = match provider {
            IssueTrackerProvider::Jira => TaskTrackerConnection::new_jira(
                provider.label(),
                provider_default_site_url(provider),
                "",
                provider_default_token(provider),
            ),
            _ => TaskTrackerConnection::new_external(
                provider,
                provider.label(),
                provider_default_site_url(provider),
                provider_default_token(provider),
            ),
        };
        let row = Self::row_from(&connection, window, cx);
        Self::clear_error_on_credential_change(&row, cx);
        self.rows.push(row);
        self.active_row = Some(self.rows.len() - 1);
        self.choosing_provider = false;
        cx.notify();
    }

    fn remove_connection(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return;
        };
        self.rows.remove(index);
        self.active_row = match self.active_row {
            Some(active) if active == index => None,
            Some(active) if active > index => Some(active - 1),
            active => active,
        };
        cx.notify();
    }

    fn collect(&self, cx: &App) -> Vec<TaskTrackerConnection> {
        self.rows
            .iter()
            .filter_map(|row| {
                let name = row.name.read(cx).value().trim().to_string();
                let site_url = row.site_url.read(cx).value().trim().to_string();
                let email = row.email.read(cx).value().trim().to_string();
                let api_token = row.api_token.read(cx).value().trim().to_string();
                if api_token.is_empty() {
                    return None;
                }
                if row.provider == IssueTrackerProvider::Jira
                    && (site_url.is_empty() || email.is_empty())
                {
                    return None;
                }
                let source_id_value = row.board_id.read(cx).value().trim().to_string();
                let board_id = (row.provider == IssueTrackerProvider::Jira)
                    .then(|| source_id_value.parse::<i64>().ok())
                    .flatten();
                let source_id = (!source_id_value.is_empty()).then_some(source_id_value.clone());
                let board_name = row.board_name.read(cx).value().trim().to_string();
                let source_name = (!board_name.is_empty()).then_some(board_name.clone());
                let assignee_filter = row
                    .assignee_filter
                    .as_ref()
                    .map(|filter| filter.trim().to_string())
                    .filter(|filter| !filter.is_empty());
                let assignee_account_id = row
                    .assignee_account_id
                    .as_ref()
                    .map(|account_id| account_id.trim().to_string())
                    .filter(|account_id| !account_id.is_empty());
                let assignee_display_name = row
                    .assignee_display_name
                    .as_ref()
                    .map(|name| name.trim().to_string())
                    .filter(|name| !name.is_empty());
                let pr_done_status = {
                    let value = row.pr_done_status.read(cx).value().trim().to_string();
                    (!value.is_empty()).then_some(value)
                };
                let connection = TaskTrackerConnection {
                    id: row.id,
                    provider: row.provider,
                    name: if name.is_empty() {
                        row.provider.label().to_string()
                    } else {
                        name
                    },
                    site_url,
                    email,
                    api_token,
                    source_id,
                    source_name,
                    source_kind: Some(provider_source_kind(row.provider).to_string()),
                    provider_config_json: "{}".to_string(),
                    filters_json: "{}".to_string(),
                    board_id,
                    board_name: (row.provider == IssueTrackerProvider::Jira)
                        .then_some(board_name)
                        .filter(|name| !name.is_empty()),
                    assignee_filter: assignee_account_id
                        .is_none()
                        .then_some(assignee_filter)
                        .flatten(),
                    assignee_account_id,
                    assignee_display_name,
                };
                Some(connection.with_pr_done_status(pr_done_status))
            })
            .collect()
    }

    fn connection_from_row(&self, ix: usize, cx: &App) -> Option<TaskTrackerConnection> {
        let row = self.rows.get(ix)?;
        let mut connections = Self {
            rows: vec![TaskConnectionRow {
                id: row.id,
                provider: row.provider,
                show_help: row.show_help,
                name: row.name.clone(),
                site_url: row.site_url.clone(),
                email: row.email.clone(),
                api_token: row.api_token.clone(),
                board_id: row.board_id.clone(),
                board_name: row.board_name.clone(),
                pr_done_status: row.pr_done_status.clone(),
                assignee_filter: row.assignee_filter.clone(),
                assignee_account_id: row.assignee_account_id.clone(),
                assignee_display_name: row.assignee_display_name.clone(),
                sources: Vec::new(),
                assignees: Vec::new(),
                loading_boards: false,
                loading_assignees: false,
                error: None,
            }],
            mode: TaskTrackerEditorMode::ManageAll,
            active_row: Some(0),
            choosing_provider: false,
        }
        .collect(cx);
        connections.pop()
    }

    fn load_boards(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.rows.get(ix).is_some_and(|row| row.loading_boards) {
            return;
        }
        let Some(connection) = self.connection_from_row(ix, cx) else {
            if let Some(row) = self.rows.get_mut(ix) {
                row.error = Some("Enter Jira site URL, email, and API token first.".into());
            }
            cx.notify();
            return;
        };
        let request_connection = connection.clone();
        if let Some(row) = self.rows.get_mut(ix) {
            row.loading_boards = true;
            row.error = None;
            row.sources.clear();
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let client = TaskTrackerClient::new(connection)?;
                    client.list_sources()
                })
                .await;
            this.update(cx, |this, cx| {
                let request_is_current = this
                    .connection_from_row(ix, cx)
                    .is_some_and(|current| current == request_connection);
                if let Some(row) = this.rows.get_mut(ix) {
                    row.loading_boards = false;
                    if !request_is_current {
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(boards) => {
                            row.sources = boards;
                            row.error = None;
                        }
                        Err(error) => {
                            row.error = Some(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn load_assignees(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.rows.get(ix).is_some_and(|row| row.loading_assignees) {
            return;
        }
        let Some(connection) = self.connection_from_row(ix, cx) else {
            if let Some(row) = self.rows.get_mut(ix) {
                row.error = Some("Enter the site URL and API token first.".into());
            }
            cx.notify();
            return;
        };
        if !connection.has_selected_source() {
            if let Some(row) = self.rows.get_mut(ix) {
                row.error = Some("Choose a board first, then load users.".into());
            }
            cx.notify();
            return;
        }
        let request_connection = connection.clone();
        if let Some(row) = self.rows.get_mut(ix) {
            row.loading_assignees = true;
            row.error = None;
            row.assignees.clear();
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let client = TaskTrackerClient::new(connection)?;
                    client.list_assignees()
                })
                .await;
            this.update(cx, |this, cx| {
                let request_is_current = this
                    .connection_from_row(ix, cx)
                    .is_some_and(|current| current == request_connection);
                if let Some(row) = this.rows.get_mut(ix) {
                    row.loading_assignees = false;
                    if !request_is_current {
                        cx.notify();
                        return;
                    }
                    match result {
                        Ok(assignees) => {
                            row.assignees = assignees;
                            row.error = None;
                        }
                        Err(error) => {
                            row.error = Some(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn select_assignee(&mut self, ix: usize, assignee: Option<JiraUser>, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(ix) {
            if let Some(assignee) = assignee {
                row.assignee_account_id = Some(assignee.account_id);
                row.assignee_display_name = Some(assignee.display_name);
                row.assignee_filter = None;
            } else {
                row.assignee_account_id = None;
                row.assignee_display_name = None;
                row.assignee_filter = None;
            }
        }
        cx.notify();
    }

    fn toggle_help(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(ix) {
            row.show_help = !row.show_help;
        }
        cx.notify();
    }

    fn render_connection_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w(px(TASK_CONNECTION_EDITOR_CONTENT_WIDTH))
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Connections"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Add as many Jira, Linear, ClickUp, or Asana connections as you need."),
                    ),
            )
            .when(self.rows.is_empty(), |list| {
                list.child(
                    div()
                        .w_full()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::line(cx).opacity(0.36))
                        .bg(crate::ui::design::base(cx).opacity(0.35))
                        .px_3()
                        .py_4()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No external connections yet."),
                )
            })
            .children(self.rows.iter().enumerate().map(|(ix, row)| {
                let name = row.name.read(cx).value().trim().to_string();
                let source = row.board_name.read(cx).value().trim().to_string();
                let title = if name.is_empty() {
                    row.provider.label().to_string()
                } else {
                    name
                };
                let connection_id = row.id;
                let removal_detail = title.clone();
                let detail = if source.is_empty() {
                    format!("{} · No {} selected", row.provider.label(), provider_source_label(row.provider).to_lowercase())
                } else {
                    format!("{} · {source}", row.provider.label())
                };
                h_flex()
                    .id(("task-connection-summary", ix))
                    .w_full()
                    .gap_2p5()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .border_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.36))
                    .bg(crate::ui::design::base(cx).opacity(0.35))
                    .px_3()
                    .py_2p5()
                    .child(provider_badge(row.provider, 30., cx))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(SharedString::from(title)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(SharedString::from(detail)),
                            ),
                    )
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            ("edit-task-connection", ix),
                            "Edit",
                            cx,
                        )
                        .flex_none()
                        .on_click(cx.listener(move |this, _, _, cx| {
                                this.active_row = Some(ix);
                                this.choosing_provider = false;
                                cx.notify();
                            })),
                    )
                    .child(
                        crate::ui::style::destructive_icon_button(
                            ("remove-task-connection", ix),
                            cx,
                        )
                        .flex_none()
                        .tooltip("Remove connection")
                        .on_click(cx.listener(move |_, _, window, cx| {
                            let editor = cx.entity();
                            let removal_detail = removal_detail.clone();
                            crate::ui::confirm::ConfirmDialog::new(
                                "Remove task connection?",
                                "This removes the connection from this list. The change is applied only when you save the task connections.",
                            )
                            .detail(removal_detail)
                            .confirm_label("Remove")
                            .confirm_id("confirm-remove-task-connection")
                            .on_confirm(move |_, cx| {
                                editor.update(cx, |editor, cx| {
                                    editor.remove_connection(connection_id, cx);
                                });
                            })
                            .open(window, cx);
                        })),
                    )
            }))
            .when(self.choosing_provider, |list| {
                list.child(self.render_provider_picker(cx))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "add-task-connection",
                            "Add connection",
                            cx,
                        )
                        .flex_none()
                        .icon(IconName::Plus)
                        .disabled(self.choosing_provider)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choosing_provider = true;
                                cx.notify();
                            })),
                    )
                    .when(self.choosing_provider, |actions| {
                        actions.child(
                            crate::ui::style::dialog_neutral_button(
                                "cancel-provider-picker",
                                "Cancel",
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                    this.choosing_provider = false;
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_provider_picker(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .pt_1()
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t3(cx))
                    .child("Choose a provider"),
            )
            .child(
                h_flex().w_full().gap_2().flex_wrap().children(
                    IssueTrackerProvider::EXTERNAL
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(provider_ix, provider)| {
                            h_flex()
                                .id(("add-task-provider", provider_ix))
                                .w(px(132.))
                                .gap_2()
                                .items_center()
                                .rounded(crate::ui::design::r_sm())
                                .border_1()
                                .border_color(crate::ui::design::line(cx).opacity(0.5))
                                .bg(crate::ui::design::base(cx).opacity(0.35))
                                .px_2()
                                .py_2()
                                .cursor_pointer()
                                .hover(|choice| choice.bg(crate::ui::design::hover(cx)))
                                .child(provider_badge(provider, 26., cx))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child(provider.label()),
                                )
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.add_connection(provider, window, cx);
                                }))
                        }),
                ),
            )
            .into_any_element()
    }
}

impl Render for TaskTrackerConnectionsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = v_flex()
            .gap_3()
            .w(px(TASK_CONNECTION_EDITOR_CONTENT_WIDTH))
            .max_h(px(620.))
            .overflow_y_scrollbar();

        if matches!(self.mode, TaskTrackerEditorMode::ManageAll) && self.active_row.is_none() {
            return editor
                .child(self.render_connection_list(cx))
                .into_any_element();
        }

        if self.choosing_provider && self.active_row.is_none() {
            return editor
                .child(self.render_provider_picker(cx))
                .into_any_element();
        }

        let active_row = self.active_row;
        let show_back = matches!(self.mode, TaskTrackerEditorMode::ManageAll);
        editor
            .children(self.rows.iter().enumerate().filter(move |(ix, _)| {
                active_row == Some(*ix)
            }).map(|(ix, row)| {
                let assignee_label = selected_assignee_label(row);
                let selected_account_id = row.assignee_account_id.clone();
                v_flex()
                    .id(("task-tracker-row", ix))
                    .w_full()
                    .gap_3()
                    .when(show_back, |form| {
                        form.child(
                            crate::ui::style::dialog_neutral_button(
                                "back-to-task-connections",
                                "All connections",
                                cx,
                            )
                            .flex_none()
                            .icon(IconName::ChevronLeft)
                            .on_click(cx.listener(|this, _, _, cx| {
                                    this.active_row = None;
                                    cx.notify();
                                })),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2p5()
                            .items_center()
                            .child(provider_badge(row.provider, 30., cx))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(SharedString::from(format!(
                                                "{} connection",
                                                row.provider.label()
                                            ))),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(
                                            "Issues become read-only cards you can run agents on",
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(form_field_label("Connection name", cx))
                            .child(Input::new(&row.name)),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(form_field_label("Site URL", cx))
                            .child(Input::new(&row.site_url)),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_3()
                            .items_start()
                            .when(row.provider == IssueTrackerProvider::Jira, |fields| {
                                fields.child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .gap_1()
                                        .child(form_field_label("Email", cx))
                                        .child(Input::new(&row.email)),
                                )
                            })
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(form_field_label("API token", cx))
                                    .child(Input::new(&row.api_token).mask_toggle()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .line_height(gpui::relative(1.45))
                            .text_color(crate::ui::design::t3(cx))
                            .child(SharedString::from(format!(
                                "Paste a token, or reference an env var like ${{{}_API_TOKEN}}.",
                                row.provider.label().to_uppercase()
                            ))),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(form_field_label(provider_source_label(row.provider), cx))
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .child(Input::new(&row.board_name)),
                                    )
                                    .child(div().w(px(96.)).child(Input::new(&row.board_id)))
                                    .child(
                                        crate::ui::style::secondary_button_compact(
                                            ("load-jira-boards", ix),
                                            if row.loading_boards {
                                                "Fetching"
                                            } else {
                                                "Fetch"
                                            },
                                        )
                                        .custom(crate::ui::style::dialog_neutral_variant(cx))
                                        .icon(IconName::LoaderCircle)
                                        .flex_none()
                                        .disabled(row.loading_boards)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.load_boards(ix, cx);
                                            }),
                                        ),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(form_field_label("Suggested status when a PR is done", cx))
                            .child(Input::new(&row.pr_done_status))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        "Pre-selected on the ship card after a PR — never applied automatically.",
                                    ),
                            ),
                    )
                    .when(row.provider != IssueTrackerProvider::Personal, |card| {
                        card.child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .w(px(110.))
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Show tasks for"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .rounded(crate::ui::design::r_sm())
                                        .border_1()
                                        .border_color(crate::ui::design::line(cx).opacity(0.32))
                                        .bg(crate::ui::design::base(cx).opacity(0.44))
                                        .px_2()
                                        .py_1p5()
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .items_center()
                                                .child(
                                                    Icon::new(IconName::User)
                                                        .size(crate::ui::design::icon_md())
                                                        .text_color(crate::ui::design::t3(cx)),
                                                )
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .truncate()
                                                        .text_size(crate::ui::design::text_body())
                                                        .text_color(crate::ui::design::t1(cx))
                                                        .child(SharedString::from(assignee_label)),
                                                ),
                                        ),
                                )
                                .child(
                                    crate::ui::style::dialog_neutral_button(
                                        ("load-jira-assignees", ix),
                                        if row.loading_assignees {
                                            "Loading"
                                        } else {
                                            "Load Users"
                                        },
                                        cx,
                                    )
                                    .flex_none()
                                        .disabled(row.loading_assignees)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.load_assignees(ix, cx);
                                        })),
                                )
                                .child(
                                    crate::ui::style::dialog_neutral_button(
                                        ("jira-board-assignee-all", ix),
                                        "All",
                                        cx,
                                    )
                                    .flex_none()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                            this.select_assignee(ix, None, cx);
                                        })),
                                ),
                        )
                    })
                    .when(
                        row.provider != IssueTrackerProvider::Personal && !row.assignees.is_empty(),
                        |card| {
                            card.child(v_flex().gap_1().children(
                                row.assignees.iter().cloned().enumerate().map(
                                    |(assignee_ix, assignee)| {
                                        let selected = selected_account_id.as_ref().is_some_and(
                                            |account_id| account_id == &assignee.account_id,
                                        );
                                        let email = assignee.email.clone();
                                        h_flex()
                                            .id(("jira-assignee-row", ix * 1000 + assignee_ix))
                                            .gap_2()
                                            .items_center()
                                            .px_2()
                                            .py_1()
                                            .rounded(crate::ui::design::r_sm())
                                            .cursor_pointer()
                                            .when(selected, |row| {
                                                row.bg(crate::ui::design::accent(cx).opacity(0.12))
                                                    .border_1()
                                                    .border_color(crate::ui::design::accent(cx).opacity(0.28))
                                            })
                                            .hover(|row| row.bg(crate::ui::design::hover(cx)))
                                            .child(Icon::new(IconName::User).size(crate::ui::design::icon_md()).text_color(
                                                if selected {
                                                    crate::ui::design::accent(cx)
                                                } else {
                                                    crate::ui::design::t3(cx)
                                                },
                                            ))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .truncate()
                                                    .text_size(crate::ui::design::text_body())
                                                    .text_color(crate::ui::design::t1(cx))
                                                    .child(SharedString::from(
                                                        assignee.display_name.clone(),
                                                    )),
                                            )
                                            .when_some(email, |row, email| {
                                                row.child(
                                                    div()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .truncate()
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child(SharedString::from(email)),
                                                )
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.select_assignee(
                                                    ix,
                                                    Some(assignee.clone()),
                                                    cx,
                                                );
                                            }))
                                    },
                                ),
                            ))
                        },
                    )
                    .when_some(row.error.clone(), |card, error| {
                        card.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::rose(cx))
                                .child(SharedString::from(error)),
                        )
                    })
                    .when(!row.sources.is_empty(), |card| {
                        card.child(v_flex().gap_1().children(
                            row.sources.iter().take(12).cloned().enumerate().map(
                                |(source_ix, source)| {
                                    let id = source.id.clone();
                                    let name = source.name.clone();
                                    let source_type = source.source_type.clone();
                                    h_flex()
                                        .id(("task-source-row", ix * 1000 + source_ix))
                                        .gap_2()
                                        .items_center()
                                        .px_2()
                                        .py_1()
                                        .rounded(crate::ui::design::r_sm())
                                        .cursor_pointer()
                                        .hover(|row| row.bg(crate::ui::design::hover(cx)))
                                        .child(
                                            div()
                                                .w(px(64.))
                                                .font_family(crate::ui::design::FONT_MONO)
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(id.clone()),
                                        )
                                        .child(div().flex_1().text_size(crate::ui::design::text_body()).child(name.clone()))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(source_type),
                                        )
                                        .on_click(cx.listener(move |this, _, window, cx| {
                                            if let Some(row) = this.rows.get(ix) {
                                                let board_id_input = row.board_id.clone();
                                                let board_name_input = row.board_name.clone();
                                                board_id_input.update(cx, |input, cx| {
                                                    input.set_value(id.clone(), window, cx)
                                                });
                                                board_name_input.update(cx, |input, cx| {
                                                    input.set_value(name.clone(), window, cx)
                                                });
                                            }
                                            if let Some(row) = this.rows.get_mut(ix) {
                                                row.assignee_filter = None;
                                                row.assignee_account_id = None;
                                                row.assignee_display_name = None;
                                                row.assignees.clear();
                                            }
                                            if this.rows.get(ix).is_some_and(|row| {
                                                row.provider != IssueTrackerProvider::Personal
                                            }) {
                                                this.load_assignees(ix, cx);
                                            }
                                        }))
                                },
                            ),
                        ))
                    })
                    .child(
                        v_flex()
                            .w_full()
                            .rounded(crate::ui::design::r_sm())
                            .border_1()
                            .border_color(crate::ui::design::line(cx).opacity(0.36))
                            .bg(crate::ui::design::base(cx).opacity(0.5))
                            .px_3()
                            .py_2()
                            .gap_1p5()
                            .child(
                                h_flex()
                                    .id(("task-provider-help", ix))
                                    .gap_1p5()
                                    .items_center()
                                    .cursor_pointer()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_help(ix, cx);
                                    }))
                                    .child(
                                        Icon::new(if row.show_help {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(crate::ui::design::icon_sm())
                                        .text_color(crate::ui::design::t3(cx)),
                                    )
                                    .child(provider_help_title(row.provider)),
                            )
                            .when(row.show_help, |help| {
                                help.children(
                                    provider_help_steps(row.provider).iter().enumerate().map(
                                        |(step_ix, step)| {
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .line_height(gpui::relative(1.4))
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(SharedString::from(format!(
                                                    "{}. {}",
                                                    step_ix + 1,
                                                    step
                                                )))
                                        },
                                    ),
                                )
                            }),
                    )
            }))
            .into_any_element()
    }
}
