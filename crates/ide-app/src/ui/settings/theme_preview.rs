//! Theme previews for Settings → Appearance.
//!
//! A theme name tells you nothing about how the app will look, so each option
//! is drawn twice over. A miniature of Choro painted in *that theme's own
//! tokens* — sidebar plane with its accent cast, canvas, one content card —
//! answers "what will the app look like"; a row of orbs beneath it (accent ·
//! success · warning · danger · info) answers "what are the colors" at a size
//! you can actually read. Swatches that small inside the miniature fought the
//! mock UI around them, so they moved out to their own row.
//!
//! The colors are read straight from the registered theme JSON, so a theme can
//! be shown without applying it.

use gpui::prelude::FluentBuilder;
use gpui::{
    div, linear_color_stop, linear_gradient, px, relative, App, Div, ElementId, Hsla,
    InteractiveElement, Length, ParentElement, Rgba, SharedString, Stateful, Styled,
};
use gpui_component::{h_flex, v_flex, Colorize as _, Icon, IconName, ThemeRegistry};

use crate::ui::design;

/// Tile width — four across the Settings card at its usual width, and wide
/// enough that the miniature still reads as a window rather than a smudge.
const CARD_W: f32 = 148.0;
/// Preview height, on the app's own 16:9-ish window proportion.
const PREVIEW_H: f32 = 76.0;
/// Sidebar strip inside the miniature.
const RAIL_W: f32 = 22.0;
/// Title-bar strip inside the miniature.
const TITLE_BAR_H: f32 = 9.0;
/// Palette orb diameter — big enough to read a hue at a glance.
const ORB: f32 = 14.0;

/// Blend `a` toward `b` by `t` (0..1) — the preview's own copy of the token
/// module's mixer, used for the sidebar's accent cast.
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let lerp = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: lerp(a.r, b.r),
        g: lerp(a.g, b.g),
        b: lerp(a.b, b.b),
        a: 1.0,
    }
    .into()
}

fn parse(raw: &Option<SharedString>, fallback: Hsla) -> Hsla {
    raw.as_ref()
        .and_then(|hex| Hsla::parse_hex(hex).ok())
        .unwrap_or(fallback)
}

/// The handful of tokens a preview needs from a theme that is not applied.
#[derive(Clone, Copy)]
pub(super) struct ThemePalette {
    base: Hsla,
    nav: Hsla,
    sink: Hsla,
    surface: Hsla,
    line: Hsla,
    t1: Hsla,
    t3: Hsla,
    accent: Hsla,
    sage: Hsla,
    amber: Hsla,
    rose: Hsla,
    sky: Hsla,
}

impl ThemePalette {
    /// Reads a registered theme's palette. Falls back to the live theme for a
    /// name the registry doesn't know (or a theme missing a given color), so a
    /// preview always renders something coherent.
    pub(super) fn load(name: &str, cx: &App) -> Self {
        let fallback = Self {
            base: design::base(cx),
            nav: design::nav(cx),
            sink: design::sink(cx),
            surface: design::surface(cx),
            line: design::line(cx),
            t1: design::t1(cx),
            t3: design::t3(cx),
            accent: design::accent(cx),
            sage: design::sage(cx),
            amber: design::amber(cx),
            rose: design::rose(cx),
            sky: design::sky(cx),
        };
        let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() else {
            return fallback;
        };
        let colors = &config.colors;
        Self {
            base: parse(&colors.background, fallback.base),
            nav: parse(&colors.sidebar, fallback.nav),
            sink: parse(&colors.title_bar, fallback.sink),
            surface: parse(&colors.secondary, fallback.surface),
            line: parse(&colors.border, fallback.line),
            t1: parse(&colors.foreground, fallback.t1),
            t3: parse(&colors.muted_foreground, fallback.t3),
            accent: parse(&colors.primary, fallback.accent),
            sage: parse(&colors.success, fallback.sage),
            amber: parse(&colors.warning, fallback.amber),
            rose: parse(&colors.danger, fallback.rose),
            sky: parse(&colors.info, fallback.sky),
        }
    }

    /// The miniature window: sidebar, title bar, canvas, one content card.
    fn miniature(&self) -> Div {
        h_flex()
            .w_full()
            .h(px(PREVIEW_H))
            .flex_none()
            .overflow_hidden()
            .rounded(design::r_sm())
            .border_1()
            .border_color(self.line)
            .bg(self.base)
            .child(self.rail())
            .child(self.canvas())
    }

    /// Sidebar plane, carrying the same accent cast the real sidebar does.
    fn rail(&self) -> Div {
        v_flex()
            .flex_none()
            .w(px(RAIL_W))
            .h_full()
            .py(px(6.))
            .px(px(5.))
            .gap(px(4.))
            .bg(linear_gradient(
                160.0,
                linear_color_stop(mix(self.nav, self.accent, 0.14), 0.0),
                linear_color_stop(self.nav, 0.7),
            ))
            .child(bar(px(12.), self.accent, 1.0))
            .child(bar(px(9.), self.t3, 0.5))
            .child(bar(px(11.), self.t3, 0.32))
    }

    /// Canvas: title bar plus one content card on the surface plane.
    fn canvas(&self) -> Div {
        v_flex()
            .flex_1()
            .h_full()
            .min_w(px(0.))
            // A whisper of accent in the upper-left corner, the way the real
            // canvas picks up light from the sidebar.
            .bg(linear_gradient(
                135.0,
                linear_color_stop(mix(self.base, self.accent, 0.09), 0.0),
                linear_color_stop(self.base, 0.65),
            ))
            .child(
                div()
                    .w_full()
                    .h(px(TITLE_BAR_H))
                    .flex_none()
                    .bg(self.sink.opacity(0.9)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .w_full()
                    .min_h(px(0.))
                    .p(px(6.))
                    .gap(px(6.))
                    .child(
                        v_flex()
                            .w_full()
                            .flex_none()
                            .p(px(4.))
                            .gap(px(3.))
                            .rounded(px(design::R_XS))
                            .border_1()
                            .border_color(self.line)
                            .bg(self.surface)
                            .child(bar(relative(0.72), self.t1, 0.8))
                            .child(bar(relative(0.44), self.t3, 0.6)),
                    ),
            )
    }

    /// The theme's colors as such: accent first, then the status family.
    fn palette(&self) -> Div {
        h_flex()
            .w_full()
            .items_center()
            .gap(px(6.))
            .px(px(2.))
            .child(orb(self.accent))
            .child(orb(self.sage))
            .child(orb(self.amber))
            .child(orb(self.rose))
            .child(orb(self.sky))
    }
}

/// A stand-in text line inside the miniature.
fn bar(width: impl Into<Length>, color: Hsla, alpha: f32) -> Div {
    div()
        .flex_none()
        .h(px(3.))
        .w(width.into())
        .rounded(design::r_pill())
        .bg(color.opacity(alpha))
}

/// One palette orb. GPUI has no radial gradient, so the sheen is a diagonal
/// two-stop blend — lit at the top-left corner, settling into the color's own
/// deeper edge — which is enough to read as a sphere at this size.
fn orb(color: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(ORB))
        .rounded(design::r_pill())
        .bg(linear_gradient(
            145.0,
            linear_color_stop(mix(color, gpui::white(), 0.38), 0.0),
            linear_color_stop(mix(color, gpui::black(), 0.14), 1.0),
        ))
}

/// A selectable theme tile: the miniature, the palette row, the theme name,
/// and the tick.
pub(super) fn theme_card(
    id: impl Into<ElementId>,
    name: SharedString,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let palette = ThemePalette::load(name.as_ref(), cx);
    v_flex()
        .id(id)
        .w(px(CARD_W))
        .flex_none()
        .p(px(6.))
        .gap(px(8.))
        .rounded(design::r_md())
        .border_1()
        .border_color(if selected {
            design::accent(cx)
        } else {
            design::line_2(cx)
        })
        .bg(if selected {
            design::accent_soft(cx)
        } else {
            design::surface(cx).opacity(0.55)
        })
        .cursor_pointer()
        .when(!selected, |card| {
            card.hover(|card| card.bg(design::surface_2(cx)))
        })
        .child(palette.miniature())
        .child(palette.palette())
        .child(
            h_flex()
                .w_full()
                .items_center()
                .gap(px(4.))
                .px(px(2.))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .truncate()
                        .text_size(design::text_ui())
                        .text_color(if selected {
                            design::t1(cx)
                        } else {
                            design::t2(cx)
                        })
                        .child(name),
                )
                .when(selected, |row| {
                    row.child(
                        Icon::new(IconName::Check)
                            .size(design::icon_sm())
                            .text_color(design::accent(cx)),
                    )
                }),
        )
}
