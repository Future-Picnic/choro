//! The canonical middle-screen header — the agent-header pattern.
//!
//! Every view shown in the center (agents, code, tasks, db, docs, assets,
//! services) uses these so the header reads identically everywhere: a title
//! column on the left (title + optional contextual metadata) and a fixed action
//! slot on the right (status + primary actions). Compose the pieces; render only
//! the metadata and actions a view actually has.
//!
//! ```ignore
//! design::header::bar(cx)
//!     .child(design::header::title_col(cx)
//!         .child(design::header::title("Docs", cx))
//!         .child(design::header::subtitle("3 docs", cx)))
//!     .child(design::header::actions().child(new_doc_button))
//! ```

use gpui::{px, Div, FontWeight, ParentElement, SharedString, Styled};
use gpui_component::{h_flex, v_flex, Icon, IconName};

use super::{scale, token};

fn bar_base(cx: &gpui::App) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .items_start()
        .gap_3()
        .py_2p5()
        .bg(token::base(cx))
}

/// The canonical readable element-header band. Its title and action edges share
/// the centered primary-content frame instead of drifting to viewport edges.
/// Columns remain top-aligned so actions stay aligned with the title when a
/// metadata row is present.
pub fn bar(cx: &gpui::App) -> Div {
    bar_base(cx)
        .max_w(scale::center_content_frame_max_w())
        .mx_auto()
        .px(scale::agent_chat_gutter_x())
}

/// Full-width header for data workspaces and canvases whose content uses the
/// entire center pane. The standard workspace inset aligns it with tables.
pub fn workspace_bar(cx: &gpui::App) -> Div {
    bar_base(cx).px_5()
}

/// Agent-chat header frame. Its inner edges share the exact responsive column
/// used by chat rows and the composer, so title/indicators and actions remain
/// aligned with real conversation content at every window width.
pub fn agent_chat_shell(cx: &gpui::App) -> Div {
    gpui::div()
        .w_full()
        .flex_none()
        .relative()
        .bg(token::base(cx))
}

/// The constrained inner row carried by the full-width agent-chat shell.
pub fn agent_chat_bar(cx: &gpui::App) -> Div {
    bar(cx)
}

/// The growing left column that stacks the title over its subline.
pub fn title_col(_cx: &gpui::App) -> Div {
    v_flex().flex_1().min_w(px(0.)).gap_1()
}

/// A title-row container for editable titles or titles with a small adjacent
/// affordance. It owns the shrinking/truncation boundary for the left column.
pub fn title_row() -> Div {
    h_flex().w_full().min_w(px(0.)).items_center().gap_1()
}

/// The title — one per view. Uses the dedicated title token, semibold primary
/// text, and truncation so the fixed action slot always remains visible.
pub fn title(text: impl Into<SharedString>, cx: &gpui::App) -> Div {
    gpui::div()
        .text_size(scale::text_title())
        .line_height(gpui::relative(1.25))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(token::t1(cx))
        .truncate()
        .child(text.into())
}

/// The quiet subline under the title (subtitle text or an indicator row).
/// `text_ui` muted text; callers can append indicator elements instead.
pub fn subtitle(text: impl Into<SharedString>, cx: &gpui::App) -> Div {
    gpui::div()
        .text_size(scale::text_ui())
        .line_height(gpui::relative(1.))
        .text_color(token::t3(cx))
        .child(text.into())
}

/// The indicator subline row — for context glyphs (task · pr · doc). Sits tight
/// under the title with even spacing; each child is a `design::indicator`.
pub fn subline() -> Div {
    h_flex().items_center().gap_3p5().flex_wrap()
}

/// The right-hand action slot — fixed, never grows, holds the view's controls.
pub fn actions() -> Div {
    h_flex().flex_none().items_center().gap_2()
}

/// A single-row contextual-panel header (Git, Board, Files). The panel title,
/// optional tabs, and one view-flip action share this fixed-height band.
/// Deliberately borderless: the fixed height and title typography carry the
/// separation, so the panel surface (and its gradient) flows uninterrupted
/// under the header.
pub fn panel_bar(_cx: &gpui::App) -> Div {
    h_flex()
        .w_full()
        .h(scale::header_h())
        .flex_none()
        .items_center()
        .px_3p5()
}

/// The leading identity in a contextual-panel header. It is deliberately one
/// type step above tabs (`text_body` versus `text_ui`). Git intentionally omits
/// its icon to preserve useful resize range; other views may supply one.
pub fn panel_identity(
    icon: Option<IconName>,
    label: impl Into<SharedString>,
    cx: &gpui::App,
) -> Div {
    h_flex()
        .flex_none()
        .items_center()
        .gap_1p5()
        .children(icon.map(|icon| {
            Icon::new(icon)
                .size(scale::icon_sm())
                .text_color(token::t3(cx))
        }))
        .child(
            gpui::div()
                .text_size(scale::text_body())
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(token::t1(cx))
                .child(label.into()),
        )
}

/// Quiet contextual information beside a panel identity, such as an item
/// count. It never replaces the surface title, so every panel keeps the same
/// typographic hierarchy.
pub fn panel_meta(text: impl Into<SharedString>, cx: &gpui::App) -> Div {
    gpui::div()
        .flex_none()
        .text_size(scale::text_ui())
        .text_color(token::t3(cx))
        .child(text.into())
}

/// Tight tab cluster for a contextual-panel header. Children own their active
/// underline; this container owns the mock's 2px inter-tab gap.
pub fn panel_tabs() -> Div {
    h_flex()
        .h_full()
        .flex_none()
        .items_center()
        .gap_0p5()
        .pt(scale::panel_tab_offset_y())
}

/// A small mono meta tag shown inline next to the title (e.g. a rel-path or
/// branch). Faint, so it never competes with the title.
pub fn title_meta(text: impl Into<SharedString>, cx: &gpui::App) -> Div {
    gpui::div()
        .text_size(scale::text_ui())
        .font_family(scale::FONT_MONO)
        .text_color(token::t4(cx))
        .child(text.into())
}
