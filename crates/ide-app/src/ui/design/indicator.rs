//! Indicators — the "colored glyph + plain text" law.
//!
//! State color lives on the glyph; the label stays neutral text. Never a filled
//! pill, never a colored border, never a colored background for status.

use gpui::{
    prelude::FluentBuilder, px, relative, Div, ElementId, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, Styled,
};
use gpui_component::{h_flex, Icon, IconName};

use super::{scale, token};

/// A standard glyph from the bundled Lucide font. This keeps uncommon product
/// icons in the same tokenized size/color pipeline as the component icon set.
pub fn lucide_icon(icon: lucide_icons::Icon, color: Hsla, size: gpui::Pixels) -> Div {
    gpui::div()
        .flex_none()
        .font_family(scale::FONT_LUCIDE)
        .text_size(size)
        .line_height(relative(1.))
        .text_color(color)
        .child(icon.unicode().to_string())
}

/// A status/context indicator: a color-carrying icon followed by plain text.
/// `color` tints only the icon; the label reads at `t2`.
pub fn indicator(
    icon: IconName,
    label: impl Into<SharedString>,
    color: Hsla,
    cx: &gpui::App,
) -> Div {
    h_flex()
        .items_center()
        .gap_1p5()
        .text_size(scale::text_ui())
        .text_color(token::t2(cx))
        .child(Icon::new(icon).size(px(scale::ICON_IND)).text_color(color))
        .child(label.into())
}

/// The quieter subline variant (header indicator row): 12px, muted label.
pub fn subline_indicator(
    icon: IconName,
    label: impl Into<SharedString>,
    color: Hsla,
    cx: &gpui::App,
) -> Div {
    subline_indicator_with_icon(
        Icon::new(icon).size(px(scale::ICON_IND)).text_color(color),
        label,
        cx,
    )
}

/// The Solo mark — shuffle: paths crossing and going their own way. A Lucide
/// font glyph, so it renders everywhere without asset plumbing. The single
/// source for every Solo surface (band, badges, scope flips, sidebar chips),
/// so the glyph can never drift between them.
pub fn solo_icon(color: Hsla, size: gpui::Pixels) -> Div {
    lucide_icon(lucide_icons::Icon::Shuffle, color, size)
}

/// The lead's role mark. Kept in contextual details so sidebar task names
/// stay quiet; activity and child disclosure retain their own indicators.
pub fn lead_icon(color: Hsla, size: gpui::Pixels) -> Div {
    lucide_icon(lucide_icons::Icon::Blend, color, size)
}

/// A bandmate's identity, separate from its activity glyph. Callers pass the
/// durable assignment position, never the row's position in a filtered list.
/// Larger bands reuse the six instruments without changing existing members.
pub fn bandmate_icon(index: usize, color: Hsla, size: gpui::Pixels) -> Div {
    use lucide_icons::Icon;
    const INSTRUMENTS: [Icon; 6] = [
        Icon::Drum,
        Icon::Guitar,
        Icon::Piano,
        Icon::Mic,
        Icon::Speaker,
        Icon::DiscAlbum,
    ];
    lucide_icon(INSTRUMENTS[index % INSTRUMENTS.len()], color, size)
}

/// The PocketComet origin mark. Orbit is a concrete link between a task and
/// its running Choro agent, and remains distinct from Choro's Solo fork mark.
pub fn pocketcomet_icon(color: Hsla, size: gpui::Pixels) -> Div {
    lucide_icon(lucide_icons::Icon::Orbit, color, size)
}

/// Tumble's app-icon silhouette without its tile; the label stays rail-colored.
pub fn tumble_rail_icon() -> Div {
    gpui::div().flex_none().size(scale::icon_lg()).child(
        gpui::svg()
            .path("brand/tumble.svg")
            .size(scale::icon_lg())
            .text_color(super::palette::tumble_brand()),
    )
}

/// A header-subline indicator with a supplied icon element. This keeps custom
/// product glyphs (for example the existing pull-request icon) on the same
/// type, spacing, and text tokens as standard library icons.
pub fn subline_indicator_with_icon(
    icon: impl IntoElement,
    label: impl Into<SharedString>,
    cx: &gpui::App,
) -> Div {
    h_flex()
        .min_w(px(0.))
        .items_center()
        .gap_1()
        .text_size(scale::text_ui())
        .text_color(token::t3(cx))
        .child(icon)
        .child(gpui::div().min_w(px(0.)).truncate().child(label.into()))
}

/// A state indicator for an element-header action: a small colored dot and a
/// neutral label. The state color never leaks into the button surface.
pub fn status(label: impl Into<SharedString>, color: Hsla, cx: &gpui::App) -> Div {
    h_flex()
        .items_center()
        .gap_1p5()
        .text_size(scale::text_ui())
        .text_color(token::t2(cx))
        .child(dot(color))
        .child(label.into())
}

/// The interactive form of a standard subline indicator. The caller attaches
/// the click behavior; the shared builder owns its neutral hover treatment.
pub fn subline_link(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    color: Hsla,
    cx: &gpui::App,
) -> Stateful<Div> {
    subline_indicator(icon, label, color, cx)
        .id(id)
        .cursor_pointer()
        .hover(|item| item.text_color(token::t2(cx)))
}

/// The interactive form of a subline indicator that uses a supplied icon.
pub fn subline_link_with_icon(
    id: impl Into<ElementId>,
    icon: impl IntoElement,
    label: impl Into<SharedString>,
    cx: &gpui::App,
) -> Stateful<Div> {
    subline_indicator_with_icon(icon, label, cx)
        .id(id)
        .cursor_pointer()
        .hover(|item| item.text_color(token::t2(cx)))
}

/// The single-letter git change indicator (M / A / D / U). A bare colored mono
/// glyph — no chip, no fill — so the change color *is* the indicator.
pub fn letter(letter: impl Into<SharedString>, color: Hsla, _cx: &gpui::App) -> Div {
    gpui::div()
        .w(px(12.))
        .flex_none()
        .text_center()
        .font_family(scale::FONT_MONO)
        .text_size(scale::text_ui())
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(color)
        .child(letter.into())
}

/// A status-aware selection control for dense file lists. The border and tick
/// use the solid semantic color; selection adds its paired soft semantic fill.
/// This is intentionally distinct from the neutral form checkbox.
pub fn status_checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    color: Hsla,
    soft_fill: Hsla,
    idle_border: Hsla,
) -> Stateful<Div> {
    // At rest the box stays neutral — the section header already names the
    // change kind, so the semantic color appears only on hover and when the
    // entry is actually staged.
    gpui::div()
        .id(id)
        .size(scale::icon())
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(scale::r_xs())
        .border_1()
        .border_color(if checked { color } else { idle_border })
        .cursor_pointer()
        .hover(move |control| control.bg(soft_fill).border_color(color))
        .when(checked, move |control| {
            control.bg(soft_fill).child(
                Icon::new(IconName::Check)
                    .size(scale::icon_sm())
                    .text_color(color),
            )
        })
}

/// A small live/status dot (agent live, needs-you). Pass the state color.
pub fn dot(color: Hsla) -> Div {
    gpui::div()
        .size(px(6.))
        .flex_none()
        .rounded_full()
        .bg(color)
}

/// A tiny live dot for the accent channel (agent working).
pub fn live_dot(cx: &gpui::App) -> Div {
    dot(token::accent(cx))
}

/// The `+add −del` diff-count pair (mono). Sage additions, rose deletions.
pub fn diff_counts(added: i64, deleted: i64, cx: &gpui::App) -> Div {
    h_flex()
        .items_center()
        .gap_1p5()
        .font_family(scale::FONT_MONO)
        .text_size(scale::text_ui())
        .child(
            gpui::div()
                .text_color(token::sage(cx))
                .child(SharedString::from(format!("+{added}"))),
        )
        .child(
            gpui::div()
                .text_color(token::rose(cx))
                .child(SharedString::from(format!("−{deleted}"))),
        )
}
