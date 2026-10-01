//! Visual parts of the center database workspace: the pinned Connections
//! tab, one tab per open object, and the context bar that names where the
//! open object lives (connection, namespace, object) and how it may be used.
//! The center owns tab state; these builders only draw it.

use gpui::{
    div, prelude::FluentBuilder as _, px, App, Div, ElementId, FontWeight, InteractiveElement as _,
    ParentElement as _, SharedString, Stateful, Styled as _,
};
use gpui_component::h_flex;
use ide_core::{DbObjectKind, DbProvider};

use super::chrome::{object_kind_icon, object_kind_label};

pub const DB_TAB_BAR_H: f32 = 36.;
pub const DB_CONTEXT_BAR_H: f32 = 34.;
const DB_TAB_MAX_W: f32 = 220.;

/// What a database tab shows about its object, captured when it opens.
/// Write posture is not stored here: it is read live from the session so a
/// revoked connection is reflected in tabs that are already open.
#[derive(Clone, Debug, PartialEq)]
pub struct DbTabMeta {
    pub connection_name: SharedString,
    pub provider: DbProvider,
    pub namespace: SharedString,
    pub object: SharedString,
    pub kind: DbObjectKind,
    pub prod: bool,
}

impl DbTabMeta {
    pub fn tooltip(&self) -> SharedString {
        format!(
            "{} in {}\n{}.{}",
            object_kind_label(self.kind),
            self.connection_name,
            self.namespace,
            self.object
        )
        .into()
    }
}

pub fn tab_strip(cx: &App) -> Div {
    h_flex()
        .w_full()
        .h(px(DB_TAB_BAR_H))
        .flex_none()
        .pl_1()
        .pr_1()
        .gap_0p5()
        .items_end()
        .bg(crate::ui::design::nav(cx))
        .border_b_1()
        .border_color(crate::ui::design::line(cx).opacity(0.22))
}

fn tab_frame(id: impl Into<ElementId>, selected: bool, cx: &App) -> Stateful<Div> {
    h_flex()
        .id(id)
        .h(px(DB_TAB_BAR_H - 5.))
        .flex_none()
        .px_2p5()
        .gap_1p5()
        .items_center()
        .rounded_t(crate::ui::design::r_sm())
        .border_b_2()
        .cursor_pointer()
        .text_size(crate::ui::design::text_ui())
        .when(selected, |tab| {
            tab.bg(crate::ui::design::base(cx))
                .border_color(crate::ui::design::accent(cx))
                .text_color(crate::ui::design::t1(cx))
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!selected, |tab| {
            tab.border_color(crate::ui::design::base(cx).opacity(0.))
                .text_color(crate::ui::design::t3(cx))
                .hover(|tab| {
                    tab.bg(crate::ui::design::hover(cx).opacity(0.5))
                        .text_color(crate::ui::design::t2(cx))
                })
        })
}

/// The pinned first tab: the connections home.
pub fn home_tab(selected: bool, cx: &App) -> Stateful<Div> {
    tab_frame("db-tab-home", selected, cx)
        .debug_selector(|| "db-tab-home".into())
        .child(crate::ui::design::indicator::lucide_icon(
            lucide_icons::Icon::Database,
            if selected {
                crate::ui::design::accent(cx)
            } else {
                crate::ui::design::t3(cx)
            },
            crate::ui::design::icon_md(),
        ))
        .child("Connections")
}

/// An object tab: provider mark, object name, and a production glyph when
/// the connection looks like production. Callers append the close control.
pub fn object_tab(ix: usize, meta: &DbTabMeta, selected: bool, cx: &App) -> Stateful<Div> {
    tab_frame(("db-tab", ix), selected, cx)
        .max_w(px(DB_TAB_MAX_W))
        .child(super::provider_brand_mark(meta.provider, 13.))
        .when(meta.prod, |tab| {
            tab.child(crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::ShieldAlert,
                crate::ui::design::rose(cx),
                crate::ui::design::icon_sm(),
            ))
        })
        .child(div().min_w(px(0.)).truncate().child(meta.object.clone()))
}

/// Where the open object lives, as a breadcrumb, with its live access posture.
pub fn context_bar(meta: &DbTabMeta, read_only: bool, cx: &App) -> Div {
    let separator = || {
        div()
            .flex_none()
            .text_color(crate::ui::design::t4(cx))
            .child("/")
    };
    h_flex()
        .debug_selector(|| "db-context-bar".into())
        .w_full()
        .h(px(DB_CONTEXT_BAR_H))
        .flex_none()
        .px_3()
        .gap_3()
        .items_center()
        .bg(crate::ui::design::base(cx))
        .border_b_1()
        .border_color(crate::ui::design::line(cx).opacity(0.22))
        .child(
            h_flex()
                .debug_selector(|| "db-context-path".into())
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .gap_2()
                .items_center()
                .text_size(crate::ui::design::text_ui())
                .child(super::provider_brand_mark(meta.provider, 16.))
                .child(
                    div()
                        .flex_shrink()
                        .min_w(px(40.))
                        .max_w(px(220.))
                        .truncate()
                        .text_color(crate::ui::design::t2(cx))
                        .child(meta.connection_name.clone()),
                )
                .child(separator())
                .child(
                    div()
                        .flex_shrink()
                        .min_w(px(40.))
                        .max_w(px(220.))
                        .truncate()
                        .text_color(crate::ui::design::t2(cx))
                        .child(meta.namespace.clone()),
                )
                .child(separator())
                .child(crate::ui::design::indicator::lucide_icon(
                    object_kind_icon(meta.kind),
                    crate::ui::design::t2(cx),
                    crate::ui::design::icon_md(),
                ))
                .child(
                    div()
                        .flex_shrink()
                        .min_w(px(40.))
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t1(cx))
                        .child(meta.object.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(object_kind_label(meta.kind)),
                ),
        )
        .child(
            div()
                .debug_selector(|| "db-context-access".into())
                .flex_none()
                .child(super::access_indicators(meta.prod, read_only, cx)),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_tooltip_names_kind_connection_and_path_without_credentials() {
        let meta = DbTabMeta {
            connection_name: "Analytics".into(),
            provider: DbProvider::PostgreSql,
            namespace: "public".into(),
            object: "users".into(),
            kind: DbObjectKind::Table,
            prod: false,
        };
        assert_eq!(meta.tooltip().as_ref(), "Table in Analytics\npublic.users");
    }
}
