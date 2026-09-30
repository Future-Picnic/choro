use gpui::{div, px, svg, App, IntoElement, ParentElement, Styled};
use gpui_component::{h_flex, Icon, IconName};
use ide_core::AgentStatus;

use crate::ui::design;

/// The colour that carries an agent's workflow state — from the design system's
/// semantic tokens, so it tracks every theme. Backlog reads muted, To do = sky,
/// In progress = amber, Done = sage, Rejected = rose.
pub fn status_accent(status: AgentStatus, cx: &App) -> gpui::Hsla {
    match status {
        AgentStatus::Backlog => design::t3(cx),
        AgentStatus::Todo => design::sky(cx),
        AgentStatus::InProgress => design::amber(cx),
        AgentStatus::Done => design::sage(cx),
        AgentStatus::Rejected => design::rose(cx),
    }
}

/// Colour for the Implement control's *implemented* state (a subset of the
/// status accents — the colour carries the state, so no status word).
pub fn implement_status_color(status: AgentStatus, cx: &App) -> gpui::Hsla {
    status_accent(status, cx)
}

pub fn status_dot(status: AgentStatus, size: f32, cx: &App) -> gpui::AnyElement {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(status_accent(status, cx))
        .into_any_element()
}

/// Shape-coded status glyph for the kanban board. Deliberately distinct from
/// `status_dot` (a filled dot that means run/attention state elsewhere) so the
/// workflow stage reads as its own vocabulary: dashed circle = backlog, open
/// circle = to do, spinner = in progress, check = done, cross = rejected.
pub fn status_icon(status: AgentStatus, color: gpui::Hsla) -> gpui::AnyElement {
    let builtin = match status {
        AgentStatus::InProgress => Some(IconName::LoaderCircle),
        AgentStatus::Done => Some(IconName::CircleCheck),
        AgentStatus::Rejected => Some(IconName::CircleX),
        _ => None,
    };
    if let Some(icon) = builtin {
        return Icon::new(icon)
            .size(crate::ui::design::icon_md())
            .text_color(color)
            .into_any_element();
    }
    let path = match status {
        AgentStatus::Backlog => "icons/circle-dashed.svg",
        _ => "icons/circle.svg",
    };
    svg()
        .size(crate::ui::design::icon_md())
        .flex_shrink_0()
        .path(path)
        .text_color(color)
        .into_any_element()
}

pub fn status_menu_row(status: AgentStatus, cx: &mut App) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .min_w(px(148.))
        .items_center()
        .gap_2()
        .child(status_icon(status, status_accent(status, cx)))
        .child(
            div()
                .flex_1()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t1(cx))
                .child(status.label()),
        )
        .into_any_element()
}
