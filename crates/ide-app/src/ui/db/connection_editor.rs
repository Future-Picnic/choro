//! The project's database connections dialog: a list of saved connections,
//! a capability-grouped provider picker, and one form per provider with an
//! explicit test lifecycle (idle, testing, connected, failed).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, PathPromptOptions, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Window,
};
use gpui_component::{
    h_flex,
    input::{InputEvent, InputState},
    v_flex, IconName, WindowExt,
};
use ide_core::{DatabaseHandle, DbConnection, DbProvider};
use uuid::Uuid;

use super::providers::{
    effective_read_only, endpoint_summary, provider_connection_label, provider_hint,
    provider_name_placeholder, provider_placeholder, ProviderSection,
};
use crate::state::Workspace;
use form::test_state_indicator;

mod form;

struct ConnectionRow {
    id: Uuid,
    provider: DbProvider,
    read_only: bool,
    show_help: bool,
    name: Entity<InputState>,
    uri: Entity<InputState>,
}

/// A connection test request and its outcome. `request` guards against a
/// slow result landing after the user has edited the form.
enum TestState {
    Testing { seq: u64 },
    Connected { elapsed: Duration },
    Failed(String),
}

const DB_CONNECTION_EDITOR_CONTENT_WIDTH: f32 = 576.;

/// Dialog for editing a project's database connections, mirroring the task
/// connection list/detail workflow.
pub struct DbConnectionsEditor {
    rows: Vec<ConnectionRow>,
    connection_tests: HashMap<Uuid, TestState>,
    test_seq: u64,
    active_row: Option<usize>,
    choosing_provider: bool,
    validation_error: Option<SharedString>,
}

impl DbConnectionsEditor {
    pub fn open(workspace: Entity<Workspace>, window: &mut Window, cx: &mut App) {
        let Some(project) = workspace.read(cx).active_project() else {
            return;
        };
        let project_id = project.id;
        let project_name = project.name.clone();
        let connections = project.db_connections.clone();

        let editor = cx.new(|cx| {
            let rows = connections
                .iter()
                .map(|conn| Self::row_from(conn, window, cx))
                .collect::<Vec<_>>();
            for row in &rows {
                Self::clear_status_on_change(row, cx);
            }
            Self {
                rows,
                connection_tests: HashMap::new(),
                test_seq: 0,
                active_row: None,
                choosing_provider: connections.is_empty(),
                validation_error: None,
            }
        });

        let footer_editor = editor.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let save_editor = footer_editor.clone();
            let save_workspace = workspace.clone();
            dialog
                .w(px(624.))
                .title(SharedString::from(format!(
                    "Database connections for {project_name}"
                )))
                .child(footer_editor.clone())
                .footer(move |_, _, _, cx| {
                    let editor = save_editor.clone();
                    let workspace = save_workspace.clone();
                    vec![
                        crate::ui::style::primary_button_compact("save-db-connections", "Save", cx)
                            .icon(IconName::Check)
                            .on_click(move |_, window, cx| match editor.read(cx).collect(cx) {
                                Ok(connections) => {
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.update_db_connections(
                                            project_id,
                                            connections,
                                            cx,
                                        );
                                    });
                                    window.close_dialog(cx);
                                }
                                Err((index, error)) => {
                                    editor.update(cx, |editor, cx| {
                                        editor.active_row = Some(index);
                                        editor.choosing_provider = false;
                                        editor.validation_error = Some(error.into());
                                        cx.notify();
                                    });
                                }
                            }),
                        crate::ui::style::dialog_neutral_button(
                            "cancel-db-connections",
                            "Cancel",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn row_from(conn: &DbConnection, window: &mut Window, cx: &mut App) -> ConnectionRow {
        ConnectionRow {
            id: conn.id,
            provider: conn.provider,
            read_only: effective_read_only(conn.provider, conn.read_only),
            show_help: false,
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_name_placeholder(conn.provider))
                    .default_value(conn.name.clone())
            }),
            uri: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_placeholder(conn.provider))
                    .default_value(conn.uri.clone())
                    .masked(!conn.provider.capabilities().local_file)
            }),
        }
    }

    fn new_row(provider: DbProvider, window: &mut Window, cx: &mut App) -> ConnectionRow {
        ConnectionRow {
            id: Uuid::new_v4(),
            provider,
            read_only: effective_read_only(provider, provider.capabilities().default_read_only),
            show_help: false,
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_name_placeholder(provider))
                    .default_value(provider.display_name())
            }),
            uri: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_placeholder(provider))
                    .masked(!provider.capabilities().local_file)
            }),
        }
    }

    fn clear_status_on_change(row: &ConnectionRow, cx: &mut Context<Self>) {
        let row_id = row.id;
        for input in [&row.name, &row.uri] {
            cx.subscribe(input, move |this: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.connection_tests.remove(&row_id);
                    this.validation_error = None;
                    cx.notify();
                }
            })
            .detach();
        }
    }

    fn add_connection(
        &mut self,
        provider: DbProvider,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let row = Self::new_row(provider, window, cx);
        Self::clear_status_on_change(&row, cx);
        self.rows.push(row);
        self.active_row = Some(self.rows.len() - 1);
        self.choosing_provider = false;
        self.validation_error = None;
        cx.notify();
    }

    fn set_read_only(&mut self, index: usize, read_only: bool, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(index) {
            row.read_only = effective_read_only(row.provider, read_only);
            self.connection_tests.remove(&row.id);
            self.validation_error = None;
        }
        cx.notify();
    }

    fn remove_connection(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            return;
        };
        let removed = self.rows.remove(index);
        self.connection_tests.remove(&removed.id);
        self.active_row = match self.active_row {
            Some(active) if active == index => None,
            Some(active) if active > index => Some(active - 1),
            active => active,
        };
        cx.notify();
    }

    fn toggle_help(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get_mut(index) {
            row.show_help = !row.show_help;
        }
        cx.notify();
    }

    /// Opens a real connection and runs the provider's health probe. Schema
    /// discovery is not required, so least-privilege users still pass.
    fn test_connection(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(connection) = self.connection_from_row(index, cx) else {
            return;
        };
        if connection.uri.is_empty() {
            self.connection_tests.insert(
                connection.id,
                TestState::Failed(format!(
                    "Enter the {} first.",
                    provider_connection_label(connection.provider).to_lowercase()
                )),
            );
            cx.notify();
            return;
        }
        self.test_seq += 1;
        let seq = self.test_seq;
        let id = connection.id;
        let request = connection.clone();
        self.connection_tests.insert(id, TestState::Testing { seq });
        cx.spawn(async move |this, cx| {
            let started = Instant::now();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let handle = DatabaseHandle::connect(&connection)?;
                    handle.ping()
                })
                .await;
            let elapsed = started.elapsed();
            this.update(cx, |this, cx| {
                let current = matches!(
                    this.connection_tests.get(&id),
                    Some(TestState::Testing { seq: current }) if *current == seq
                );
                let unchanged = this
                    .rows
                    .iter()
                    .position(|row| row.id == id)
                    .and_then(|index| this.connection_from_row(index, cx))
                    .is_some_and(|connection| connection == request);
                if !current || !unchanged {
                    return;
                }
                this.connection_tests.insert(
                    id,
                    match result {
                        Ok(()) => TestState::Connected { elapsed },
                        Err(error) => TestState::Failed(format!("{error:#}")),
                    },
                );
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn choose_sqlite_file(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose SQLite Database".into()),
        });
        let row_id = self.rows.get(index).map(|row| row.id);
        cx.spawn_in(window, async move |this, cx| {
            let path = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(path) = path else {
                return;
            };
            cx.update(|window, cx| {
                this.update(cx, |this, cx| {
                    if let Some(row) = this.rows.iter().find(|row| Some(row.id) == row_id) {
                        row.uri.update(cx, |input, cx| {
                            input.set_value(path.display().to_string(), window, cx);
                        });
                    }
                    cx.notify();
                })
                .ok();
            })
            .ok();
        })
        .detach();
    }

    fn connection_from_row(&self, index: usize, cx: &App) -> Option<DbConnection> {
        let row = self.rows.get(index)?;
        let name = row.name.read(cx).value().trim().to_string();
        Some(DbConnection {
            id: row.id,
            provider: row.provider,
            read_only: effective_read_only(row.provider, row.read_only),
            name: if name.is_empty() {
                row.provider.display_name().to_string()
            } else {
                name
            },
            uri: row.uri.read(cx).value().trim().to_string(),
        })
    }

    fn collect(&self, cx: &App) -> Result<Vec<DbConnection>, (usize, String)> {
        self.rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let name = row.name.read(cx).value().trim().to_string();
                if name.is_empty() {
                    return Err((index, "Enter a connection name.".into()));
                }
                let uri = row.uri.read(cx).value().trim().to_string();
                if uri.is_empty() {
                    return Err((
                        index,
                        format!(
                            "Enter the {}.",
                            provider_connection_label(row.provider).to_lowercase()
                        ),
                    ));
                }
                Ok(DbConnection {
                    id: row.id,
                    provider: row.provider,
                    read_only: effective_read_only(row.provider, row.read_only),
                    name,
                    uri,
                })
            })
            .collect()
    }

    fn render_connection_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w(px(DB_CONNECTION_EDITOR_CONTENT_WIDTH))
            .gap_2()
            .when(self.rows.is_empty() && !self.choosing_provider, |list| {
                list.child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::line(cx).opacity(0.36))
                        .bg(crate::ui::design::base(cx).opacity(0.35))
                        .px_3()
                        .py_5()
                        .child(
                            div()
                                .w(px(124.))
                                .h(px(84.))
                                .flex_none()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    crate::ui::illustrations::illustration(
                                        crate::ui::illustrations::Illustration::Database,
                                        cx,
                                    )
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Contain),
                                ),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Add a connection to browse this project's data."),
                        ),
                )
            })
            .children((0..self.rows.len()).map(|index| self.render_connection_summary(index, cx)))
            .when(self.choosing_provider, |list| {
                list.child(self.render_provider_picker(cx))
            })
            .child(
                h_flex()
                    .pt_1()
                    .gap_2()
                    .when(!self.choosing_provider, |actions| {
                        actions.child(
                            crate::ui::style::dialog_neutral_button(
                                "add-db-connection",
                                "Add connection",
                                cx,
                            )
                            .icon(IconName::Plus)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.choosing_provider = true;
                                cx.notify();
                            })),
                        )
                    })
                    .when(self.choosing_provider && !self.rows.is_empty(), |actions| {
                        actions.child(
                            crate::ui::style::dialog_neutral_button(
                                "cancel-db-provider-picker",
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

    fn render_connection_summary(&self, index: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let row = &self.rows[index];
        let name = row.name.read(cx).value().trim().to_string();
        let title = if name.is_empty() {
            row.provider.display_name().to_string()
        } else {
            name
        };
        let connection_id = row.id;
        let removal_detail = title.clone();
        let uri = row.uri.read(cx).value().to_string();
        let endpoint = endpoint_summary(row.provider, &uri);
        let read_only = effective_read_only(row.provider, row.read_only);
        let test_status = self.connection_tests.get(&row.id);
        h_flex()
            .id(("db-connection-summary", index))
            .w_full()
            .gap_2p5()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.36))
            .bg(crate::ui::design::base(cx).opacity(0.35))
            .px_3()
            .py_2()
            .child(super::provider_brand_mark(row.provider, 28.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(SharedString::from(title)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(row.provider.display_name()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .font_family(crate::ui::design::FONT_MONO)
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(SharedString::from(
                                        endpoint.unwrap_or_else(|| "Not configured".into()),
                                    )),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(if read_only { "Read only" } else { "Edits allowed" }),
                            ),
                    ),
            )
            .when_some(test_state_indicator(test_status, cx), |card, status| {
                card.child(status)
            })
            .child(
                crate::ui::style::dialog_neutral_button(("edit-db-connection", index), "Edit", cx)
                    .flex_none()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.active_row = Some(index);
                        this.choosing_provider = false;
                        this.validation_error = None;
                        cx.notify();
                    })),
            )
            .child(
                crate::ui::style::destructive_icon_button(("remove-db-connection", index), cx)
                    .flex_none()
                    .tooltip("Remove connection")
                    .on_click(cx.listener(move |_, _, window, cx| {
                        let editor = cx.entity();
                        let removal_detail = removal_detail.clone();
                        crate::ui::confirm::ConfirmDialog::new(
                            "Remove database connection?",
                            "This removes the connection from this list. The change is applied only when you save the database connections.",
                        )
                        .detail(removal_detail)
                        .confirm_label("Remove")
                        .confirm_id("confirm-remove-db-connection")
                        .on_confirm(move |_, cx| {
                            editor.update(cx, |editor, cx| {
                                editor.remove_connection(connection_id, cx);
                            });
                        })
                        .open(window, cx);
                    })),
            )
            .into_any_element()
    }

    fn render_provider_picker(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_3()
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child("Choose a database"),
            )
            .children(ProviderSection::ORDER.into_iter().map(|section| {
                v_flex()
                    .w_full()
                    .gap_1p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_baseline()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t2(cx))
                                    .child(section.title()),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(section.description()),
                            ),
                    )
                    .child(
                        h_flex().w_full().gap_2().flex_wrap().children(
                            section
                                .providers()
                                .map(|provider| self.render_provider_card(provider, cx)),
                        ),
                    )
            }))
            .into_any_element()
    }

    fn render_provider_card(
        &self,
        provider: DbProvider,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .id(SharedString::from(format!(
                "add-db-provider-{}",
                provider.as_str()
            )))
            .w(px(186.))
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.5))
            .bg(crate::ui::design::base(cx).opacity(0.35))
            .px_2p5()
            .py_2()
            .cursor_pointer()
            .hover(|choice| choice.bg(crate::ui::design::hover(cx)))
            .child(super::provider_brand_mark(provider, 24.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(crate::ui::design::t1(cx))
                            .child(provider.display_name()),
                    )
                    .child(
                        div()
                            .truncate()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(provider_hint(provider)),
                    ),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.add_connection(provider, window, cx);
            }))
            .into_any_element()
    }
}
