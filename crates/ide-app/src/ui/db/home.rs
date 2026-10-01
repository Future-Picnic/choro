//! The database workspace home: every saved connection as a live
//! switchboard. A connection opens in place, its namespaces line up as a
//! rail, and the focused namespace's tables, views or collections open
//! straight into tabs. All state is the explorer's own (`DbPanel`), so the
//! home and the sidebar tree always agree about what is connected.

use gpui::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, Entity, FontWeight,
    InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::{h_flex, spinner::Spinner, tooltip::Tooltip, v_flex, IconName, Sizable as _};
use ide_core::{DbConnection, DbObject};

use super::chrome::object_kind_icon;
use super::db_panel::tree_rows::{connection_mark, connection_state_color, namespace_noun};
use super::db_panel::{DbConnectionsEditor, DbPanel, Loaded};
use super::providers::{effective_read_only, endpoint_summary};
use crate::state::Workspace;
use crate::ui::style;

/// Objects listed per connection before the rest are left to the explorer.
const HOME_OBJECT_LIMIT: usize = 36;
const OBJECT_CELL_W: f32 = 196.;
const HOME_MARK: f32 = 30.;

pub(crate) fn render_home(
    panel: Option<Entity<DbPanel>>,
    workspace: Entity<Workspace>,
    connections: Vec<DbConnection>,
    cx: &App,
) -> AnyElement {
    let header = h_flex()
        .w_full()
        .flex_wrap()
        .gap_3()
        .items_end()
        .child(
            v_flex()
                .flex_1()
                .min_w(px(240.))
                .gap_1()
                .child(
                    div()
                        .text_size(crate::ui::design::text_title())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t1(cx))
                        .child("Databases"),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(if connections.is_empty() {
                            "Save a connection to browse its tables and documents here."
                        } else {
                            "Open a connection, then a table or collection. Each one opens in its own tab."
                        }),
                ),
        )
        .child({
            let workspace = workspace.clone();
            let label = if connections.is_empty() {
                "Add connection"
            } else {
                "Manage connections"
            };
            style::dialog_neutral_button("db-home-manage", label, cx)
                .icon(if connections.is_empty() {
                    IconName::Plus
                } else {
                    IconName::Settings2
                })
                .on_click(move |_, window, cx| {
                    DbConnectionsEditor::open(workspace.clone(), window, cx);
                })
        });

    let body = if connections.is_empty() {
        render_providers(cx)
    } else {
        v_flex()
            .w_full()
            .gap_3()
            .children(
                connections
                    .iter()
                    .enumerate()
                    .map(|(index, conn)| render_connection(index, conn, panel.clone(), cx)),
            )
            .into_any_element()
    };

    div()
        .id("db-home")
        .debug_selector(|| "db-home".into())
        .size_full()
        .overflow_y_scroll()
        .bg(crate::ui::design::base(cx))
        .child(
            v_flex()
                .w_full()
                .max_w(px(crate::ui::design::CENTER_CONTENT_MAX_W))
                .mx_auto()
                .px_6()
                .pt_6()
                .pb_8()
                .gap_5()
                .child(header)
                .child(body),
        )
        .into_any_element()
}

/// With nothing saved yet, the home shows what can be connected.
fn render_providers(cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap_3()
        .p_4()
        .rounded(crate::ui::design::r_md())
        .border_1()
        .border_color(crate::ui::design::line(cx).opacity(0.4))
        .child(
            div()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t2(cx))
                .child("Supported databases"),
        )
        .child(h_flex().w_full().flex_wrap().gap_x_5().gap_y_3().children(
            ide_core::DbProvider::ALL.into_iter().map(|provider| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(super::provider_brand_mark(provider, 20.))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t1(cx))
                            .child(provider.display_name().to_string()),
                    )
            }),
        ))
        .into_any_element()
}

fn render_connection(
    index: usize,
    conn: &DbConnection,
    panel: Option<Entity<DbPanel>>,
    cx: &App,
) -> AnyElement {
    let explorer = panel.as_ref().map(|panel| panel.read(cx));
    let state = explorer.and_then(|explorer| explorer.namespaces(conn.id));
    let read_only = effective_read_only(conn.provider, conn.read_only);
    let prod = conn.looks_like_prod();
    let endpoint: SharedString = endpoint_summary(conn.provider, &conn.uri)
        .unwrap_or_else(|| "Not configured".into())
        .into();

    let identity = h_flex()
        .w_full()
        .flex_wrap()
        .gap_x_3()
        .gap_y_2()
        .items_center()
        .child(
            h_flex()
                .flex_1()
                .min_w(px(220.))
                .gap_3()
                .items_center()
                .child(connection_mark(
                    conn.provider,
                    HOME_MARK,
                    connection_state_color(state, cx),
                    cx,
                ))
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
                                .child(SharedString::from(conn.name.clone())),
                        )
                        .child(
                            h_flex()
                                .min_w(px(0.))
                                .gap_2()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t3(cx))
                                .child(
                                    div()
                                        .flex_none()
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(conn.provider.display_name().to_string()),
                                )
                                .child(
                                    div()
                                        .id(("db-home-endpoint", index))
                                        .min_w(px(0.))
                                        .truncate()
                                        .font_family(crate::ui::design::FONT_MONO)
                                        .child(endpoint.clone())
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(endpoint.clone()).build(window, cx)
                                        }),
                                ),
                        ),
                ),
        )
        .child(super::access_indicators(prod, read_only, cx))
        .when_some(
            panel
                .clone()
                .filter(|_| matches!(state, Some(Loaded::Ready(_) | Loaded::Failed(_)))),
            |row, panel| {
                let conn = conn.clone();
                row.child(
                    style::refresh_icon_button(("db-home-reconnect", index), cx)
                        .tooltip("Reconnect and reload")
                        .on_click(move |_, _, cx| {
                            panel
                                .update(cx, |panel, cx| panel.refresh_connection(conn.clone(), cx));
                        }),
                )
            },
        );

    let detail = match (panel, state) {
        (None, _) => None,
        (Some(panel), None) => Some(render_disconnected(index, conn, panel, cx)),
        (Some(_), Some(Loaded::Loading)) => Some(render_progress("Connecting", cx)),
        (Some(panel), Some(Loaded::Failed(error))) => {
            Some(render_failed(index, conn, error, panel, cx))
        }
        (Some(panel), Some(Loaded::Ready(namespaces))) => {
            Some(render_namespaces(index, conn, namespaces, panel, cx))
        }
    };

    v_flex()
        .id(("db-home-connection", index))
        .debug_selector(move || format!("db-home-connection-{index}"))
        .w_full()
        .rounded(crate::ui::design::r_md())
        .border_1()
        .border_color(if prod {
            crate::ui::design::rose(cx).opacity(0.35)
        } else {
            crate::ui::design::line(cx).opacity(0.4)
        })
        .bg(crate::ui::design::surface(cx))
        .overflow_hidden()
        .child(div().w_full().px_4().py_3().child(identity))
        .when_some(detail, |card, detail| {
            card.child(
                div()
                    .w_full()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.22))
                    .bg(crate::ui::design::base(cx).opacity(0.55))
                    .child(detail),
            )
        })
        .into_any_element()
}

fn render_disconnected(
    index: usize,
    conn: &DbConnection,
    panel: Entity<DbPanel>,
    cx: &App,
) -> AnyElement {
    let (_, many) = namespace_noun(conn.provider);
    let conn = conn.clone();
    h_flex()
        .w_full()
        .px_4()
        .py_2p5()
        .gap_3()
        .items_center()
        .child(
            style::primary_button_compact(("db-home-connect", index), "Connect", cx).on_click(
                move |_, _, cx| {
                    panel.update(cx, |panel, cx| panel.connect(conn.clone(), cx));
                },
            ),
        )
        .child(
            div()
                .min_w(px(0.))
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(format!("Opens a session and lists its {many}.")),
        )
        .into_any_element()
}

fn render_progress(label: &'static str, cx: &App) -> AnyElement {
    h_flex()
        .w_full()
        .px_4()
        .py_3()
        .gap_2()
        .items_center()
        .text_size(crate::ui::design::text_ui())
        .text_color(crate::ui::design::t3(cx))
        .child(Spinner::new().xsmall())
        .child(label)
        .into_any_element()
}

fn render_failed(
    index: usize,
    conn: &DbConnection,
    error: &str,
    panel: Entity<DbPanel>,
    cx: &App,
) -> AnyElement {
    let conn = conn.clone();
    h_flex()
        .w_full()
        .px_4()
        .py_2p5()
        .gap_2()
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
                .text_size(crate::ui::design::text_ui())
                .line_height(gpui::relative(1.45))
                .text_color(crate::ui::design::t2(cx))
                .whitespace_normal()
                .child(SharedString::from(error.to_string())),
        )
        .child(
            style::refresh_button(("db-home-retry", index), "Retry", cx).on_click(
                move |_, _, cx| {
                    panel.update(cx, |panel, cx| panel.refresh_connection(conn.clone(), cx));
                },
            ),
        )
        .into_any_element()
}

fn render_namespaces(
    index: usize,
    conn: &DbConnection,
    namespaces: &[String],
    panel: Entity<DbPanel>,
    cx: &App,
) -> AnyElement {
    let (one, many) = namespace_noun(conn.provider);
    if namespaces.is_empty() {
        return div()
            .px_4()
            .py_3()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(format!("No {many} are visible to this user."))
            .into_any_element();
    }
    let explorer = panel.read(cx);
    let focused = explorer
        .focused_namespace(conn.id)
        .filter(|namespace| namespaces.iter().any(|known| known == namespace))
        .map(str::to_string);

    let rail = h_flex()
        .w_full()
        .flex_wrap()
        .gap_1()
        .items_center()
        .children(namespaces.iter().enumerate().map(|(ns_index, namespace)| {
            let selected = focused.as_deref() == Some(namespace.as_str());
            let count = match explorer.objects(conn.id, namespace) {
                Some(Loaded::Ready(objects)) => Some(objects.len()),
                _ => None,
            };
            let panel = panel.clone();
            let conn = conn.clone();
            let target = namespace.clone();
            h_flex()
                .id(("db-home-namespace", index * 10_000 + ns_index))
                .h(px(26.))
                .max_w(px(260.))
                .px_2p5()
                .gap_1p5()
                .items_center()
                .rounded(crate::ui::design::r_sm())
                .cursor_pointer()
                .text_size(crate::ui::design::text_ui())
                .when(selected, |pill| {
                    pill.bg(crate::ui::design::accent_soft(cx))
                        .text_color(crate::ui::design::t1(cx))
                        .font_weight(FontWeight::MEDIUM)
                })
                .when(!selected, |pill| {
                    pill.text_color(crate::ui::design::t2(cx))
                        .hover(|pill| pill.bg(crate::ui::design::hover(cx)))
                })
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::Layers,
                    if selected {
                        crate::ui::design::accent(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    },
                    crate::ui::design::icon_sm(),
                ))
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .child(SharedString::from(namespace.clone())),
                )
                .when_some(count, |pill, count| {
                    pill.child(
                        div()
                            .flex_none()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(count.to_string()),
                    )
                })
                .on_click(move |_, _, cx| {
                    panel.update(cx, |panel, cx| {
                        panel.focus_namespace(conn.clone(), target.clone(), cx)
                    });
                })
        }));

    let objects = match &focused {
        None => div()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(format!("Choose a {one} to list what it contains."))
            .into_any_element(),
        Some(namespace) => match explorer.objects(conn.id, namespace) {
            None | Some(Loaded::Loading) => render_progress("Loading", cx),
            Some(Loaded::Failed(error)) => div()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::rose(cx))
                .whitespace_normal()
                .child(SharedString::from(error.clone()))
                .into_any_element(),
            Some(Loaded::Ready(objects)) if objects.is_empty() => div()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(format!("This {one} has no tables, views or collections."))
                .into_any_element(),
            Some(Loaded::Ready(objects)) => {
                render_objects(index, conn, namespace, objects, panel.clone(), cx)
            }
        },
    };

    v_flex()
        .w_full()
        .px_3()
        .py_2p5()
        .gap_2p5()
        .child(rail)
        .child(div().px_1().child(objects))
        .into_any_element()
}

fn render_objects(
    index: usize,
    conn: &DbConnection,
    namespace: &str,
    objects: &[DbObject],
    panel: Entity<DbPanel>,
    cx: &App,
) -> AnyElement {
    let hidden = objects.len().saturating_sub(HOME_OBJECT_LIMIT);
    v_flex()
        .w_full()
        .gap_2()
        .child(
            h_flex()
                .w_full()
                .flex_wrap()
                .children(
                    objects
                        .iter()
                        .take(HOME_OBJECT_LIMIT)
                        .enumerate()
                        .map(|(object_index, object)| {
                            let panel = panel.clone();
                            let conn = conn.clone();
                            let namespace = namespace.to_string();
                            let target = object.clone();
                            let full_name: SharedString =
                                format!("{namespace}.{}", object.name).into();
                            h_flex()
                                .id(("db-home-object", index * 100_000 + object_index))
                                .when(object_index == 0, |cell| {
                                    cell.debug_selector(move || {
                                        format!("db-home-first-object-{index}")
                                    })
                                })
                                .w(px(OBJECT_CELL_W))
                                .h(px(28.))
                                .px_2()
                                .gap_2()
                                .items_center()
                                .rounded(crate::ui::design::r_sm())
                                .cursor_pointer()
                                .hover(|cell| cell.bg(crate::ui::design::hover(cx)))
                                .child(crate::ui::design::indicator::lucide_icon(
                                    object_kind_icon(object.kind),
                                    crate::ui::design::t3(cx),
                                    crate::ui::design::icon_md(),
                                ))
                                .child(
                                    div()
                                        .min_w(px(0.))
                                        .truncate()
                                        .font_family(crate::ui::design::FONT_MONO)
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t1(cx))
                                        .child(SharedString::from(object.name.clone())),
                                )
                                .tooltip(move |window, cx| {
                                    Tooltip::new(full_name.clone()).build(window, cx)
                                })
                                .on_click(move |_, window, cx| {
                                    panel.update(cx, |panel, cx| {
                                        panel.open_object(
                                            conn.clone(),
                                            namespace.clone(),
                                            target.clone(),
                                            window,
                                            cx,
                                        )
                                    });
                                })
                        }),
                ),
        )
        .when(hidden > 0, |list| {
            list.child(
                div()
                    .px_2()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child(format!(
                        "{hidden} more are listed in the Database explorer, where you can filter them by name."
                    )),
            )
        })
        .into_any_element()
}
