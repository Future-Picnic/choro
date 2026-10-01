//! Headless layout checks for the database workspace home and context bar.
//! No database is contacted: explorer state is seeded directly.

use std::path::PathBuf;

use gpui::{
    div, px, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, WeakEntity, Window,
};
use ide_core::{DbConnection, DbObject, DbObjectKind, DbProvider};
use uuid::Uuid;

use super::db_panel::{DbPanel, Loaded};
use super::workspace::{context_bar, DbTabMeta, DB_CONTEXT_BAR_H};
use crate::state::Workspace;

const WIDTHS: [(f32, f32); 4] = [(1100., 700.), (700., 520.), (560., 480.), (1100., 700.)];

fn connections() -> Vec<DbConnection> {
    vec![
        DbConnection {
            id: Uuid::new_v4(),
            provider: DbProvider::SQLite,
            read_only: false,
            name: "Local fixtures".into(),
            uri: "/__choro_missing_layout_fixture__/db.sqlite".into(),
        },
        DbConnection {
            id: Uuid::new_v4(),
            provider: DbProvider::PostgreSql,
            read_only: true,
            name: "An unusually long production analytics warehouse connection name".into(),
            uri: "postgres://reader:${PG_PASSWORD}@db.example.com:5432/analytics".into(),
        },
        DbConnection {
            id: Uuid::new_v4(),
            provider: DbProvider::ClickHouse,
            read_only: true,
            name: "Events".into(),
            uri: "http://localhost:8123/events".into(),
        },
    ]
}

struct HomeFixture {
    panel: Entity<DbPanel>,
    workspace: Entity<Workspace>,
    connections: Vec<DbConnection>,
}

impl Render for HomeFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(super::home::render_home(
            Some(self.panel.clone()),
            self.workspace.clone(),
            self.connections.clone(),
            cx,
        ))
    }
}

#[gpui::test]
fn home_cards_fit_ordinary_and_narrow_panes_in_every_connection_state(
    cx: &mut gpui::TestAppContext,
) {
    cx.update(gpui_component::init);
    let saved = connections();
    let mut fixture = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let project = ide_core::Project::from_path(PathBuf::from("/in-memory/db-home"));
        let config = ide_core::AppConfig {
            active_project: Some(project.id),
            projects: vec![project],
            ..Default::default()
        };
        let workspace = cx.new(|_| Workspace::in_memory(config));
        let panel = DbPanel::view(workspace.clone(), WeakEntity::new_invalid(), cx);
        panel.update(cx, |panel, _| {
            let objects = (0..60)
                .map(|index| DbObject {
                    name: format!("an_unusually_long_table_name_number_{index}"),
                    kind: if index % 5 == 0 {
                        DbObjectKind::View
                    } else {
                        DbObjectKind::Table
                    },
                })
                .collect();
            panel.seed_for_test(
                saved[0].id,
                Loaded::Ready(vec!["main".into()]),
                Some(("main".into(), Loaded::Ready(objects))),
            );
            panel.seed_for_test(
                saved[1].id,
                Loaded::Failed(
                    "connection refused: the server at db.example.com:5432 did not accept a \
                     connection within 7 seconds. Check the host, port and network access."
                        .into(),
                ),
                None,
            );
        });
        let view = cx.new(|_| HomeFixture {
            panel,
            workspace,
            connections: saved.clone(),
        });
        fixture = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    assert!(fixture.is_some());

    for (width, height) in WIDTHS {
        cx.simulate_resize(gpui::size(px(width), px(height)));
        cx.run_until_parked();
        let home = cx.debug_bounds("db-home").expect("home renders");
        let cards = [
            "db-home-connection-0",
            "db-home-connection-1",
            "db-home-connection-2",
        ];
        assert_eq!(cards.len(), saved.len());
        for (index, selector) in cards.into_iter().enumerate() {
            let card = cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("card {index} missing at {width}"));
            assert!(
                card.left() >= home.left() && card.right() <= home.right(),
                "card {index} overflows the home at {width}x{height}"
            );
        }
        let ready = cx.debug_bounds("db-home-connection-0").unwrap();
        let first_object = cx
            .debug_bounds("db-home-first-object-0")
            .expect("a connected namespace lists its objects");
        assert!(first_object.top() > ready.top());
        assert!(
            first_object.right() <= ready.right() && first_object.bottom() <= ready.bottom(),
            "object grid escapes its card at {width}x{height}"
        );
        let failed = cx.debug_bounds("db-home-connection-1").unwrap();
        assert!(failed.top() >= ready.bottom(), "cards overlap at {width}");
        assert!(
            cx.debug_bounds("db-home-first-object-1").is_none(),
            "a failed connection must not list objects"
        );
    }
}

#[gpui::test]
fn home_without_connections_offers_supported_providers(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|_| Workspace::in_memory(Default::default()));
        let panel = DbPanel::view(workspace.clone(), WeakEntity::new_invalid(), cx);
        let view = cx.new(|_| HomeFixture {
            panel,
            workspace,
            connections: Vec::new(),
        });
        gpui_component::Root::new(view, window, cx)
    });
    cx.simulate_resize(gpui::size(px(560.), px(420.)));
    cx.run_until_parked();
    assert!(cx.debug_bounds("db-home").is_some());
    assert!(cx.debug_bounds("db-home-connection-0").is_none());
}

struct ContextFixture {
    meta: DbTabMeta,
}

impl Render for ContextFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .debug_selector(|| "db-context-root".into())
            .size_full()
            .child(context_bar(&self.meta, false, cx))
    }
}

#[gpui::test]
fn context_bar_truncates_long_paths_and_keeps_access_visible(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|_| ContextFixture {
            meta: DbTabMeta {
                connection_name: "An unusually long production analytics connection".into(),
                provider: DbProvider::PostgreSql,
                namespace: "an_unusually_long_schema_name_for_reporting".into(),
                object: "an_unusually_long_table_name_for_customer_events".into(),
                kind: DbObjectKind::Table,
                prod: true,
            },
        });
        gpui_component::Root::new(view, window, cx)
    });
    for (width, height) in WIDTHS {
        cx.simulate_resize(gpui::size(px(width), px(height)));
        cx.run_until_parked();
        let root = cx.debug_bounds("db-context-root").unwrap();
        let bar = cx.debug_bounds("db-context-bar").unwrap();
        let path = cx.debug_bounds("db-context-path").unwrap();
        let access = cx.debug_bounds("db-context-access").unwrap();
        assert_eq!(bar.size.height, px(DB_CONTEXT_BAR_H));
        assert!(bar.right() <= root.right());
        assert!(
            path.right() <= access.left(),
            "path overlaps access at {width}"
        );
        assert!(
            access.right() <= bar.right(),
            "access posture clipped at {width}x{height}"
        );
    }
}
