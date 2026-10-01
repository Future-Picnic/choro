//! Shared chrome for database documents. Every result set — a relational
//! table or a document collection — is framed the same way, after DBFlux's
//! document layout: a query bar that commits a filter, the result body, and a
//! status bar that reports what is on screen. The workspace above them owns
//! identity (connection, namespace, object), so panes never repeat it.

use gpui::{
    div, prelude::FluentBuilder as _, px, AnyElement, App, Div, FontWeight, Hsla,
    InteractiveElement as _, IntoElement as _, ParentElement as _, SharedString, Styled as _,
};
use gpui_component::{button::Button, h_flex, v_flex};
use ide_core::{DatabaseHandle, DbObjectKind};

pub(crate) const QUERY_BAR_H: f32 = 40.;
pub(crate) const STATUS_BAR_H: f32 = 30.;

/// The bar above a result set. Its leading keyword names the operation the
/// filter feeds (`WHERE` for SQL equality filters, `find` for Mongo), so the
/// syntax expected in the field is never a guess.
pub(crate) fn query_bar(cx: &App) -> Div {
    h_flex()
        .w_full()
        .min_h(px(QUERY_BAR_H))
        .flex_none()
        .flex_wrap()
        .px_2()
        .py_1p5()
        .gap_2()
        .items_center()
        .bg(crate::ui::design::nav(cx))
        .border_b_1()
        .border_color(crate::ui::design::line(cx).opacity(0.22))
}

/// The keyword plate at the head of the query bar, set in the data face.
pub(crate) fn query_keyword(word: &'static str, cx: &App) -> Div {
    div()
        .flex_none()
        .h(crate::ui::design::control_h())
        .px_2()
        .flex()
        .items_center()
        .rounded(crate::ui::design::r_xs())
        .bg(crate::ui::design::sky_soft(cx))
        .font_family(crate::ui::design::FONT_MONO)
        .text_size(crate::ui::design::text_label())
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::sky(cx))
        .child(word)
}

/// The bar under a result set: counts on the left, paging on the right.
pub(crate) fn status_bar(cx: &App) -> Div {
    h_flex()
        .w_full()
        .h(px(STATUS_BAR_H))
        .flex_none()
        .pl_3()
        .pr_1()
        .gap_3()
        .items_center()
        .bg(crate::ui::design::nav(cx))
        .border_t_1()
        .border_color(crate::ui::design::line(cx).opacity(0.22))
        .text_size(crate::ui::design::text_label())
        .text_color(crate::ui::design::t3(cx))
}

/// A figure in the status bar: the number in the data face, its unit muted.
pub(crate) fn status_figure(value: impl Into<SharedString>, unit: &'static str, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap_1()
        .items_baseline()
        .child(
            div()
                .font_family(crate::ui::design::FONT_MONO)
                .text_color(crate::ui::design::t1(cx))
                .child(value.into()),
        )
        .child(unit)
}

pub(crate) fn status_text(text: impl Into<SharedString>) -> Div {
    div().flex_none().child(text.into())
}

/// Centered result-state message (loading failed, filter matched nothing).
pub(crate) fn state_message(
    icon: lucide_icons::Icon,
    tone: Hsla,
    title: SharedString,
    detail: Option<SharedString>,
    action: Option<Button>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .debug_selector(|| "db-state-message".into())
        .flex_1()
        .min_h(px(0.))
        .items_center()
        .justify_center()
        .gap_2()
        .px_8()
        .child(crate::ui::design::indicator::lucide_icon(
            icon,
            tone,
            crate::ui::design::icon_xl(),
        ))
        .child(
            div()
                .text_size(crate::ui::design::text_body())
                .font_weight(FontWeight::MEDIUM)
                .text_color(crate::ui::design::t1(cx))
                .child(title),
        )
        .when_some(detail, |message, detail| {
            message.child(
                div()
                    .max_w(px(560.))
                    .text_center()
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.45))
                    .text_color(crate::ui::design::t3(cx))
                    .whitespace_normal()
                    .child(detail),
            )
        })
        .when_some(action, |message, action| {
            message.child(div().pt_1().child(action))
        })
        .into_any_element()
}

/// A one-line notice between the query bar and the results. The tone sits
/// on the glyph; the text stays neutral.
pub(crate) fn inline_notice(
    icon: lucide_icons::Icon,
    tone: Hsla,
    text: SharedString,
    action: Option<Button>,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .px_3()
        .py_1p5()
        .gap_2()
        .items_center()
        .bg(tone.opacity(0.06))
        .border_b_1()
        .border_color(tone.opacity(0.22))
        .child(crate::ui::design::indicator::lucide_icon(
            icon,
            tone,
            crate::ui::design::icon_sm(),
        ))
        .child(
            div()
                .flex_1()
                .min_w(px(0.))
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t2(cx))
                .whitespace_normal()
                .child(text),
        )
        .when_some(action, |notice, action| notice.child(action))
}

pub(crate) fn object_kind_icon(kind: DbObjectKind) -> lucide_icons::Icon {
    match kind {
        DbObjectKind::Collection => lucide_icons::Icon::Braces,
        DbObjectKind::Table => lucide_icons::Icon::Table2,
        DbObjectKind::View => lucide_icons::Icon::ScanEye,
    }
}

pub(crate) fn object_kind_label(kind: DbObjectKind) -> &'static str {
    match kind {
        DbObjectKind::Collection => "Collection",
        DbObjectKind::Table => "Table",
        DbObjectKind::View => "View",
    }
}

/// Plural group label used where the explorer separates object kinds.
pub(crate) fn object_kind_group(kind: DbObjectKind) -> &'static str {
    match kind {
        DbObjectKind::Collection => "Collections",
        DbObjectKind::Table => "Tables",
        DbObjectKind::View => "Views",
    }
}

/// Live write posture of an open document's session. A retired or
/// reconfigured connection flips this to read-only underneath open tabs.
pub(crate) fn handle_read_only(handle: &DatabaseHandle) -> bool {
    match handle {
        DatabaseHandle::Mongo(handle) => handle.is_read_only(),
        DatabaseHandle::Relational(handle) => handle.is_read_only(),
    }
}
