use std::collections::{HashMap, HashSet};

use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, PathPromptOptions, Render, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    spinner::Spinner,
    v_flex, Disableable, Icon, IconName, Selectable, Sizable, WindowExt,
};
use ide_core::{DatabaseHandle, DbConnection, DbObject, DbObjectKind, DbProvider};
use uuid::Uuid;

use crate::state::Workspace;
use crate::ui::center::CenterArea;
use crate::ui::style;

/// Async-loaded tree level: not asked yet / in flight / done / failed.
enum Loaded<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// Right panel tab: saved database connections for the active project,
/// expanding to namespaces and database objects.
pub struct DbPanel {
    workspace: Entity<Workspace>,
    center: gpui::WeakEntity<CenterArea>,
    expanded_conns: HashSet<Uuid>,
    expanded_dbs: HashSet<(Uuid, String)>,
    connection_specs: HashMap<Uuid, (DbProvider, String, bool)>,
    handles: HashMap<Uuid, DatabaseHandle>,
    databases: HashMap<Uuid, Loaded<Vec<String>>>,
    collections: HashMap<(Uuid, String), Loaded<Vec<DbObject>>>,
}

impl DbPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        center: gpui::WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            Self {
                workspace,
                center,
                expanded_conns: HashSet::new(),
                expanded_dbs: HashSet::new(),
                connection_specs: HashMap::new(),
                handles: HashMap::new(),
                databases: HashMap::new(),
                collections: HashMap::new(),
            }
        })
    }

    /// A pooled client per connection, created lazily off the UI thread by
    /// the load functions (connect itself is cheap; queries do the blocking).
    fn handle_for(&mut self, conn: &DbConnection) -> anyhow::Result<DatabaseHandle> {
        if let Some(handle) = self.handles.get(&conn.id) {
            return Ok(handle.clone());
        }
        let handle = DatabaseHandle::connect(conn)?;
        self.handles.insert(conn.id, handle.clone());
        Ok(handle)
    }

    fn toggle_connection(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        if !self.expanded_conns.insert(conn.id) {
            self.expanded_conns.remove(&conn.id);
            cx.notify();
            return;
        }
        if !matches!(self.databases.get(&conn.id), Some(Loaded::Ready(_))) {
            self.load_databases(conn, cx);
        }
        cx.notify();
    }

    fn load_databases(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        let conn_id = conn.id;
        self.databases.insert(conn_id, Loaded::Loading);
        let handle = match self.handle_for(&conn) {
            Ok(handle) => handle,
            Err(error) => {
                self.databases
                    .insert(conn_id, Loaded::Failed(format!("{error:#}")));
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { handle.list_namespaces() })
                .await;
            this.update(cx, |this, cx| {
                this.databases.insert(
                    conn_id,
                    match result {
                        Ok(dbs) => Loaded::Ready(dbs),
                        Err(error) => Loaded::Failed(format!("{error:#}")),
                    },
                );
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn toggle_database(&mut self, conn: DbConnection, db: String, cx: &mut Context<Self>) {
        let key = (conn.id, db.clone());
        if !self.expanded_dbs.insert(key.clone()) {
            self.expanded_dbs.remove(&key);
            cx.notify();
            return;
        }
        if !matches!(self.collections.get(&key), Some(Loaded::Ready(_))) {
            self.load_collections(conn, db, cx);
        }
        cx.notify();
    }

    fn load_collections(&mut self, conn: DbConnection, db: String, cx: &mut Context<Self>) {
        let key = (conn.id, db.clone());
        self.collections.insert(key.clone(), Loaded::Loading);
        let handle = match self.handle_for(&conn) {
            Ok(handle) => handle,
            Err(error) => {
                self.collections
                    .insert(key, Loaded::Failed(format!("{error:#}")));
                return;
            }
        };
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { handle.list_objects(&db) })
                .await;
            this.update(cx, |this, cx| {
                this.collections.insert(
                    key,
                    match result {
                        Ok(colls) => Loaded::Ready(colls),
                        Err(error) => Loaded::Failed(format!("{error:#}")),
                    },
                );
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_object(
        &mut self,
        conn: DbConnection,
        db: String,
        object: DbObject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let handle = match self.handle_for(&conn) {
            Ok(handle) => handle,
            Err(error) => {
                eprintln!("db: connect failed: {error:#}");
                return;
            }
        };
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_db_object(handle, conn.clone(), db, object, window, cx);
            });
        }
    }

    /// Drops cached state for a connection so the next expand reloads.
    fn refresh_connection(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        self.handles.remove(&conn.id);
        self.databases.remove(&conn.id);
        self.collections.retain(|(id, _), _| *id != conn.id);
        if self.expanded_conns.contains(&conn.id) {
            self.load_databases(conn, cx);
        }
        cx.notify();
    }

    /// Invalidates provider/URI-dependent caches after the connection editor
    /// changes a saved connection while keeping its stable id. Access changes
    /// propagate through the shared handle so already-open panes are protected.
    fn sync_connection_specs(&mut self, connections: &[DbConnection], cx: &mut Context<Self>) {
        let current_ids = connections
            .iter()
            .map(|connection| connection.id)
            .collect::<HashSet<_>>();
        let mut reload = Vec::new();

        for connection in connections {
            let next = (
                connection.provider,
                connection.uri.clone(),
                connection.read_only,
            );
            let previous = self.connection_specs.get(&connection.id);
            let endpoint_changed =
                previous.is_some_and(|current| current.0 != next.0 || current.1 != next.1);
            let access_changed = previous.is_some_and(|current| current.2 != next.2);
            if access_changed && !endpoint_changed {
                if let Some(handle) = self.handles.get(&connection.id) {
                    handle.set_read_only(connection.read_only);
                }
            }
            self.connection_specs.insert(connection.id, next);
            if endpoint_changed {
                self.handles.remove(&connection.id);
                self.databases.remove(&connection.id);
                self.collections.retain(|(id, _), _| *id != connection.id);
                self.expanded_dbs.retain(|(id, _)| *id != connection.id);
                if self.expanded_conns.contains(&connection.id) {
                    reload.push(connection.clone());
                }
            }
        }

        self.connection_specs
            .retain(|id, _| current_ids.contains(id));
        self.handles.retain(|id, _| current_ids.contains(id));
        self.databases.retain(|id, _| current_ids.contains(id));
        self.collections
            .retain(|(id, _), _| current_ids.contains(id));
        self.expanded_conns.retain(|id| current_ids.contains(id));
        self.expanded_dbs.retain(|(id, _)| current_ids.contains(id));

        for connection in reload {
            self.load_databases(connection, cx);
        }
    }

    fn tree_row(&self, indent: usize, content: impl IntoElement, cx: &Context<Self>) -> gpui::Div {
        h_flex()
            .w_full()
            .pl(px(10. + indent as f32 * 16.))
            .pr_2()
            .py_1()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .text_size(crate::ui::design::text_body())
            .hover(|style| style.bg(crate::ui::design::hover(cx)))
            .child(content)
    }
}

/// "2h ago"-style label.
impl Render for DbPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(project) = self.workspace.read(cx).active_project() else {
            return style::empty_context_panel("DB", "Open a project to browse databases", cx)
                .into_any_element();
        };
        let project_id = project.id;
        let connections = project.db_connections.clone();
        self.sync_connection_specs(&connections, cx);

        let header = crate::ui::design::header::panel_bar(cx)
            .child(crate::ui::design::header::panel_identity(
                None, "Database", cx,
            ))
            .child(div().flex_1())
            .child({
                let workspace = self.workspace.clone();
                style::context_panel_action_button(
                    "db-edit-connections",
                    IconName::Settings2,
                    "Edit",
                    cx,
                )
                .tooltip("Manage connections")
                .on_click(move |_, window, cx| {
                    DbConnectionsEditor::open(workspace.clone(), window, cx);
                })
            })
            .child(style::right_sidebar_toggle(true, false, cx));

        if connections.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(crate::ui::design::t3(cx))
                        .child(Icon::new(IconName::Database).size_8())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .child("No database connections"),
                        ),
                )
                .into_any_element();
        }

        let danger = crate::ui::design::rose(cx);
        let mut list = v_flex()
            .id("db-tree")
            .flex_1()
            .min_h(px(0.))
            .px_1()
            .overflow_y_scroll();

        for (cix, conn) in connections.iter().enumerate() {
            let expanded = self.expanded_conns.contains(&conn.id);
            let prod = conn.looks_like_prod();
            let toggle_conn = conn.clone();
            let refresh_conn = conn.clone();
            list = list.child(
                self.tree_row(
                    0,
                    h_flex()
                        .w_full()
                        .gap_1p5()
                        .items_center()
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(crate::ui::design::icon_sm())
                            .text_color(crate::ui::design::t3(cx)),
                        )
                        .child(super::provider_brand_mark(
                            conn.provider,
                            crate::ui::design::ICON_MD,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(SharedString::from(conn.name.clone())),
                        )
                        .when(prod, |row| {
                            row.child(
                                div()
                                    .px_1()
                                    .rounded(crate::ui::design::r_xs())
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::BOLD)
                                    .bg(danger)
                                    .text_color(crate::ui::design::on_rose(cx))
                                    .child("PROD"),
                            )
                        })
                        .child(
                            style::icon_button(("db-refresh-conn", cix), IconName::Redo2, cx)
                                .tooltip("Reconnect & reload")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.refresh_connection(refresh_conn.clone(), cx);
                                })),
                        ),
                    cx,
                )
                .id(("db-conn-row", cix))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_connection(toggle_conn.clone(), cx);
                })),
            );

            if !expanded {
                continue;
            }
            match self.databases.get(&conn.id) {
                None | Some(Loaded::Loading) => {
                    list = list.child(self.tree_row(
                        1,
                        h_flex().gap_2().child(Spinner::new().small()),
                        cx,
                    ));
                }
                Some(Loaded::Failed(error)) => {
                    list = list.child(
                        self.tree_row(
                            1,
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(danger)
                                .whitespace_normal()
                                .child(SharedString::from(error.clone())),
                            cx,
                        ),
                    );
                }
                Some(Loaded::Ready(dbs)) => {
                    for (dix, db) in dbs.clone().iter().enumerate() {
                        let key = (conn.id, db.clone());
                        let db_expanded = self.expanded_dbs.contains(&key);
                        let toggle_conn = conn.clone();
                        let toggle_db = db.clone();
                        list = list.child(
                            self.tree_row(
                                1,
                                h_flex()
                                    .w_full()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        Icon::new(if db_expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(crate::ui::design::icon_sm())
                                        .text_color(crate::ui::design::t3(cx)),
                                    )
                                    .child(
                                        Icon::new(IconName::FolderClosed)
                                            .size(crate::ui::design::icon_md())
                                            .text_color(crate::ui::design::t3(cx)),
                                    )
                                    .child(SharedString::from(db.clone())),
                                cx,
                            )
                            .id(("db-db-row", cix * 1000 + dix))
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.toggle_database(
                                        toggle_conn.clone(),
                                        toggle_db.clone(),
                                        cx,
                                    );
                                },
                            )),
                        );

                        if !db_expanded {
                            continue;
                        }
                        match self.collections.get(&key) {
                            None | Some(Loaded::Loading) => {
                                list = list.child(self.tree_row(
                                    2,
                                    h_flex().gap_2().child(Spinner::new().small()),
                                    cx,
                                ));
                            }
                            Some(Loaded::Failed(error)) => {
                                list = list.child(
                                    self.tree_row(
                                        2,
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(danger)
                                            .whitespace_normal()
                                            .child(SharedString::from(error.clone())),
                                        cx,
                                    ),
                                );
                            }
                            Some(Loaded::Ready(colls)) => {
                                for (tix, object) in colls.clone().iter().enumerate() {
                                    let open_conn = conn.clone();
                                    let open_db = db.clone();
                                    let open_object = object.clone();
                                    let icon = match object.kind {
                                        DbObjectKind::Collection => IconName::File,
                                        DbObjectKind::Table => IconName::Database,
                                        DbObjectKind::View => IconName::Eye,
                                    };
                                    list = list.child(
                                        self.tree_row(
                                            2,
                                            h_flex()
                                                .w_full()
                                                .gap_1p5()
                                                .items_center()
                                                .child(
                                                    Icon::new(icon)
                                                        .size(crate::ui::design::icon_md())
                                                        .text_color(crate::ui::design::t3(cx)),
                                                )
                                                .child(SharedString::from(object.name.clone()))
                                                .when(object.kind == DbObjectKind::View, |row| {
                                                    row.child(
                                                        div()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .text_color(crate::ui::design::t3(cx))
                                                            .child("view"),
                                                    )
                                                }),
                                            cx,
                                        )
                                        .id(("db-coll-row", cix * 100_000 + dix * 1000 + tix))
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.open_object(
                                                    open_conn.clone(),
                                                    open_db.clone(),
                                                    open_object.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // Keep project id alive for the editor (avoids unused warning churn).
        let _ = project_id;

        v_flex()
            .size_full()
            .child(header)
            .child(list)
            .into_any_element()
    }
}

// ----- connections editor dialog -----

struct ConnectionRow {
    id: Uuid,
    provider: DbProvider,
    read_only: bool,
    show_help: bool,
    name: Entity<InputState>,
    uri: Entity<InputState>,
}

const DB_CONNECTION_EDITOR_CONTENT_WIDTH: f32 = 576.;

/// Dialog for editing a project's database connections, mirroring the task
/// connection list/detail workflow.
pub struct DbConnectionsEditor {
    rows: Vec<ConnectionRow>,
    connection_tests: HashMap<Uuid, Loaded<()>>,
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
                active_row: None,
                choosing_provider: false,
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
                    "Database Connections — {project_name}"
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
            read_only: conn.read_only,
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
                    .masked(conn.provider != DbProvider::SQLite)
            }),
        }
    }

    fn new_row(provider: DbProvider, window: &mut Window, cx: &mut App) -> ConnectionRow {
        ConnectionRow {
            id: Uuid::new_v4(),
            provider,
            read_only: provider.is_postgres() || provider.is_mysql(),
            show_help: false,
            name: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_name_placeholder(provider))
                    .default_value(provider.display_name())
            }),
            uri: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(provider_placeholder(provider))
                    .masked(provider != DbProvider::SQLite)
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
            row.read_only = read_only;
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

    fn test_connection(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(connection) = self.connection_from_row(index, cx) else {
            return;
        };
        if connection.uri.is_empty() {
            self.connection_tests.insert(
                connection.id,
                Loaded::Failed("Enter a connection string or file path first".into()),
            );
            cx.notify();
            return;
        }
        let id = connection.id;
        let request = connection.clone();
        self.connection_tests.insert(id, Loaded::Loading);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let handle = DatabaseHandle::connect(&connection)?;
                    handle.list_namespaces()?;
                    Ok::<_, anyhow::Error>(())
                })
                .await;
            this.update(cx, |this, cx| {
                if this.connection_from_row(index, cx).as_ref() != Some(&request) {
                    return;
                }
                this.connection_tests.insert(
                    id,
                    match result {
                        Ok(()) => Loaded::Ready(()),
                        Err(error) => Loaded::Failed(format!("{error:#}")),
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
                    if let Some(row) = this.rows.get(index) {
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
            read_only: row.read_only,
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
                        format!("Enter {}.", provider_connection_label(row.provider)),
                    ));
                }
                Ok(DbConnection {
                    id: row.id,
                    provider: row.provider,
                    read_only: row.read_only,
                    name,
                    uri,
                })
            })
            .collect()
    }

    fn render_connection_list(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w(px(DB_CONNECTION_EDITOR_CONTENT_WIDTH))
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Connections"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Each provider has its own setup form and connection guidance."),
                    ),
            )
            .when(self.rows.is_empty(), |list| {
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
                                .child("No database connections yet."),
                        ),
                )
            })
            .children(self.rows.iter().enumerate().map(|(index, row)| {
                let name = row.name.read(cx).value().trim().to_string();
                let title = if name.is_empty() {
                    row.provider.display_name().to_string()
                } else {
                    name
                };
                let connection_id = row.id;
                let removal_detail = title.clone();
                let configured = !row.uri.read(cx).value().trim().is_empty();
                let detail = connection_summary(row.provider, row.read_only, configured);
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
                    .py_2p5()
                    .child(super::provider_brand_mark(row.provider, 30.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
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
                    .when_some(test_status_label(test_status), |card, (icon, label, ok)| {
                        card.child(
                            h_flex()
                                .gap_1()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(if ok {
                                    crate::ui::design::accent(cx)
                                } else {
                                    crate::ui::design::rose(cx)
                                })
                                .child(Icon::new(icon).size(crate::ui::design::icon_sm()))
                                .child(label),
                        )
                    })
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            ("edit-db-connection", index),
                            "Edit",
                            cx,
                        )
                        .flex_none()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.active_row = Some(index);
                            this.choosing_provider = false;
                            this.validation_error = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        crate::ui::style::destructive_icon_button(
                            ("remove-db-connection", index),
                            cx,
                        )
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
            }))
            .when(self.choosing_provider, |list| {
                list.child(self.render_provider_picker(cx))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "add-db-connection",
                            "Add connection",
                            cx,
                        )
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

    fn render_provider_picker(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .pt_1()
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t3(cx))
                    .child("Choose a database provider"),
            )
            .child(
                h_flex().w_full().gap_2().flex_wrap().children(
                    DB_PROVIDERS
                        .iter()
                        .copied()
                        .enumerate()
                        .map(|(provider_index, provider)| {
                            v_flex()
                                .id(("add-db-provider", provider_index))
                                .w(px(176.))
                                .h(px(82.))
                                .gap_1p5()
                                .rounded(crate::ui::design::r_sm())
                                .border_1()
                                .border_color(crate::ui::design::line(cx).opacity(0.5))
                                .bg(crate::ui::design::base(cx).opacity(0.35))
                                .px_2p5()
                                .py_2()
                                .cursor_pointer()
                                .hover(|choice| choice.bg(crate::ui::design::hover(cx)))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(super::provider_brand_mark(provider, 26.))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(crate::ui::design::t1(cx))
                                                .child(provider.display_name()),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(provider_picker_description(provider)),
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

impl Render for DbConnectionsEditor {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = v_flex()
            .gap_3()
            .w(px(DB_CONNECTION_EDITOR_CONTENT_WIDTH))
            .max_h(px(620.))
            .overflow_y_scrollbar();

        let Some(index) = self.active_row else {
            return editor
                .child(self.render_connection_list(cx))
                .into_any_element();
        };
        let Some(row) = self.rows.get(index) else {
            self.active_row = None;
            return editor
                .child(self.render_connection_list(cx))
                .into_any_element();
        };
        let provider = row.provider;
        let read_only = row.read_only;
        let show_help = row.show_help;
        let test_status = self.connection_tests.get(&row.id);
        let test_loading = matches!(test_status, Some(Loaded::Loading));
        let connection_input = if provider == DbProvider::SQLite {
            Input::new(&row.uri)
                .w_full()
                .min_w(px(320.))
                .into_any_element()
        } else {
            Input::new(&row.uri)
                .w_full()
                .min_w(px(440.))
                .mask_toggle()
                .into_any_element()
        };

        editor
            .child(
                v_flex()
                    .id(("database-connection-form", index))
                    .w_full()
                    .gap_3()
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "back-to-db-connections",
                            "All connections",
                            cx,
                        )
                        .flex_none()
                        .icon(IconName::ChevronLeft)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.active_row = None;
                            this.validation_error = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2p5()
                            .items_center()
                            .child(super::provider_brand_mark(provider, 32.))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(format!(
                                                "{} connection",
                                                provider.display_name()
                                            )),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(provider_form_description(provider)),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(db_form_field_label("Connection name", cx))
                            .child(
                                div()
                                    .w_full()
                                    .min_w(px(440.))
                                    .child(Input::new(&row.name).w_full()),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1()
                            .child(db_form_field_label(provider_connection_label(provider), cx))
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(if provider == DbProvider::SQLite {
                                                px(320.)
                                            } else {
                                                px(440.)
                                            })
                                            .child(connection_input),
                                    )
                                    .when(provider == DbProvider::SQLite, |field| {
                                        field.child(
                                            crate::ui::style::dialog_neutral_button(
                                                "choose-sqlite-file",
                                                "Choose file",
                                                cx,
                                            )
                                            .icon(IconName::FolderOpen)
                                            .on_click(
                                                cx.listener(move |this, _, window, cx| {
                                                    this.choose_sqlite_file(index, window, cx);
                                                }),
                                            ),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .line_height(gpui::relative(1.45))
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(provider_connection_help(provider)),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_1p5()
                            .child(db_form_field_label("Access", cx))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        crate::ui::style::secondary_button_compact(
                                            "db-access-read-only",
                                            "Read only",
                                        )
                                        .icon(IconName::Eye)
                                        .selected(read_only)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.set_read_only(index, true, cx);
                                            }),
                                        ),
                                    )
                                    .child(
                                        crate::ui::style::secondary_button_compact(
                                            "db-access-writes",
                                            "Allow edits",
                                        )
                                        .icon(IconName::Inspector)
                                        .selected(!read_only)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                this.set_read_only(index, false, cx);
                                            }),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(access_help(provider, read_only)),
                            ),
                    )
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
                                    .id(("db-provider-help", index))
                                    .w_full()
                                    .gap_1p5()
                                    .items_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_help(index, cx);
                                    }))
                                    .child(
                                        Icon::new(if show_help {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(crate::ui::design::icon_sm())
                                        .text_color(crate::ui::design::t3(cx)),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(provider_help_title(provider)),
                                    ),
                            )
                            .when(show_help, |help| {
                                help.children(provider_help_steps(provider).iter().enumerate().map(
                                    |(step_index, step)| {
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .line_height(gpui::relative(1.4))
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(SharedString::from(format!(
                                                "{}. {}",
                                                step_index + 1,
                                                step
                                            )))
                                    },
                                ))
                            }),
                    )
                    .when_some(self.validation_error.clone(), |form, error| {
                        form.child(
                            div()
                                .w_full()
                                .text_size(crate::ui::design::text_ui())
                                .line_height(gpui::relative(1.4))
                                .text_color(crate::ui::design::rose(cx))
                                .whitespace_normal()
                                .child(error),
                        )
                    })
                    .when_some(test_result_message(test_status), |form, (message, ok)| {
                        form.child(
                            h_flex()
                                .w_full()
                                .gap_1p5()
                                .items_start()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(if ok {
                                    crate::ui::design::accent(cx)
                                } else {
                                    crate::ui::design::rose(cx)
                                })
                                .child(
                                    Icon::new(if ok {
                                        IconName::CircleCheck
                                    } else {
                                        IconName::CircleX
                                    })
                                    .size(crate::ui::design::icon_sm()),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .line_height(gpui::relative(1.4))
                                        .whitespace_normal()
                                        .child(SharedString::from(message)),
                                ),
                        )
                    })
                    .child(
                        h_flex().child(
                            crate::ui::style::dialog_neutral_button(
                                "test-db-connection",
                                if test_loading {
                                    "Testing connection"
                                } else {
                                    "Test connection"
                                },
                                cx,
                            )
                            .icon(IconName::Redo2)
                            .loading(test_loading)
                            .disabled(test_loading)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.test_connection(index, cx);
                                },
                            )),
                        ),
                    ),
            )
            .into_any_element()
    }
}

const DB_PROVIDERS: [DbProvider; 6] = [
    DbProvider::MongoDb,
    DbProvider::PostgreSql,
    DbProvider::Supabase,
    DbProvider::SQLite,
    DbProvider::MySql,
    DbProvider::MariaDb,
];

fn provider_placeholder(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "mongodb://localhost:27017 or mongodb+srv://… (${VAR} ok)",
        DbProvider::PostgreSql => "postgresql://user:${PASSWORD}@host:5432/database",
        DbProvider::Supabase => {
            "Paste Supabase Connect → Direct connection URL (port 5432; ${VAR} ok)"
        }
        DbProvider::SQLite => "/absolute/path/to/database.sqlite",
        DbProvider::MySql | DbProvider::MariaDb => "mysql://user:${PASSWORD}@host:3306/database",
    }
}

fn provider_name_placeholder(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "e.g. Production MongoDB",
        DbProvider::PostgreSql => "e.g. Staging PostgreSQL",
        DbProvider::Supabase => "e.g. Product Supabase",
        DbProvider::SQLite => "e.g. Local app database",
        DbProvider::MySql => "e.g. Analytics MySQL",
        DbProvider::MariaDb => "e.g. Production MariaDB",
    }
}

fn provider_connection_label(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "MongoDB connection string",
        DbProvider::PostgreSql => "PostgreSQL connection string",
        DbProvider::Supabase => "Supabase Direct connection URL",
        DbProvider::SQLite => "SQLite database file",
        DbProvider::MySql => "MySQL connection string",
        DbProvider::MariaDb => "MariaDB connection string",
    }
}

fn provider_picker_description(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "Document database URI",
        DbProvider::PostgreSql => "Postgres server URL",
        DbProvider::Supabase => "Direct Postgres URL",
        DbProvider::SQLite => "Local database file",
        DbProvider::MySql => "MySQL server URL",
        DbProvider::MariaDb => "MariaDB server URL",
    }
}

fn provider_form_description(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "Browse databases and edit JSON documents by _id.",
        DbProvider::PostgreSql => "Browse schemas, tables, and views on any PostgreSQL server.",
        DbProvider::Supabase => {
            "Connect a Supabase project using its copy-paste Direct connection URL."
        }
        DbProvider::SQLite => "Open an existing SQLite file directly from this Mac.",
        DbProvider::MySql => "Browse MySQL databases, tables, and views.",
        DbProvider::MariaDb => "Browse MariaDB databases, tables, and views.",
    }
}

fn provider_connection_help(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => {
            "Paste a mongodb:// or mongodb+srv:// URI. Passwords may use env vars such as ${MONGO_PASSWORD}."
        }
        DbProvider::PostgreSql => {
            "Use a postgres:// or postgresql:// URL. You can reference secrets with ${POSTGRES_PASSWORD}."
        }
        DbProvider::Supabase => {
            "In Supabase, open Connect, select Direct connection, and paste its port 5432 URL. The port 6543 transaction pooler is not supported."
        }
        DbProvider::SQLite => {
            "Choose an existing .sqlite, .sqlite3, or .db file. Choro will not create a missing file."
        }
        DbProvider::MySql => {
            "Use a mysql:// URL. You can reference secrets with ${MYSQL_PASSWORD}."
        }
        DbProvider::MariaDb => {
            "Use a mysql:// URL for MariaDB. You can reference secrets with ${MARIADB_PASSWORD}."
        }
    }
}

fn provider_help_title(provider: DbProvider) -> &'static str {
    match provider {
        DbProvider::MongoDb => "Connect MongoDB",
        DbProvider::PostgreSql => "Connect PostgreSQL",
        DbProvider::Supabase => "Connect Supabase",
        DbProvider::SQLite => "Open SQLite",
        DbProvider::MySql => "Connect MySQL",
        DbProvider::MariaDb => "Connect MariaDB",
    }
}

fn provider_help_steps(provider: DbProvider) -> &'static [&'static str] {
    match provider {
        DbProvider::MongoDb => &[
            "Copy the connection string from MongoDB Atlas or your MongoDB server.",
            "Paste it above, choose the access mode, then test the connection.",
        ],
        DbProvider::PostgreSql => &[
            "Copy a PostgreSQL connection URL that includes the host, port, user, and database.",
            "Keep Read only enabled until you intentionally want primary-key row editing.",
        ],
        DbProvider::Supabase => &[
            "Open the Supabase project dashboard and choose Connect.",
            "In the Connect dialog, select Direct connection.",
            "Copy the URI on port 5432, replace [YOUR-PASSWORD], paste it above, and test it.",
        ],
        DbProvider::SQLite => &[
            "Choose a database file already present on this Mac.",
            "Allow edits only when Choro should write directly to that local file.",
        ],
        DbProvider::MySql => &[
            "Copy a MySQL URL containing the host, port, user, password, and database.",
            "Keep Read only enabled until you intentionally want primary-key row editing.",
        ],
        DbProvider::MariaDb => &[
            "Copy a MariaDB connection as a mysql:// URL with its host, credentials, and database.",
            "Keep Read only enabled until you intentionally want primary-key row editing.",
        ],
    }
}

fn access_help(provider: DbProvider, read_only: bool) -> &'static str {
    if read_only {
        return "Browsing and filtering are allowed; Choro will not expose edit actions.";
    }
    if provider == DbProvider::MongoDb {
        "Documents with an _id can be edited. Production connections require confirmation."
    } else {
        "Tables with a primary key can be edited. Views and keyless tables remain read-only."
    }
}

fn connection_summary(provider: DbProvider, read_only: bool, configured: bool) -> String {
    let configuration = if configured {
        if provider == DbProvider::SQLite {
            "File selected"
        } else {
            "Connection string set"
        }
    } else {
        "Not configured"
    };
    format!(
        "{} · {} · {configuration}",
        provider.display_name(),
        if read_only {
            "Read only"
        } else {
            "Edits allowed"
        }
    )
}

fn db_form_field_label(text: impl Into<SharedString>, cx: &App) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t3(cx))
        .child(text.into())
        .into_any_element()
}

fn test_status_label(status: Option<&Loaded<()>>) -> Option<(IconName, &'static str, bool)> {
    match status {
        Some(Loaded::Loading) => Some((IconName::LoaderCircle, "Testing", true)),
        Some(Loaded::Ready(())) => Some((IconName::CircleCheck, "Connected", true)),
        Some(Loaded::Failed(_)) => Some((IconName::CircleX, "Failed", false)),
        None => None,
    }
}

fn test_result_message(status: Option<&Loaded<()>>) -> Option<(String, bool)> {
    match status {
        Some(Loaded::Ready(())) => Some(("Connection successful.".into(), true)),
        Some(Loaded::Failed(error)) => Some((format!("Connection failed: {error}"), false)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_providers_have_specific_form_copy() {
        let labels = DB_PROVIDERS
            .iter()
            .copied()
            .map(provider_connection_label)
            .collect::<HashSet<_>>();
        assert_eq!(labels.len(), DB_PROVIDERS.len());
        assert!(provider_connection_help(DbProvider::Supabase).contains("port 5432"));
        assert!(provider_connection_help(DbProvider::Supabase).contains("6543"));
        assert!(provider_connection_help(DbProvider::Supabase).contains("Direct connection"));
        assert_eq!(
            provider_help_steps(DbProvider::Supabase)[1],
            "In the Connect dialog, select Direct connection."
        );
        assert_eq!(
            provider_connection_label(DbProvider::SQLite),
            "SQLite database file"
        );
        assert_eq!(
            provider_connection_label(DbProvider::MongoDb),
            "MongoDB connection string"
        );
    }
}
