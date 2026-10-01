use std::collections::{HashMap, HashSet};

use gpui::{
    div, px, App, AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    spinner::Spinner,
    v_flex, Icon, IconName, Sizable,
};
use ide_core::{DatabaseHandle, DbConnection, DbObject, DbProvider};
use uuid::Uuid;

pub use super::connection_editor::DbConnectionsEditor;
use super::providers::effective_read_only;
use crate::state::Workspace;
use crate::ui::center::CenterArea;
use crate::ui::style;

pub(crate) mod tree_rows;

/// Async-loaded tree level: in flight / done / failed.
pub(crate) enum Loaded<T> {
    Loading,
    Ready(T),
    Failed(String),
}

/// Right panel tab: saved database connections for the active project,
/// expanding lazily to namespaces and database objects. Each level loads only
/// when opened (DBFlux's shallow schema fetch), and every async result is
/// checked against the connection's epoch so a reconnect or edit can never be
/// overwritten by a slower, older response.
pub struct DbPanel {
    workspace: Entity<Workspace>,
    center: gpui::WeakEntity<CenterArea>,
    expanded_conns: HashSet<Uuid>,
    expanded_dbs: HashSet<(Uuid, String)>,
    connection_specs: HashMap<Uuid, (DbProvider, String, bool)>,
    handles: HashMap<Uuid, DatabaseHandle>,
    epochs: HashMap<Uuid, u64>,
    databases: HashMap<Uuid, Loaded<Vec<String>>>,
    collections: HashMap<(Uuid, String), Loaded<Vec<DbObject>>>,
    /// The namespace each connection shows on the workspace home.
    home_focus: HashMap<Uuid, String>,
    filter: Option<Entity<InputState>>,
}

impl DbPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        center: gpui::WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            // Saved connections drive cached sessions whether or not the
            // explorer is mounted: the workspace home uses this state too,
            // and the side panel can be hidden.
            cx.observe(&workspace, |this: &mut Self, _, cx| {
                this.sync_from_workspace(cx);
                cx.notify();
            })
            .detach();
            let mut panel = Self {
                workspace,
                center,
                expanded_conns: HashSet::new(),
                expanded_dbs: HashSet::new(),
                connection_specs: HashMap::new(),
                handles: HashMap::new(),
                epochs: HashMap::new(),
                databases: HashMap::new(),
                collections: HashMap::new(),
                home_focus: HashMap::new(),
                filter: None,
            };
            panel.sync_from_workspace(cx);
            panel
        })
    }

    /// Reconciles cached sessions with every saved connection in every
    /// project. Using all projects (not just the active one) means switching
    /// projects never retires sessions that open tabs still use.
    fn sync_from_workspace(&mut self, cx: &mut Context<Self>) {
        let connections = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .flat_map(|project| project.db_connections.iter().cloned())
            .collect::<Vec<_>>();
        self.sync_connection_specs(&connections, cx);
    }

    /// The access a connection's sessions must have now, from the saved
    /// settings rather than a possibly stale copy captured by a click.
    fn current_read_only(&self, conn: &DbConnection) -> bool {
        self.connection_specs
            .get(&conn.id)
            .map(|spec| spec.2)
            .unwrap_or_else(|| effective_read_only(conn.provider, conn.read_only))
    }

    /// Namespace level of a saved connection; `None` until it is opened.
    pub(crate) fn namespaces(&self, id: Uuid) -> Option<&Loaded<Vec<String>>> {
        self.databases.get(&id)
    }

    /// Object level of one namespace; `None` until it is opened.
    pub(crate) fn objects(&self, id: Uuid, namespace: &str) -> Option<&Loaded<Vec<DbObject>>> {
        self.collections.get(&(id, namespace.to_string()))
    }

    pub(crate) fn focused_namespace(&self, id: Uuid) -> Option<&str> {
        self.home_focus.get(&id).map(String::as_str)
    }

    /// Places loaded levels directly, for headless layout tests that must
    /// not contact a database.
    #[cfg(test)]
    pub(crate) fn seed_for_test(
        &mut self,
        id: Uuid,
        namespaces: Loaded<Vec<String>>,
        focus: Option<(String, Loaded<Vec<DbObject>>)>,
    ) {
        self.expanded_conns.insert(id);
        self.databases.insert(id, namespaces);
        if let Some((namespace, objects)) = focus {
            self.expanded_dbs.insert((id, namespace.clone()));
            self.collections.insert((id, namespace.clone()), objects);
            self.home_focus.insert(id, namespace);
        }
    }

    /// Opens a connection from outside the tree (the workspace home): expands
    /// it in the explorer and loads its namespaces once.
    pub(crate) fn connect(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        self.expanded_conns.insert(conn.id);
        if !matches!(
            self.databases.get(&conn.id),
            Some(Loaded::Ready(_) | Loaded::Loading)
        ) {
            self.load_databases(conn, cx);
        }
        cx.notify();
    }

    /// Shows one namespace's objects on the home and in the explorer.
    pub(crate) fn focus_namespace(
        &mut self,
        conn: DbConnection,
        namespace: String,
        cx: &mut Context<Self>,
    ) {
        let key = (conn.id, namespace.clone());
        self.expanded_conns.insert(conn.id);
        self.expanded_dbs.insert(key.clone());
        self.home_focus.insert(conn.id, namespace.clone());
        if !matches!(
            self.collections.get(&key),
            Some(Loaded::Ready(_) | Loaded::Loading)
        ) {
            self.load_collections(conn, namespace, cx);
        }
        cx.notify();
    }

    fn epoch(&self, id: Uuid) -> u64 {
        self.epochs.get(&id).copied().unwrap_or_default()
    }

    /// Forgets everything derived from a connection's endpoint and invalidates
    /// any in-flight loads for it.
    fn reset_connection(&mut self, id: Uuid) {
        *self.epochs.entry(id).or_default() += 1;
        if let Some(retired) = self.handles.remove(&id) {
            // Tabs can outlive the cached connection. Never permit writes to a
            // former endpoint after reconnecting or editing the connection.
            retired.set_read_only(true);
        }
        self.databases.remove(&id);
        self.collections.retain(|(conn, _), _| *conn != id);
    }

    /// A pooled client per connection. Construction is cheap and lazy; the
    /// first query does the blocking work on a background executor.
    fn handle_for(&mut self, conn: &DbConnection) -> anyhow::Result<DatabaseHandle> {
        match self.connection_specs.get(&conn.id) {
            // A click captured before the connection was edited must not
            // open a session to the former endpoint.
            Some(spec) if spec.0 != conn.provider || spec.1 != conn.uri => {
                anyhow::bail!("This connection's settings changed. Open it again.");
            }
            Some(_) => {}
            None => {
                // Specs are reconciled from saved settings at construction
                // and on workspace changes. An old click must not recreate
                // a connection that the user has since removed.
                anyhow::bail!("This connection was removed. Choose a saved connection.");
            }
        }
        if let Some(handle) = self.handles.get(&conn.id) {
            return Ok(handle.clone());
        }
        let handle = DatabaseHandle::connect(conn)?;
        handle.set_read_only(self.current_read_only(conn));
        self.handles.insert(conn.id, handle.clone());
        Ok(handle)
    }

    fn toggle_connection(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        if !self.expanded_conns.insert(conn.id) {
            self.expanded_conns.remove(&conn.id);
            cx.notify();
            return;
        }
        if !matches!(
            self.databases.get(&conn.id),
            Some(Loaded::Ready(_) | Loaded::Loading)
        ) {
            self.load_databases(conn, cx);
        }
        cx.notify();
    }

    fn load_databases(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        let conn_id = conn.id;
        let epoch = self.epoch(conn_id);
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
                if this.epoch(conn_id) != epoch {
                    return;
                }
                // A lone namespace (SQLite's `main`, a single Mongo database)
                // needs no choice: open it so its objects are one click away.
                let lone = match &result {
                    Ok(dbs) if dbs.len() == 1 && !this.home_focus.contains_key(&conn_id) => {
                        Some(dbs[0].clone())
                    }
                    _ => None,
                };
                this.databases.insert(
                    conn_id,
                    match result {
                        Ok(dbs) => Loaded::Ready(dbs),
                        Err(error) => Loaded::Failed(format!("{error:#}")),
                    },
                );
                if let Some(namespace) = lone {
                    this.focus_namespace(conn.clone(), namespace, cx);
                }
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
        if !matches!(
            self.collections.get(&key),
            Some(Loaded::Ready(_) | Loaded::Loading)
        ) {
            self.load_collections(conn, db, cx);
        }
        cx.notify();
    }

    fn load_collections(&mut self, conn: DbConnection, db: String, cx: &mut Context<Self>) {
        let key = (conn.id, db.clone());
        let epoch = self.epoch(conn.id);
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
                if this.epoch(key.0) != epoch {
                    return;
                }
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
        cx.notify();
    }

    pub(crate) fn open_object(
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
                self.databases
                    .insert(conn.id, Loaded::Failed(format!("{error:#}")));
                cx.notify();
                return;
            }
        };
        // Opening a pane applies this access to the shared session, so it
        // must come from the saved settings, never from a stale copy.
        let conn = DbConnection {
            read_only: self.current_read_only(&conn),
            ..conn
        };
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_db_object(handle, conn, db, object, window, cx);
            });
        }
    }

    /// Reconnects: drops the cached client and every loaded level for the
    /// connection, then reloads its namespaces and any open namespaces.
    pub(crate) fn refresh_connection(&mut self, conn: DbConnection, cx: &mut Context<Self>) {
        self.reset_connection(conn.id);
        self.expanded_conns.insert(conn.id);
        let reopen = self
            .expanded_dbs
            .iter()
            .filter(|(id, _)| *id == conn.id)
            .map(|(_, db)| db.clone())
            .collect::<Vec<_>>();
        self.load_databases(conn.clone(), cx);
        for db in reopen {
            self.load_collections(conn.clone(), db, cx);
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
            let read_only = effective_read_only(connection.provider, connection.read_only);
            let next = (connection.provider, connection.uri.clone(), read_only);
            let previous = self.connection_specs.get(&connection.id);
            let endpoint_changed =
                previous.is_some_and(|current| current.0 != next.0 || current.1 != next.1);
            let access_changed = previous.is_some_and(|current| current.2 != next.2);
            if access_changed && !endpoint_changed {
                if let Some(handle) = self.handles.get(&connection.id) {
                    handle.set_read_only(read_only);
                }
            }
            self.connection_specs.insert(connection.id, next);
            if endpoint_changed {
                self.reset_connection(connection.id);
                self.expanded_dbs.retain(|(id, _)| *id != connection.id);
                self.home_focus.remove(&connection.id);
                if self.expanded_conns.contains(&connection.id) {
                    reload.push(connection.clone());
                }
            }
        }

        self.connection_specs
            .retain(|id, _| current_ids.contains(id));
        self.handles.retain(|id, handle| {
            let keep = current_ids.contains(id);
            if !keep {
                handle.set_read_only(true);
            }
            keep
        });
        self.epochs.retain(|id, _| current_ids.contains(id));
        self.databases.retain(|id, _| current_ids.contains(id));
        self.collections
            .retain(|(id, _), _| current_ids.contains(id));
        self.expanded_conns.retain(|id| current_ids.contains(id));
        self.expanded_dbs.retain(|(id, _)| current_ids.contains(id));
        self.home_focus.retain(|id, _| current_ids.contains(id));

        for connection in reload {
            self.load_databases(connection, cx);
        }
    }

    fn ensure_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some(filter) = &self.filter {
            return filter.clone();
        }
        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter tables"));
        cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.filter = Some(filter.clone());
        filter
    }

    fn tree_row(&self, indent: usize, content: impl IntoElement, cx: &Context<Self>) -> gpui::Div {
        h_flex()
            .w_full()
            .min_h(px(26.))
            .pl(px(8. + indent as f32 * 16.))
            .pr_1()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .text_size(crate::ui::design::text_body())
            .hover(|style| style.bg(crate::ui::design::hover(cx)))
            .child(content)
    }

    /// A non-interactive status line inside the tree (loading, empty, failed).
    fn tree_note(&self, indent: usize, content: impl IntoElement, cx: &Context<Self>) -> gpui::Div {
        h_flex()
            .w_full()
            .pl(px(8. + indent as f32 * 16. + 18.))
            .pr_2()
            .py_1()
            .gap_1p5()
            .items_start()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(content)
    }

    fn render_failure(
        &self,
        indent: usize,
        id: impl Into<gpui::ElementId>,
        error: &str,
        on_retry: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        self.tree_note(
            indent,
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap_1p5()
                .child(
                    h_flex()
                        .gap_1p5()
                        .items_start()
                        .child(
                            div()
                                .pt(px(2.))
                                .child(crate::ui::design::indicator::lucide_icon(
                                    lucide_icons::Icon::CircleX,
                                    crate::ui::design::rose(cx),
                                    crate::ui::design::icon_sm(),
                                )),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_color(crate::ui::design::t2(cx))
                                .line_height(gpui::relative(1.4))
                                .whitespace_normal()
                                .child(SharedString::from(error.to_string())),
                        ),
                )
                .child(
                    h_flex().child(
                        style::refresh_button(id, "Retry", cx)
                            .on_click(cx.listener(move |this, _, _, cx| on_retry(this, cx))),
                    ),
                ),
            cx,
        )
    }
}

fn matches_filter(name: &str, query: &str) -> bool {
    query.is_empty() || name.to_lowercase().contains(query)
}

/// The center's identity for an open object tab; the explorer uses it to
/// mark the object that is currently on screen.
pub(crate) fn object_key(conn: Uuid, namespace: &str, object: &str) -> String {
    format!("{conn}/{namespace}/{object}")
}

/// Splits a namespace's objects into explorer groups (tables, views,
/// collections), preserving the backend's order within each group.
fn group_by_kind(objects: &[DbObject]) -> Vec<(ide_core::DbObjectKind, Vec<&DbObject>)> {
    use ide_core::DbObjectKind;
    [
        DbObjectKind::Table,
        DbObjectKind::View,
        DbObjectKind::Collection,
    ]
    .into_iter()
    .map(|kind| {
        (
            kind,
            objects
                .iter()
                .filter(|object| object.kind == kind)
                .collect::<Vec<_>>(),
        )
    })
    .filter(|(_, objects)| !objects.is_empty())
    .collect()
}

impl Render for DbPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(project) = self.workspace.read(cx).active_project() else {
            return style::empty_context_panel("DB", "Open a project to browse databases", cx)
                .into_any_element();
        };
        // Sessions are reconciled by the workspace observer, not here, so
        // they stay correct while this panel is hidden.
        let connections = project.db_connections.clone();

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
                .child(self.render_empty(cx))
                .into_any_element();
        }

        let filter = self.ensure_filter(window, cx);
        let query = filter.read(cx).value().trim().to_lowercase();
        let active_object = self
            .center
            .upgrade()
            .and_then(|center| center.read(cx).active_db_object_key(cx));

        let mut list = v_flex()
            .id("db-tree")
            .flex_1()
            .min_h(px(0.))
            .px_1()
            .pb_2()
            .overflow_y_scroll();

        for (cix, conn) in connections.iter().enumerate() {
            list = list.child(self.render_connection_row(cix, conn, cx));
            if !self.expanded_conns.contains(&conn.id) {
                continue;
            }
            match self.databases.get(&conn.id) {
                None | Some(Loaded::Loading) => {
                    list = list.child(
                        self.tree_note(
                            1,
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(Spinner::new().xsmall())
                                .child("Connecting"),
                            cx,
                        ),
                    );
                }
                Some(Loaded::Failed(error)) => {
                    let retry_conn = conn.clone();
                    let error = error.clone();
                    list = list.child(self.render_failure(
                        1,
                        ("db-retry-conn", cix),
                        &error,
                        move |this, cx| this.refresh_connection(retry_conn.clone(), cx),
                        cx,
                    ));
                }
                Some(Loaded::Ready(dbs)) if dbs.is_empty() => {
                    list = list.child(self.tree_note(
                        1,
                        div().child("No schemas are visible to this user."),
                        cx,
                    ));
                }
                Some(Loaded::Ready(dbs)) => {
                    for (dix, db) in dbs.clone().iter().enumerate() {
                        let key = (conn.id, db.clone());
                        let namespace_matches = matches_filter(db, &query);
                        let visible_objects = match self.collections.get(&key) {
                            Some(Loaded::Ready(objects)) => Some(
                                objects
                                    .iter()
                                    .filter(|object| {
                                        namespace_matches || matches_filter(&object.name, &query)
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>(),
                            ),
                            _ => None,
                        };
                        if !namespace_matches
                            && visible_objects
                                .as_ref()
                                .is_some_and(|objects| objects.is_empty())
                        {
                            continue;
                        }
                        list = list.child(self.render_database_row(cix * 1000 + dix, conn, db, cx));
                        if !self.expanded_dbs.contains(&key) {
                            continue;
                        }
                        match self.collections.get(&key) {
                            None | Some(Loaded::Loading) => {
                                list = list.child(
                                    self.tree_note(
                                        2,
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(Spinner::new().xsmall())
                                            .child("Loading tables"),
                                        cx,
                                    ),
                                );
                            }
                            Some(Loaded::Failed(error)) => {
                                let retry_conn = conn.clone();
                                let retry_db = db.clone();
                                let error = error.clone();
                                list = list.child(self.render_failure(
                                    2,
                                    ("db-retry-db", cix * 1000 + dix),
                                    &error,
                                    move |this, cx| {
                                        this.load_collections(
                                            retry_conn.clone(),
                                            retry_db.clone(),
                                            cx,
                                        )
                                    },
                                    cx,
                                ));
                            }
                            Some(Loaded::Ready(_)) => {
                                let visible = visible_objects.unwrap_or_default();
                                if visible.is_empty() {
                                    list = list.child(self.tree_note(
                                        2,
                                        div().child(if query.is_empty() {
                                            "No tables or views"
                                        } else {
                                            "No matching tables"
                                        }),
                                        cx,
                                    ));
                                }
                                let groups = group_by_kind(&visible);
                                let labelled = groups.len() > 1;
                                let mut tix = 0;
                                for (kind, objects) in groups {
                                    if labelled {
                                        list = list.child(self.render_kind_group_row(
                                            kind,
                                            objects.len(),
                                            cx,
                                        ));
                                    }
                                    for object in objects {
                                        let open = active_object.as_deref()
                                            == Some(object_key(conn.id, db, &object.name).as_str());
                                        list = list.child(self.render_object_row(
                                            cix * 100_000 + dix * 1000 + tix,
                                            conn,
                                            db,
                                            object,
                                            open,
                                            cx,
                                        ));
                                        tix += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        v_flex()
            .size_full()
            .child(header)
            .child(
                div().px_2().pb_1p5().child(
                    Input::new(&filter).small().cleanable(true).prefix(
                        Icon::new(IconName::Search)
                            .size(crate::ui::design::icon_sm())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
                ),
            )
            .child(list)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ui-layout-tests")]
    mod session_sync {
        use super::*;
        use crate::ui::db::chrome::handle_read_only;

        fn saved(name: &str, uri: &str) -> DbConnection {
            DbConnection {
                id: Uuid::new_v4(),
                provider: DbProvider::PostgreSql,
                read_only: false,
                name: name.into(),
                uri: uri.into(),
            }
        }

        fn edit_connection(
            workspace: &Entity<Workspace>,
            id: Uuid,
            cx: &mut gpui::TestAppContext,
            edit: impl FnOnce(&mut Vec<DbConnection>),
        ) {
            workspace.update(cx, |workspace, cx| {
                let project = workspace
                    .projects
                    .iter_mut()
                    .find(|project| project.db_connections.iter().any(|conn| conn.id == id))
                    .expect("connection is saved");
                edit(&mut project.db_connections);
                cx.notify();
            });
            cx.run_until_parked();
        }

        /// The explorer is never mounted or rendered here: the workspace home can
        /// drive DbPanel while the side panel is hidden. PostgreSQL handles are
        /// lazy, so no server is contacted.
        #[gpui::test]
        fn saved_connection_edits_reconcile_sessions_without_a_tree_render(
            cx: &mut gpui::TestAppContext,
        ) {
            let primary = saved("Primary", "postgres://reader@db.invalid:5432/app");
            let other = saved("Other project", "postgres://db.invalid:5432/other");
            let mut first = ide_core::Project::from_path("/in-memory/db-sync-a".into());
            first.db_connections = vec![primary.clone()];
            let mut second = ide_core::Project::from_path("/in-memory/db-sync-b".into());
            second.db_connections = vec![other.clone()];
            let config = ide_core::AppConfig {
                active_project: Some(first.id),
                projects: vec![first, second],
                ..Default::default()
            };
            let workspace = cx.new(|_| Workspace::in_memory(config));
            let panel = cx
                .update(|cx| DbPanel::view(workspace.clone(), gpui::WeakEntity::new_invalid(), cx));

            // Specs exist from construction, for every project's connections.
            panel.read_with(cx, |panel, _| {
                assert_eq!(panel.connection_specs.len(), 2);
                assert_eq!(panel.connection_specs[&primary.id].1, primary.uri);
            });

            // A session opened through the real contract, plus a clone standing
            // in for an open tab.
            let (tab, other_tab) = panel.update(cx, |panel, cx| {
                let tab = panel.handle_for(&primary).unwrap();
                let other_tab = panel.handle_for(&other).unwrap();
                panel
                    .databases
                    .insert(primary.id, Loaded::Ready(vec!["public".into()]));
                panel.expanded_dbs.insert((primary.id, "public".into()));
                panel.home_focus.insert(primary.id, "public".into());
                cx.notify();
                (tab, other_tab)
            });
            assert!(!handle_read_only(&tab));

            // Access revoked: the same live session (and the tab) turns read-only.
            edit_connection(&workspace, primary.id, cx, |connections| {
                connections[0].read_only = true;
            });
            panel.update(cx, |panel, _| {
                assert!(handle_read_only(&tab));
                let cached = panel.handles.get(&primary.id).expect("session kept");
                assert!(cached.shares_connection(&tab));
                assert!(panel.connection_specs[&primary.id].2);
                // A stale copy of the old settings cannot re-enable writes.
                assert!(panel.current_read_only(&primary));
            });

            // Endpoint changed: the old session is retired and every cache for it
            // is invalidated; the next session is a different connection.
            let epoch_before = panel.read_with(cx, |panel, _| panel.epoch(primary.id));
            let moved_uri = "postgres://reader@replica.invalid:5432/app";
            edit_connection(&workspace, primary.id, cx, |connections| {
                connections[0].read_only = false;
                connections[0].uri = moved_uri.into();
            });
            let moved = DbConnection {
                uri: moved_uri.into(),
                read_only: false,
                ..primary.clone()
            };
            let replacement = panel.update(cx, |panel, _| {
                assert!(
                    handle_read_only(&tab),
                    "retired session must stay read-only"
                );
                assert!(!panel.handles.contains_key(&primary.id));
                assert!(panel.epoch(primary.id) > epoch_before);
                assert!(panel.databases.get(&primary.id).is_none());
                assert!(panel.focused_namespace(primary.id).is_none());
                assert!(!panel.expanded_dbs.contains(&(primary.id, "public".into())));
                assert_eq!(panel.connection_specs[&primary.id].1, moved_uri);
                // A click captured before the edit cannot reopen the old endpoint.
                assert!(panel.handle_for(&primary).is_err());
                panel.handle_for(&moved).unwrap()
            });
            assert!(!replacement.shares_connection(&tab));
            assert!(!handle_read_only(&replacement));

            // Removed: its session is revoked and forgotten. The other project's
            // session is untouched even though that project is not active.
            edit_connection(&workspace, primary.id, cx, |connections| {
                connections.clear()
            });
            panel.read_with(cx, |panel, _| {
                assert!(handle_read_only(&replacement));
                assert!(!panel.handles.contains_key(&primary.id));
                assert!(!panel.connection_specs.contains_key(&primary.id));
                assert!(!panel.epochs.contains_key(&primary.id));
                assert!(panel.handles.contains_key(&other.id));
            });
            panel.update(cx, |panel, _| {
                assert!(panel.handle_for(&moved).is_err());
                assert!(!panel.handles.contains_key(&primary.id));
                assert!(!panel.connection_specs.contains_key(&primary.id));
            });
            assert!(!handle_read_only(&other_tab));
        }
    }

    #[test]
    fn filter_matching_is_case_insensitive_substring() {
        assert!(matches_filter("Orders", ""));
        assert!(matches_filter("customer_orders", "order"));
        assert!(!matches_filter("invoices", "order"));
    }

    #[test]
    fn explorer_groups_objects_by_kind_in_a_stable_order() {
        use ide_core::DbObjectKind;
        let object = |name: &str, kind| DbObject {
            name: name.into(),
            kind,
        };
        let objects = [
            object("active_users", DbObjectKind::View),
            object("users", DbObjectKind::Table),
            object("orders", DbObjectKind::Table),
        ];
        let groups = group_by_kind(&objects);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, DbObjectKind::Table);
        assert_eq!(groups[0].1[0].name, "users");
        assert_eq!(groups[0].1[1].name, "orders");
        assert_eq!(groups[1].0, DbObjectKind::View);
        assert!(group_by_kind(&[]).is_empty());
    }

    #[test]
    fn object_keys_match_the_center_tab_identity() {
        let id = Uuid::nil();
        assert_eq!(
            object_key(id, "public", "users"),
            format!("{id}/public/users")
        );
    }
}
