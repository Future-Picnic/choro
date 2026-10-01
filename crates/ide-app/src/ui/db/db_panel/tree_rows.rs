//! Row renderers for the database explorer: connections, namespaces, object
//! groups, objects, and the panel's empty state.

use gpui::{
    div, prelude::FluentBuilder, px, Context, FontWeight, InteractiveElement, IntoElement,
    ParentElement, SharedString, StatefulInteractiveElement, Styled,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex, Icon, IconName};
use ide_core::{DbConnection, DbObject, DbObjectKind, DbProvider};

use super::super::chrome::{object_kind_group, object_kind_icon};
use super::super::providers::{effective_read_only, endpoint_summary};
use super::{DbConnectionsEditor, DbPanel, Loaded};
use crate::ui::style;

/// Connection rows carry a second line of state, so they are taller than
/// the namespace and object rows beneath them.
const CONNECTION_ROW_H: f32 = 40.;
const CONNECTION_MARK: f32 = 20.;

/// What a provider calls the level under a connection.
pub(crate) fn namespace_noun(provider: DbProvider) -> (&'static str, &'static str) {
    match provider {
        DbProvider::PostgreSql | DbProvider::Supabase => ("schema", "schemas"),
        _ => ("database", "databases"),
    }
}

/// Explorer wording for a connection's live state, shared with the home.
pub(crate) fn connection_state_label(
    provider: DbProvider,
    state: Option<&Loaded<Vec<String>>>,
) -> SharedString {
    let (one, many) = namespace_noun(provider);
    match state {
        None => "Not connected".into(),
        Some(Loaded::Loading) => "Connecting".into(),
        Some(Loaded::Failed(_)) => "Connection failed".into(),
        Some(Loaded::Ready(namespaces)) => match namespaces.len() {
            1 => format!("1 {one}").into(),
            count => format!("{count} {many}").into(),
        },
    }
}

/// The state dot on a provider mark. Every state is also named in text, so
/// color is never the only signal.
pub(crate) fn connection_state_color(
    state: Option<&Loaded<Vec<String>>>,
    cx: &gpui::App,
) -> Option<gpui::Hsla> {
    match state {
        None => None,
        Some(Loaded::Loading) => Some(crate::ui::design::amber(cx)),
        Some(Loaded::Failed(_)) => Some(crate::ui::design::rose(cx)),
        Some(Loaded::Ready(_)) => Some(crate::ui::design::sage(cx)),
    }
}

/// A provider mark with the connection's state dot seated on its corner.
pub(crate) fn connection_mark(
    provider: ide_core::DbProvider,
    size: f32,
    state: Option<gpui::Hsla>,
    cx: &gpui::App,
) -> gpui::Div {
    let dot = (size * 0.38).max(6.);
    div()
        .relative()
        .flex_none()
        .size(px(size))
        .child(super::super::provider_brand_mark(provider, size))
        .when_some(state, |mark, color| {
            mark.child(
                div()
                    .absolute()
                    .right(px(-dot * 0.3))
                    .bottom(px(-dot * 0.3))
                    .size(px(dot))
                    .rounded_full()
                    .border_2()
                    .border_color(crate::ui::design::nav(cx))
                    .bg(color),
            )
        })
}

impl DbPanel {
    pub(super) fn render_connection_row(
        &self,
        cix: usize,
        conn: &DbConnection,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let expanded = self.expanded_conns.contains(&conn.id);
        let prod = conn.looks_like_prod();
        let read_only = effective_read_only(conn.provider, conn.read_only);
        let toggle_conn = conn.clone();
        let refresh_conn = conn.clone();
        let state = self.databases.get(&conn.id);
        let state_label = connection_state_label(conn.provider, state);
        let failed = matches!(state, Some(Loaded::Failed(_)));
        let tooltip: SharedString = {
            let endpoint = endpoint_summary(conn.provider, &conn.uri)
                .unwrap_or_else(|| "Not configured".into());
            let access = if read_only {
                "Read only"
            } else {
                "Edits allowed"
            };
            format!(
                "{} on {endpoint}\n{access}{}\n{state_label}",
                conn.provider.display_name(),
                if prod { ", production" } else { "" }
            )
            .into()
        };
        h_flex()
            .id(("db-conn-row", cix))
            .w_full()
            .h(px(CONNECTION_ROW_H))
            .pl_1()
            .pr_1()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .hover(|style| style.bg(crate::ui::design::hover(cx)))
            .child(
                Icon::new(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t3(cx)),
            )
            .child(connection_mark(
                conn.provider,
                CONNECTION_MARK,
                connection_state_color(state, cx),
                cx,
            ))
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
                            .child(SharedString::from(conn.name.clone())),
                    )
                    .child(
                        h_flex()
                            .min_w(px(0.))
                            .gap_1p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(
                                div()
                                    .flex_none()
                                    .child(conn.provider.display_name().to_string()),
                            )
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .when(failed, |text| {
                                        text.text_color(crate::ui::design::rose(cx))
                                    })
                                    .child(state_label),
                            ),
                    ),
            )
            .when(prod, |row| {
                row.child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::ShieldAlert,
                    crate::ui::design::rose(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .when(!read_only, |row| {
                row.child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::LockOpen,
                    crate::ui::design::amber(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .child(
                style::icon_button(("db-refresh-conn", cix), IconName::Redo2, cx)
                    .tooltip("Reconnect and reload")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.refresh_connection(refresh_conn.clone(), cx);
                    })),
            )
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_connection(toggle_conn.clone(), cx);
            }))
            .into_any_element()
    }

    pub(super) fn render_database_row(
        &self,
        id: usize,
        conn: &DbConnection,
        db: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let key = (conn.id, db.to_string());
        let expanded = self.expanded_dbs.contains(&key);
        let count = match self.collections.get(&key) {
            Some(Loaded::Ready(objects)) => Some(objects.len()),
            _ => None,
        };
        let toggle_conn = conn.clone();
        let toggle_db = db.to_string();
        let full_name: SharedString = db.to_string().into();
        self.tree_row(
            1,
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
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::Layers,
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_md(),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(crate::ui::design::t1(cx))
                        .child(full_name.clone()),
                )
                .when_some(count, |row, count| {
                    row.child(
                        div()
                            .flex_none()
                            .pr_1()
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(count.to_string()),
                    )
                }),
            cx,
        )
        .id(("db-db-row", id))
        .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.toggle_database(toggle_conn.clone(), toggle_db.clone(), cx);
        }))
        .into_any_element()
    }

    /// A quiet group label ("Tables 12") shown when a namespace holds more
    /// than one kind of object.
    pub(super) fn render_kind_group_row(
        &self,
        kind: DbObjectKind,
        count: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .h(px(22.))
            .pl(px(8. + 2. * 16.))
            .pr_2()
            .gap_1p5()
            .items_end()
            .pb(px(3.))
            .text_size(crate::ui::design::text_label())
            .text_color(crate::ui::design::t3(cx))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(object_kind_group(kind)),
            )
            .child(
                div()
                    .font_family(crate::ui::design::FONT_MONO)
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    pub(super) fn render_object_row(
        &self,
        id: usize,
        conn: &DbConnection,
        db: &str,
        object: &DbObject,
        open: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let open_conn = conn.clone();
        let open_db = db.to_string();
        let open_object = object.clone();
        let full_name: SharedString = format!("{db}.{}", object.name).into();
        self.tree_row(
            2,
            h_flex()
                .w_full()
                .gap_1p5()
                .items_center()
                .child(crate::ui::design::indicator::lucide_icon(
                    object_kind_icon(object.kind),
                    if open {
                        crate::ui::design::accent(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    },
                    crate::ui::design::icon_md(),
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(crate::ui::design::t1(cx))
                        .when(open, |name| name.font_weight(FontWeight::MEDIUM))
                        .child(SharedString::from(object.name.clone())),
                ),
            cx,
        )
        .when(open, |row| row.bg(crate::ui::design::accent_soft(cx)))
        .id(("db-coll-row", id))
        .cursor_pointer()
        .tooltip(move |window, cx| Tooltip::new(full_name.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_object(
                open_conn.clone(),
                open_db.clone(),
                open_object.clone(),
                window,
                cx,
            );
        }))
        .into_any_element()
    }

    pub(super) fn render_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let workspace = self.workspace.clone();
        v_flex()
            .flex_1()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .child(crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::Database,
                crate::ui::design::t3(cx),
                crate::ui::design::icon_xl(),
            ))
            .child(
                div()
                    .text_center()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t2(cx))
                    .whitespace_normal()
                    .child("Connect a database to browse its tables here."),
            )
            .child(
                style::dialog_neutral_button("db-empty-add-connection", "Add connection", cx)
                    .icon(IconName::Plus)
                    .on_click(move |_, window, cx| {
                        DbConnectionsEditor::open(workspace.clone(), window, cx);
                    }),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_states_are_named_in_text() {
        let pg = DbProvider::PostgreSql;
        assert_eq!(connection_state_label(pg, None).as_ref(), "Not connected");
        assert_eq!(
            connection_state_label(pg, Some(&Loaded::Loading)).as_ref(),
            "Connecting"
        );
        assert_eq!(
            connection_state_label(pg, Some(&Loaded::Failed("refused".into()))).as_ref(),
            "Connection failed"
        );
        assert_eq!(
            connection_state_label(pg, Some(&Loaded::Ready(vec!["public".into()]))).as_ref(),
            "1 schema"
        );
        assert_eq!(
            connection_state_label(
                DbProvider::MongoDb,
                Some(&Loaded::Ready(vec!["a".into(), "b".into()]))
            )
            .as_ref(),
            "2 databases"
        );
    }
}
