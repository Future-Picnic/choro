//! Legacy design-system bridge — being folded into [`crate::ui::design`].
//!
//! This file predates the token module; its foundational helpers now **delegate
//! to `design::`** so there is a single source of truth (`design/token.rs`,
//! `design/scale.rs`). The remaining builders here are being migrated into
//! `design/` submodules; new code should call `design::` directly.
#![allow(dead_code)]

use gpui::prelude::FluentBuilder;
use gpui::{
    div, px, AnyElement, App, Div, ElementId, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, Stateful, Styled,
};
use gpui_component::{
    button::{Button, ButtonCustomVariant, ButtonVariants},
    h_flex, v_flex, Icon, IconName, Sizable,
};

use crate::ui::design;

// ---- sizing tokens: forwarded to the design scale (one source of truth) ----

/// Main interactive control height. → [`design::CONTROL_H`].
pub const CONTROL_H: f32 = design::CONTROL_H;
/// Compact control height. → [`design::CONTROL_H_SM`].
pub const CONTROL_H_COMPACT: f32 = design::CONTROL_H_SM;
/// Chip / tag height. → [`design::CONTROL_H_XS`].
pub const CHIP_H: f32 = design::CONTROL_H_XS;
/// Standard control radius. → [`design::R_SM`].
pub const RADIUS: f32 = design::R_SM;
/// Small radius. → [`design::R_XS`].
pub const RADIUS_SM: f32 = design::R_XS;
/// Card radius. → [`design::R_LG`].
pub const RADIUS_LG: f32 = design::R_LG;

// ---- borders: forwarded to the design line tokens ----

/// Border for content surfaces (cards, chips, inputs). → [`design::line_2`].
pub fn border(cx: &App) -> Hsla {
    design::line_2(cx)
}

/// The faint structural divider (region edges, chrome). → [`design::line`].
pub fn hairline(cx: &App) -> Hsla {
    design::line(cx)
}

/// The Choro Pulse mark used exclusively for first-party Riffs. Two open arcs
/// form a compact C/sound-wave silhouette that remains legible inside chips.
pub fn choro_riff_icon(size: gpui::Pixels, color: Hsla) -> AnyElement {
    gpui::svg()
        .path("brand/choro-riff.svg")
        .size(size)
        .text_color(color)
        .into_any_element()
}

// ---- surfaces: forwarded to the design ramp ----

/// A content surface (cards, composer, bubbles). → [`design::surface`].
pub fn surface(cx: &App) -> Hsla {
    design::surface(cx)
}

// ---- buttons: wrap gpui-component's theme-aware variants at a fixed size ----

fn base_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    // gpui-component gives `.xsmall()` only `px_1` (4px) horizontal padding, which
    // reads as cramped on a labelled button. Set a comfortable padding here so
    // every button built from this helper is consistent without per-call `.px_*`.
    Button::new(id)
        .label(label)
        .xsmall()
        .h(px(CONTROL_H))
        .px_3()
        .rounded(px(RADIUS))
}

/// The single primary action in a view (e.g. Resume). Solid `primary` fill.
pub fn primary_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button(id, label).custom(primary_variant(cx))
}

/// One step below the primary CTA: an important, accent-flavored action that
/// isn't *the* call to action (e.g. Apply, Stage all). Neutral `secondary` fill
/// gives it the filled "button" feel, while the accent identity comes from a
/// solid `primary` border + text — never a translucent tint, so it holds up on
/// every theme (including light ones like Latte).
/// Lighten (dark themes) or darken (light themes) a surface so it sits above the
/// panel background — gives filled buttons a fill that actually contrasts in
/// every theme, instead of the see-through `secondary`.
fn lift(c: Hsla, amount: f32) -> Hsla {
    Hsla {
        l: (c.l + amount).clamp(0.0, 1.0),
        ..c
    }
}

/// Primary/focus content text (chat, docs, code, titles). → [`design::t1`].
pub fn focus_text(cx: &App) -> Hsla {
    design::t1(cx)
}

/// The theme's `primary` accent, dialed down in saturation so it reads as a
/// calm, muted accent rather than a vivid block — some themes (e.g. Catppuccin
/// Mocha) tune `primary` to a very saturated lavender that's too loud as a
/// button fill. Still derived from the theme token (no hardcoded color), so it
/// tracks every theme. `0.55` keeps the hue but leans the color back ~45%.
fn soft_primary(cx: &App) -> Hsla {
    let p = crate::ui::design::accent(cx);
    Hsla {
        s: (p.s * 0.5).clamp(0.0, 1.0),
        l: (p.l + 0.04).clamp(0.0, 1.0),
        ..p
    }
}

/// The one solid accent (`primary`) fill — the single clear action in a view (a
/// dialog's Save/Add, Ship, Implement). Uses the *full* accent (not a muted
/// variant) so every primary across the app reads as the exact same colour,
/// matching gpui's `.primary()`.
pub fn primary_variant(cx: &App) -> ButtonCustomVariant {
    let fill = crate::ui::design::accent(cx);
    ButtonCustomVariant::new(cx)
        .color(fill)
        .foreground(crate::ui::design::on_accent(cx))
        .border(fill)
        .hover(crate::ui::design::accent_2(cx))
        .active(crate::ui::design::accent_2(cx))
}

/// The refined accent recipe: an *elevated* neutral fill (lifted off the
/// background so it reads as a real button) carrying `primary` text — the accent
/// comes from the text, not a hollow colored border. Raw variant form.
pub fn accent_variant(cx: &App) -> ButtonCustomVariant {
    let bg = crate::ui::design::base(cx);
    let dir = if bg.l < 0.5 { 1.0 } else { -1.0 };
    let fill = lift(bg, 0.09 * dir);
    let hover = lift(bg, 0.14 * dir);
    ButtonCustomVariant::new(cx)
        .color(fill)
        .foreground(soft_primary(cx))
        .border(fill)
        .hover(hover)
        .active(hover)
}

pub fn accent_button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Button {
    base_button(id, label).custom(accent_variant(cx))
}

/// A neutral chip-style dropdown (label / status pickers): `secondary` fill +
/// visible border + foreground text. Any state color comes from a child dot,
/// never the chip itself — same rule as `tag` / `script_chip`.
pub fn chip_dropdown_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(design::control_raised(cx))
        .foreground(design::t1(cx))
        .border(design::control_line(cx))
        .hover(design::control_raised_hover(cx))
        .active(design::control_raised_hover(cx))
}

/// A flat dropdown control for the right-hand slot of an element header. State
/// color belongs to the caller-provided glyph; the control itself stays neutral
/// until hover, matching the design system's indicator law.
pub fn header_dropdown_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let transparent = design::base(cx).opacity(0.0);
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h())
        .px_2()
        .rounded(design::r_sm())
        .dropdown_caret(true)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(transparent)
                .foreground(design::t1(cx))
                .border(transparent)
                .hover(design::hover(cx).opacity(0.5))
                .active(design::hover(cx)),
        )
}

/// A labeled action in the project titlebar, beside the Run scripts control.
/// Active tools keep a quiet surface fill so opening a full-height workspace
/// panel is visible without competing with the primary Run action.
pub fn header_workspace_toggle_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    active: bool,
    cx: &App,
) -> Button {
    let transparent = design::base(cx).opacity(0.0);
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h())
        .px_2()
        .rounded(design::r_sm())
        .icon(icon)
        .label(label)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(if active {
                    design::surface_2(cx)
                } else {
                    transparent
                })
                .foreground(if active {
                    design::t1(cx)
                } else {
                    design::t2(cx)
                })
                .border(transparent)
                .hover(design::hover(cx))
                .active(design::hover(cx)),
        )
}

/// Secondary metadata selector used below a composer or in a compact header.
/// The caller supplies the icon, label, and caret as children so those pieces
/// can keep their individual typography and color tiers.
pub fn header_meta_button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::new(id)
        .small()
        .compact()
        .h(design::control_h())
        .rounded(design::r_sm())
        .custom(header_meta_variant(cx))
}

/// The mock's quiet agent-status indicator. The caller supplies the semantic
/// state glyph and the separately colored caret so label and affordance can use
/// their exact `t2` / `t4` tiers without changing other header dropdowns.
pub fn agent_status_dropdown_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let transparent = design::base(cx).opacity(0.0);
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h())
        .p_0()
        .rounded(design::r_sm())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(transparent)
                .foreground(design::t2(cx))
                .border(transparent)
                .hover(design::hover(cx).opacity(0.5))
                .active(design::hover(cx)),
        )
}

/// A common, non-primary action (e.g. Cancel). Outlined / neutral.
pub fn secondary_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    base_button(id, label).outline()
}

/// Neutral action inside a dialog / modal / popup (Edit, Add connection,
/// Cancel). A dialog sits on the `focus` plane, so its neutral button steps off
/// *that* plane to separate cleanly without a stroke. Apply with
/// `.custom(dialog_neutral_variant(cx))` in place of `.outline()`/`.ghost()`;
/// give the one primary action `.primary()`.
pub fn dialog_neutral_variant(cx: &App) -> ButtonCustomVariant {
    let plane = design::focus(cx);
    ButtonCustomVariant::new(cx)
        .color(design::control_on(plane, cx))
        .foreground(design::t1(cx))
        .border(design::control_line(cx))
        .hover(design::control_on_hover(plane, cx))
        .active(design::control_on_hover(plane, cx))
}

/// A neutral dialog action at the canonical compact control size — **prefer this
/// over `Button::new(..).custom(dialog_neutral_variant(cx))`**.
///
/// [`dialog_neutral_variant`] only carries colour, so a raw `Button` silently
/// keeps gpui's `Size::Medium` (14px text, 32px tall) and lands off the Choro
/// control scale — which is how every dialog's neutral buttons drifted a size
/// too large. Chain `.icon(..)` / `.disabled(..)` / `.on_click(..)` as usual.
pub fn dialog_neutral_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button_compact(id, label).custom(dialog_neutral_variant(cx))
}

/// A large selectable option inside a dialog. The caller supplies the card
/// contents, while this builder owns the canonical geometry and selected-state
/// treatment so feature UIs do not assemble raw buttons or variants.
pub fn dialog_choice_card_button(id: impl Into<ElementId>, selected: bool, cx: &App) -> Button {
    let plane = design::focus(cx);
    let fill = if selected {
        design::accent_soft(cx)
    } else {
        design::control_on(plane, cx)
    };
    let border = if selected {
        design::accent_line(cx)
    } else {
        design::control_line(cx)
    };
    Button::new(id)
        .xsmall()
        .compact()
        .w_full()
        .h(px(100.))
        .p_0()
        .rounded(design::r_md())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(fill)
                .foreground(design::t1(cx))
                .border(border)
                .hover(if selected {
                    design::accent_soft(cx)
                } else {
                    design::control_on_hover(plane, cx)
                })
                .active(design::accent_soft(cx)),
        )
}

/// A low-emphasis action. Text only, hover surface.
pub fn ghost_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    base_button(id, label).ghost()
}

/// Quiet right-side action in a contextual panel title (`Add`, `New doc`,
/// `Edit`). Matches the canonical `.flipbtn` geometry and neutral color tiers.
pub fn context_panel_action_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    _cx: &App,
) -> Button {
    ghost_button_compact(id, label).icon(icon)
}

/// A positive / commit action (e.g. Ship). Solid `success` fill.
pub fn success_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    base_button(id, label).success()
}

/// The theme's `success` green, dialed back a touch so a filled green button
/// reads calm rather than neon — sibling of `soft_primary`, but kept clearly
/// green (lighter desaturation).
fn soft_success(cx: &App) -> Hsla {
    let s = crate::ui::design::sage(cx);
    Hsla {
        s: (s.s * 0.7).clamp(0.0, 1.0),
        ..s
    }
}

/// Filled soft-green recipe for git/ship actions — they're connected to git, so
/// they read green. Soft-green fill + `success_foreground` text.
pub fn success_variant(cx: &App) -> ButtonCustomVariant {
    let fill = soft_success(cx);
    let bg = crate::ui::design::base(cx);
    let dir = if bg.l < 0.5 { 1.0 } else { -1.0 };
    ButtonCustomVariant::new(cx)
        .color(fill)
        .foreground(crate::ui::design::on_sage(cx))
        .border(fill)
        .hover(lift(fill, 0.05 * dir))
        .active(lift(fill, 0.08 * dir))
}

/// A neutral, high-contrast recipe: a true-white fill on dark themes and a
/// near-black fill on light themes, with inverted text. Calm and low-attention
/// compared to a colored fill, which is what git/ship actions want (the meaning
/// comes from the GitHub mark, not a loud green). Uses an explicit white rather
/// than the theme's `foreground`, which is usually an off-white that doesn't
/// read as white.
pub fn inverted_variant(cx: &App) -> ButtonCustomVariant {
    let white = crate::ui::design::palette::ink_on_dark();
    let is_dark = crate::ui::design::base(cx).l < 0.5;
    let (fill, text) = if is_dark {
        (white, crate::ui::design::base(cx))
    } else {
        (crate::ui::design::t1(cx), white)
    };
    // Dim the fill toward the background on hover: darker on dark, lighter on light.
    let hover_amount = if is_dark { -0.1 } else { 0.1 };
    ButtonCustomVariant::new(cx)
        .color(fill)
        .foreground(text)
        .border(fill)
        .hover(lift(fill, hover_amount))
        .active(lift(fill, hover_amount * 1.6))
}

/// The accent *wash* tier — the one action in a view that outranks every
/// neutral control without taking the solid `primary` fill. The accent is
/// blended into the plane and moved into the ink, so the control keeps the
/// app's light-ink-on-dark polarity instead of flipping to a bright slab with
/// dark text. Today this is Ship's alone; it is written as a shared variant so
/// a second action can join the tier without re-deriving the colors.
pub fn ship_action_variant(cx: &App) -> ButtonCustomVariant {
    let plane = design::focus(cx);
    ButtonCustomVariant::new(cx)
        .color(design::control_accent(plane, cx))
        .foreground(design::accent_ink(plane, cx))
        .border(design::control_line(cx))
        .hover(design::control_accent_hover(plane, cx))
        .active(design::control_accent_hover(plane, cx))
}

/// The canonical git/ship button: compact, carrying a GitHub mark, on the
/// accent wash tier — see [`ship_action_variant`]. Shared by the ship modal's
/// commit button and the agent "Ship" action so they look identical.
///
/// Currently unreferenced; kept alongside [`composer_ship_action`] so both ship
/// surfaces stay on one treatment if this is wired back up.
pub fn ship_button(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Button {
    base_button_compact(id, label)
        .custom(ship_action_variant(cx))
        .icon(IconName::GitHub)
}

/// The primary form of the ship action — an accent-filled, GitHub-marked CTA.
/// Used where shipping *is* the view's main action (the Ship modal footer), so it
/// carries full accent presence rather than the quiet inline [`ship_button`].
pub fn ship_button_primary(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button_compact(id, label)
        .custom(primary_variant(cx))
        .icon(IconName::GitHub)
}

/// Agent-header Ship action, matched to the canonical `.ship` component:
/// 27px surface control, line-2 frame, 12.5px medium label, and quiet Git mark.
pub fn agent_header_ship_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::header_ship_h())
        .p_0()
        .rounded(design::r_sm())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(design::control_raised(cx))
                .foreground(design::t1(cx))
                .border(design::control_line(cx))
                .hover(design::control_raised_hover(cx))
                .active(design::control_raised_hover(cx)),
        )
        .child(
            h_flex()
                .h_full()
                .items_center()
                .gap(design::header_ship_gap())
                .px(design::header_ship_pad_x())
                .text_size(design::text_ui())
                .font_weight(FontWeight::MEDIUM)
                .text_color(design::t1(cx))
                .child(
                    Icon::new(IconName::GitHub)
                        .size(design::icon_sm())
                        .text_color(design::t2(cx)),
                )
                .child(div().line_height(gpui::relative(1.)).child(label.into())),
        )
}

/// The signature "hand this to an agent" action. As the one clear primary action
/// in its context (a task card, a doc, a design), it carries the accent fill.
pub fn implement_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button_compact(id, label)
        .custom(primary_variant(cx))
        .icon(IconName::Bot)
}

/// Tinted variant for the *implemented* state of the Implement control, coloured
/// by the linked agent's status (grey to-do / amber running / green done).
pub fn implement_active_variant(color: Hsla, cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(color.opacity(0.14))
        .foreground(color)
        .border(color.opacity(0.45))
        .hover(color.opacity(0.22))
        .active(color.opacity(0.28))
}

/// A destructive action (e.g. Discard / Delete). Solid `danger` fill.
pub fn danger_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    base_button(id, label).danger()
}

// ---- compact buttons (28px, for dense toolbars / inline controls) ----

fn base_button_compact(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    // As in `base_button`: `.xsmall().compact()` collapses horizontal padding to
    // ~4px, so set a comfortable value here — every compact button inherits it.
    Button::new(id)
        .label(label)
        .xsmall()
        .compact()
        .h(px(CONTROL_H_COMPACT))
        .px_2p5()
        .rounded(px(RADIUS_SM))
}

pub fn primary_button_compact(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button_compact(id, label).custom(primary_variant(cx))
}

pub fn accent_button_compact(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button_compact(id, label).custom(accent_variant(cx))
}

/// Read-only busy state for compact header actions. Unlike `.disabled(true)`,
/// this remains legible and intentional instead of fading into unavailable UI.
/// Callers must not attach an action while the underlying operation is busy.
pub fn busy_button_compact(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    base_button_compact(id, label)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(design::amber(cx).opacity(0.12))
                .foreground(design::amber(cx))
                .border(design::amber(cx).opacity(0.34))
                .hover(design::amber(cx).opacity(0.16))
                .active(design::amber(cx).opacity(0.16)),
        )
        .icon(IconName::Loader)
}

pub fn secondary_button_compact(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
) -> Button {
    base_button_compact(id, label).outline()
}

pub fn ghost_button_compact(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    base_button_compact(id, label).ghost()
}

/// Recorder chip used by Settings → Keyboard shortcuts. It keeps keycaps on
/// the shared control scale while making the active recording state obvious.
pub fn shortcut_key_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    recording: bool,
    cx: &App,
) -> Button {
    let variant = if recording {
        ButtonCustomVariant::new(cx)
            .color(design::accent_soft(cx))
            .foreground(design::accent(cx))
            .border(design::accent_line(cx))
            .hover(design::accent_soft(cx))
            .active(design::accent_soft(cx))
    } else {
        ButtonCustomVariant::new(cx)
            .color(design::base(cx))
            .foreground(design::t2(cx))
            .border(design::line_2(cx))
            .hover(design::surface_2(cx))
            .active(design::surface_2(cx))
    };
    base_button_compact(id, label).custom(variant)
}

/// Compact text action used inside a Solo lane band. The emphasized Rejoin
/// treatment keeps the lane's sky identity while geometry stays canonical.
pub fn solo_lane_action_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    emphasized: bool,
    cx: &App,
) -> Button {
    let sky = design::sky(cx);
    let transparent = design::base(cx).opacity(0.0);
    base_button_compact(id, label).custom(
        ButtonCustomVariant::new(cx)
            .color(if emphasized {
                sky.opacity(0.15)
            } else {
                transparent
            })
            .foreground(if emphasized { sky } else { design::t2(cx) })
            .border(transparent)
            .hover(if emphasized {
                sky.opacity(0.24)
            } else {
                design::hover(cx)
            })
            .active(if emphasized {
                sky.opacity(0.3)
            } else {
                design::hover(cx)
            }),
    )
}

pub fn danger_button_compact(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    base_button_compact(id, label).danger()
}

/// Full-height action used in the workspace navigation rail footer. The rail
/// has a deliberately different vertical treatment from ordinary buttons, but
/// still routes through the shared control system so feature modules never
/// assemble their own raw button variants.
pub fn rail_footer_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    let transparent = design::base(cx).opacity(0.0);
    let muted = design::t3(cx);
    let label = label.into();

    Button::new(id)
        .xsmall()
        .compact()
        .w_full()
        .h(px(52.))
        .p_0()
        .rounded(px(0.))
        .custom(
            ButtonCustomVariant::new(cx)
                .color(transparent)
                .foreground(muted)
                .border(transparent)
                .hover(design::hover(cx).opacity(0.28))
                .active(design::hover(cx).opacity(0.42)),
        )
        .child(
            v_flex()
                .w_full()
                .gap_1()
                .items_center()
                .justify_center()
                .child(Icon::new(icon).size(design::icon_lg()).text_color(muted))
                .child(div().text_size(design::text_label()).child(label)),
        )
}

/// Labelled action used by the expanded sidebar footer. This is the horizontal
/// companion to [`rail_footer_button`].
pub fn sidebar_footer_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
) -> Button {
    ghost_button_compact(id, label).icon(icon)
}

// ---- composer controls -------------------------------------------------

/// A "loose" composer control: an optional leading glyph, the value label, and
/// an inline caret — no fill or border box. Controls read as a quiet strip
/// (separated by [`composer_control_divider`]); the surface fill only appears as
/// a faint wash on hover. The whole control opens the caller-provided menu.
pub fn composer_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    leading: Option<AnyElement>,
    cx: &App,
) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h_sm())
        .px_2()
        .rounded(design::r_sm())
        .text_size(design::text_ui())
        .font_weight(FontWeight::NORMAL)
        .custom(
            // `new()` leaves the fill and border transparent — the control is
            // bare, with only a faint hover wash for the affordance. The *value*
            // reads at `t1`; the glyph and caret stay muted, so the hierarchy
            // lives in the type rather than in a box.
            ButtonCustomVariant::new(cx)
                .foreground(design::t1(cx))
                .hover(design::hover(cx))
                .active(design::hover(cx)),
        )
        .child(
            h_flex()
                .h_full()
                .items_center()
                .gap_1p5()
                // Set explicitly: a Button's variant `foreground` only colours
                // `.label()` content, so custom children would otherwise inherit
                // whatever the composer row happens to set.
                .text_color(design::t1(cx))
                .children(leading)
                .child(div().line_height(gpui::relative(1.)).child(label.into()))
                .child(
                    Icon::new(IconName::ChevronDown)
                        .size(design::icon_sm())
                        .text_color(design::t4(cx)),
                ),
        )
}

/// The titlebar's project / branch selector language: a bare muted control that
/// washes to `surface_2` on hover — no fill, border, or divider at rest. Shared
/// so the new-agent composer footer reads identically to the titlebar meta.
pub fn header_meta_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(design::t3(cx))
        .hover(design::surface_2(cx))
        .active(design::surface_2(cx))
}

/// The *project* half of that meta pair: the same bare control, but at the
/// primary text tier — in the titlebar the project name reads and the branch
/// recedes, so the composer footer follows suit.
pub fn header_meta_strong_variant(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .foreground(design::t1(cx))
        .hover(design::surface_2(cx))
        .active(design::surface_2(cx))
}

/// A hairline divider placed between loose [`composer_chip`]s so the control row
/// reads as one quiet strip without boxing each control.
pub fn composer_control_divider(cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(14.))
        .rounded_full()
        // `line` sits almost on the composer surface and reads as nothing here —
        // a dialled-back text tone is quiet but actually visible.
        .bg(design::t3(cx).opacity(0.32))
}

/// A non-menu counterpart to [`composer_chip`] for binary composer modes such as
/// Build / Plan. Same loose (bare) language, no caret; the *selected* state is
/// carried by accent-coloured label and glyph, not a fill.
pub fn composer_toggle_chip(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    leading: Option<AnyElement>,
    selected: bool,
    cx: &App,
) -> Button {
    let foreground = if selected {
        design::accent(cx)
    } else {
        design::t1(cx)
    };
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h_sm())
        .px_2()
        .rounded(design::r_sm())
        .text_size(design::text_ui())
        .font_weight(FontWeight::NORMAL)
        .custom(
            ButtonCustomVariant::new(cx)
                .foreground(foreground)
                .hover(design::hover(cx))
                .active(design::hover(cx)),
        )
        .child(
            h_flex()
                .h_full()
                .items_center()
                .gap_1p5()
                // As in `composer_chip`: colour the custom children directly so a
                // parent row can't cascade over them.
                .text_color(foreground)
                .children(leading)
                .child(div().line_height(gpui::relative(1.)).child(label.into())),
        )
}

/// Attached native Project Preview capability.
pub fn preview_attachment_chip(id: impl Into<ElementId>, cx: &App) -> Button {
    let color = design::accent(cx);

    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h_xs())
        .px_1p5()
        .rounded(design::r_sm())
        .text_size(design::text_ui())
        .font_weight(FontWeight::MEDIUM)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(color.opacity(0.13))
                .foreground(color)
                .border(color.opacity(0.34))
                .hover(color.opacity(0.19))
                .active(color.opacity(0.22)),
        )
        .child(
            h_flex()
                .h_full()
                .items_center()
                .gap_1p5()
                .text_color(color)
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::MonitorPlay,
                    color,
                    design::icon_sm(),
                ))
                .child(div().line_height(gpui::relative(1.)).child("Preview"))
                .child(
                    Icon::new(IconName::Close)
                        .size(design::icon_sm())
                        .text_color(color.opacity(0.72)),
                ),
        )
}

/// Quiet action beside the composer send control. All right-side composer
/// actions share the canonical 30px control height so their edges align.
fn composer_quiet_action_variant(cx: &App) -> ButtonCustomVariant {
    let transparent = design::base(cx).opacity(0.0);
    ButtonCustomVariant::new(cx)
        .color(transparent)
        .foreground(design::t2(cx))
        .border(transparent)
        .hover(design::hover(cx).opacity(0.72))
        .active(design::hover(cx))
}

pub fn composer_ghost_action(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    icon: impl IntoElement,
    cx: &App,
) -> Button {
    let label = label.into();
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h())
        .px_2()
        .rounded(design::r_sm())
        .custom(composer_quiet_action_variant(cx))
        .text_size(design::text_ui())
        .text_color(design::t2(cx))
        .font_weight(FontWeight::NORMAL)
        .child(icon)
        .child(
            div()
                .flex_none()
                .line_height(gpui::relative(1.))
                .child(label),
        )
}

/// Icon-only form of a quiet composer action, used when the composer rail is
/// too narrow to retain action labels without crowding the send control.
pub fn composer_icon_action(id: impl Into<ElementId>, icon: impl IntoElement, cx: &App) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .w(design::control_h())
        .h(design::control_h())
        .p_0()
        .rounded(design::r_sm())
        .custom(composer_quiet_action_variant(cx))
        .text_color(design::t2(cx))
        .child(icon)
}

/// Important repository action in the composer rail. Sits on the accent wash
/// tier — see [`ship_action_variant`] — and collapses to a square GitHub icon
/// without losing that emphasis at narrow widths.
pub fn composer_ship_action(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    compact: bool,
    cx: &App,
) -> Button {
    let button = Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h())
        .rounded(design::r_sm())
        .custom(ship_action_variant(cx))
        .text_size(design::text_ui())
        .font_weight(FontWeight::MEDIUM)
        .child(
            Icon::new(IconName::GitHub)
                .size(design::icon_sm())
                .text_color(design::accent_ink(design::focus(cx), cx)),
        );

    if compact {
        button.w(design::control_h()).p_0()
    } else {
        button.px_2().label(label)
    }
}

/// Accent send square from the mock. Availability remains behavioral—the
/// visual identity is consistently accent, even before the draft is sendable.
pub fn composer_send(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    composer_send_in(id, design::accent(cx), cx)
}

/// The send square in an explicit fill — the one sanctioned deviation from the
/// accent: a Solo's composer sends in sky, so the very act of talking to it
/// carries the lane color.
pub fn composer_send_in(id: impl Into<ElementId>, fill: Hsla, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .size(design::control_h())
        .flex_none()
        .rounded(design::r_sm())
        .flex()
        .items_center()
        .justify_center()
        .bg(fill)
        .text_color(design::on_accent(cx))
        .child(Icon::new(IconName::ArrowUp).size(design::icon_sm()))
}

/// Stop counterpart to [`composer_send`]. It keeps the same geometry and
/// accent plane so a running composer does not jump or grow when its primary
/// action changes state.
pub fn composer_stop(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .size(design::control_h())
        .flex_none()
        .rounded(design::r_sm())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(design::accent(cx))
        .hover(|button| button.bg(design::accent_2(cx)))
        .child(
            div()
                .size(design::composer_stop_glyph())
                .rounded(design::r_xs())
                .bg(design::on_accent(cx)),
        )
}

/// Neutral stop control used by the regular Agent composer. Assistant surfaces
/// use [`composer_stop`] so their compact footer keeps its accent treatment.
pub fn composer_stop_neutral(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .size(design::control_h())
        .flex_none()
        .rounded(design::r_sm())
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(design::t1(cx).opacity(0.86))
        .hover(|button| button.bg(design::t1(cx)))
        .child(
            div()
                .size(design::composer_stop_glyph())
                .rounded(design::r_xs())
                .bg(design::base(cx)),
        )
}

// ---- tags & chips ----

/// A neutral tag for a property / piece of metadata (model, effort …).
/// Never an action — purely informational. Color, if any, comes from a child
/// icon or dot, never from the chip background.
pub fn tag(label: impl Into<SharedString>, cx: &App) -> Div {
    h_flex()
        .h(px(CHIP_H))
        .px_2()
        .items_center()
        .gap_1p5()
        .rounded(px(RADIUS_SM))
        .bg(crate::ui::design::surface(cx))
        .border_1()
        .border_color(border(cx))
        .text_size(design::text_ui())
        .text_color(crate::ui::design::t1(cx))
        .child(label.into())
}

// ---- checkbox ----

/// A 16px checkbox. Visible `muted` border when off, `primary` fill + tick when
/// on. The caller attaches `.on_click(...)`.
pub fn checkbox(id: impl Into<ElementId>, checked: bool, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .size(crate::ui::design::icon())
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_SM))
        .border_1()
        .border_color(if checked {
            crate::ui::design::accent(cx)
        } else {
            crate::ui::design::t3(cx).opacity(0.5)
        })
        .cursor_pointer()
        .when(checked, |el| {
            el.bg(crate::ui::design::accent(cx)).child(
                Icon::new(IconName::Check)
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::on_accent(cx)),
            )
        })
}

// ---- icon button ----

/// A small icon-only action affordance (the `+`, `⋯`, collapse chevrons).
/// Ghost, xsmall, muted icon. The caller adds `.tooltip(...)`, `.on_click(...)`
/// or `.dropdown_menu(...)`.
pub fn icon_button(id: impl Into<ElementId>, icon: IconName, cx: &App) -> Button {
    Button::new(id).ghost().xsmall().icon(
        Icon::new(icon)
            .size(crate::ui::design::icon())
            .text_color(crate::ui::design::t3(cx)),
    )
}

/// A 28px-square icon action for headers/toolbars, matching the agent header's
/// icon buttons (ghost, compact, muted). Use this — not `icon_button` — anywhere
/// icon actions sit beside compact labelled buttons, so heights line up.
pub fn header_icon_button(id: impl Into<ElementId>, icon: IconName, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .compact()
        .h(px(CONTROL_H_COMPACT))
        .w(px(CONTROL_H_COMPACT))
        .icon(
            Icon::new(icon)
                .size(crate::ui::design::icon())
                .text_color(crate::ui::design::t3(cx)),
        )
}

/// Header icon action for app-owned SVG glyphs that are not represented by
/// `IconName`. Geometry and interaction treatment match `header_icon_button`.
pub fn header_svg_button(id: impl Into<ElementId>, icon: impl IntoElement, _cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .compact()
        .h(px(CONTROL_H_COMPACT))
        .w(px(CONTROL_H_COMPACT))
        .child(icon)
}

/// Canonical refresh action. A true circular-refresh glyph distinguishes this
/// from undo/retry, while the surface and border keep it visible in headers.
pub fn refresh_icon_button(id: impl Into<ElementId>, cx: &App) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .w(design::control_h())
        .h(design::control_h())
        .p_0()
        .rounded(design::r_sm())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(design::control_raised(cx))
                .foreground(design::t2(cx))
                .border(design::control_line(cx))
                .hover(design::control_raised_hover(cx))
                .active(design::control_raised_hover(cx)),
        )
        .child(design::indicator::lucide_icon(
            lucide_icons::Icon::RefreshCw,
            design::t2(cx),
            design::icon_sm(),
        ))
}

/// Labelled form of the canonical refresh action for settings and empty/error
/// states where an icon-only control would be ambiguous.
pub fn refresh_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    cx: &App,
) -> Button {
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h())
        .px_2p5()
        .rounded(design::r_sm())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(design::control_raised(cx))
                .foreground(design::t1(cx))
                .border(design::control_line(cx))
                .hover(design::control_raised_hover(cx))
                .active(design::control_raised_hover(cx)),
        )
        .child(design::indicator::lucide_icon(
            lucide_icons::Icon::RefreshCw,
            design::t2(cx),
            design::icon_sm(),
        ))
        .label(label)
}

/// Canonical destructive icon action for headers. The trash glyph is explicit
/// (never confused with a tag or close control), and red is carried by both the
/// resting stroke/icon and the hover surface.
pub fn destructive_icon_button(id: impl Into<ElementId>, cx: &App) -> Button {
    let rose = design::rose(cx);
    Button::new(id)
        .xsmall()
        .compact()
        .w(design::control_h())
        .h(design::control_h())
        .p_0()
        .rounded(design::r_sm())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(rose.opacity(0.08))
                .foreground(rose)
                .border(rose.opacity(0.38))
                .hover(rose.opacity(0.16))
                .active(rose.opacity(0.24)),
        )
        .child(design::indicator::lucide_icon(
            lucide_icons::Icon::Trash2,
            rose,
            design::icon_sm(),
        ))
}

// ---- segmented toggle (mode switcher, Board/Git) ----

/// Container for a segmented toggle. Fill it with `segment()` children. The
/// caller sizes each segment (`.min_w(...)` for fixed, `.flex_1()` to fill).
pub fn segmented_container(cx: &App) -> Div {
    // Header toggle: a *recessed* track (a touch darker than the panel), no
    // border — the active segment rises out of it (see `segment`).
    let bg = crate::ui::design::base(cx);
    let track = Hsla {
        l: (bg.l - 0.035).clamp(0.0, 1.0),
        ..bg
    };
    h_flex()
        .h(px(CONTROL_H))
        .gap_0p5()
        .p(px(1.))
        .items_center()
        .rounded(design::r_md())
        .bg(track)
}

/// A *quiet* segmented toggle for secondary controls (Board/Git,
/// Terminal/Files/Notes). A subtle borderless `surface` track — lighter and
/// calmer than the header's recessed track — so it still reads as a toggle
/// while sitting back. Pair with `segment()`.
pub fn segmented_container_quiet(cx: &App) -> Div {
    // Matches the design system's `.segbox`: a recessed `base` track with a
    // hairline `line` border; the active segment (surface-2) rises out of it.
    h_flex()
        .w_full()
        .gap(px(2.))
        .p(px(2.))
        .items_center()
        .rounded(design::r_sm())
        .border_1()
        .border_color(design::line(cx))
        .bg(design::base(cx))
}

/// Compact text-only tabs for narrow tool sidebars. This mirrors embedded
/// design-tool tab rows while keeping their interaction treatment in Choro's
/// shared design system.
pub fn sidebar_mode_tabs(cx: &App) -> Div {
    let base = design::base(cx);
    let track = Hsla {
        l: (base.l - 0.035).clamp(0.0, 1.0),
        ..base
    };
    h_flex()
        .flex_1()
        .min_w(px(0.))
        .h(px(30.))
        .gap(px(2.))
        .p(px(2.))
        .items_center()
        .rounded(design::r_sm())
        .bg(track)
}

/// Compact icon action inside `sidebar_mode_tabs`. It shares the tab track and
/// height while keeping a narrower fixed column than the text tabs.
pub fn sidebar_mode_icon_tab(id: impl Into<ElementId>, icon: IconName, cx: &App) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .compact()
        .w(px(30.))
        .h(px(26.))
        .rounded(design::r_xs())
        .icon(
            Icon::new(icon)
                // The panel glyph has more internal whitespace than Penpot's
                // masked icon. Size it generously so its visible mark matches
                // Penpot's 16px collapse glyph in the neighboring sidebar.
                .size(px(22.))
                .text_color(design::t3(cx)),
        )
}

/// One equal-width text tab inside `sidebar_mode_tabs`.
pub fn sidebar_mode_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let foreground = if selected {
        design::t1(cx)
    } else {
        design::t3(cx)
    };
    h_flex()
        .id(id)
        .h(px(26.))
        .min_w(px(0.))
        .flex_1()
        .items_center()
        .justify_center()
        .rounded(design::r_xs())
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(foreground)
        .when(selected, |tab| tab.bg(design::surface_2(cx)))
        .when(!selected, |tab| {
            tab.hover(|tab| tab.bg(design::surface(cx)).text_color(design::t1(cx)))
        })
        .child(div().min_w(px(0.)).truncate().child(label.into()))
}

/// Full-width agent detail strip uses this compact, borderless tab language.
/// The bar owns alignment and its top divider; each tab owns only its hover /
/// selected fill, matching the mock's `.dbtn` component.
pub fn agent_detail_tabs() -> Div {
    h_flex()
        .flex_none()
        .items_center()
        .gap(design::agent_detail_tab_group_gap())
}

pub fn agent_detail_tab(
    id: impl Into<ElementId>,
    icon: lucide_icons::Icon,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let foreground = if selected {
        design::t1(cx)
    } else {
        design::t3(cx)
    };
    h_flex()
        .id(id)
        .h(design::control_h_sm())
        .flex_none()
        .items_center()
        .gap(design::agent_detail_tab_gap())
        .px(design::agent_detail_tab_pad_x())
        .rounded(design::r_sm())
        .cursor_pointer()
        .text_size(design::text_ui())
        .font_weight(FontWeight::NORMAL)
        .text_color(foreground)
        .when(selected, |tab| tab.bg(design::surface_2(cx)))
        .when(!selected, |tab| {
            tab.hover(|tab| tab.bg(design::surface(cx)).text_color(design::t1(cx)))
        })
        .child(design::indicator::lucide_icon(
            icon,
            foreground,
            design::icon_sm(),
        ))
        .child(div().line_height(gpui::relative(1.)).child(label.into()))
}

/// Selectable form of an agent-detail footer tab for controls that need to be
/// used as a popover trigger. It intentionally mirrors `agent_detail_tab`.
pub fn agent_detail_tab_button(
    id: impl Into<ElementId>,
    icon: lucide_icons::Icon,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Button {
    let foreground = if selected {
        design::t1(cx)
    } else {
        design::t3(cx)
    };
    let transparent = design::base(cx).opacity(0.0);
    let background = if selected {
        design::surface_2(cx)
    } else {
        transparent
    };
    Button::new(id)
        .xsmall()
        .compact()
        .h(design::control_h_sm())
        .flex_none()
        .gap(design::agent_detail_tab_gap())
        .px(design::agent_detail_tab_pad_x())
        .rounded(design::r_sm())
        .custom(
            ButtonCustomVariant::new(cx)
                .color(background)
                .foreground(foreground)
                .border(transparent)
                .hover(design::surface(cx))
                .active(design::surface_2(cx)),
        )
        .text_size(design::text_ui())
        .font_weight(FontWeight::NORMAL)
        .child(design::indicator::lucide_icon(
            icon,
            foreground,
            design::icon_sm(),
        ))
        // `Button` renders its built-in label before custom children. Keep the
        // footer-tab contents as children so they retain the canonical
        // icon-then-label order used by `agent_detail_tab`.
        .child(
            div()
                .line_height(gpui::relative(1.))
                .text_size(design::text_ui())
                .font_weight(FontWeight::NORMAL)
                .text_color(foreground)
                .child(label.into()),
        )
}

/// One segment of a segmented toggle. Active = filled `secondary` (surface-2),
/// inactive = transparent + muted (the border stays present-but-transparent so
/// switching doesn't shift layout). The caller adds `.on_click(...)` and sizing.
pub fn segment(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let fg = if selected {
        crate::ui::design::t1(cx)
    } else {
        crate::ui::design::t3(cx)
    };
    segment_with_leading(
        id,
        Icon::new(icon)
            .size(crate::ui::design::icon_md())
            .text_color(fg)
            .into_any_element(),
        label,
        selected,
        cx,
    )
}

/// A segmented control item with a caller-provided brand mark. Use this when a
/// source has a canonical asset that should not be approximated by IconName.
pub fn segment_with_leading(
    id: impl Into<ElementId>,
    leading: AnyElement,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let label = label.into();
    let fg = if selected {
        crate::ui::design::t1(cx)
    } else {
        crate::ui::design::t3(cx)
    };
    h_flex()
        .id(id)
        .h(px(CONTROL_H_COMPACT))
        .min_w(px(0.))
        .px_2()
        .gap_1p5()
        .items_center()
        .justify_center()
        .rounded(design::r_xs())
        .cursor_pointer()
        .text_size(design::text_body())
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(fg)
        .when(selected, |seg| seg.bg(design::surface_2(cx)))
        .when(!selected, |seg| {
            seg.hover(|style| style.bg(crate::ui::design::hover(cx).opacity(0.48)))
        })
        .child(leading)
        .child(div().min_w(px(0.)).truncate().child(label))
}

/// A neutral navigation tab (Changes/Commits/PRs, editor tabs) — the design
/// system's `.tab`. Navigation is the *location* channel, so the active tab and
/// its underline read `t1` (neutral), never accent. Inactive = `t3`. The caller
/// adds `.on_click(...)`; wrap a row of these in an `h_flex().border_b_1()`.
pub fn nav_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    cx: &App,
) -> Stateful<Div> {
    let fg = if selected {
        design::t1(cx)
    } else {
        design::t3(cx)
    };
    h_flex()
        .id(id)
        .h_full()
        .px_2()
        .items_center()
        .cursor_pointer()
        .text_size(design::text_ui())
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(fg)
        .border_b_2()
        .border_color(if selected {
            design::t1(cx)
        } else {
            design::base(cx).opacity(0.0)
        })
        .when(!selected, |tab| {
            tab.hover(|style| style.text_color(design::t2(cx)))
        })
        .child(label.into())
}

/// A compact open-file tab for the native editor strip. The selected file uses
/// a quiet raised surface and neutral location underline; modification state is
/// carried by a small amber dot rather than changing the filename itself.
pub fn editor_file_tab(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    selected: bool,
    modified: bool,
    cx: &App,
) -> Stateful<Div> {
    let fg = if selected {
        design::t1(cx)
    } else {
        design::t3(cx)
    };

    h_flex()
        .id(id)
        .h_full()
        .max_w(design::editor_tab_max_w())
        .flex_none()
        .items_center()
        .gap(design::editor_tab_gap())
        .px(design::editor_tab_pad_x())
        .rounded_t(design::r_xs())
        .border_b_2()
        .border_color(if selected {
            design::t1(cx).opacity(0.82)
        } else {
            design::base(cx).opacity(0.0)
        })
        .bg(if selected {
            design::surface(cx).opacity(0.72)
        } else {
            design::base(cx).opacity(0.0)
        })
        .cursor_pointer()
        .text_size(design::text_ui())
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(fg)
        .when(!selected, |tab| {
            tab.hover(|tab| {
                tab.bg(design::hover(cx).opacity(0.48))
                    .text_color(design::t2(cx))
            })
        })
        .when(modified, |tab| {
            tab.child(
                div()
                    .size(px(5.))
                    .flex_none()
                    .rounded_full()
                    .bg(design::amber(cx)),
            )
        })
        .child(div().min_w(px(0.)).truncate().child(label.into()))
}

/// Hairline separator between independent action groups in a compact toolbar.
pub fn toolbar_divider(cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(1.))
        .h(px(16.))
        .mx_1()
        .bg(design::line(cx))
}

// ---- script chip ----

/// A script-run chip: a status dot + the script name in a neutral pill. The
/// `dot` color carries the run state (success = running, danger = failed,
/// muted = idle) — the chip itself stays neutral, like every other tag.
pub fn script_chip(name: impl Into<SharedString>, dot: Hsla, cx: &App) -> Div {
    h_flex()
        .px_2()
        .py(px(3.))
        .gap_1p5()
        .items_center()
        .rounded(px(RADIUS_SM))
        .bg(crate::ui::design::surface(cx))
        .text_size(design::text_ui())
        .text_color(crate::ui::design::t1(cx))
        .child(div().size(px(7.)).flex_none().rounded_full().bg(dot))
        .child(div().max_w(px(120.)).truncate().child(name.into()))
}

/// A Solo branch reduced to a short display slug for tight controls: the
/// `solo/` prefix dropped and the rest hard-cut with an ellipsis.
pub fn solo_slug_short(branch: &str, max_chars: usize) -> String {
    let slug = branch.trim_start_matches("solo/");
    if slug.chars().count() <= max_chars {
        slug.to_string()
    } else {
        let cut: String = slug.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{cut}…")
    }
}

/// A Solo lane's running script in the sidebar: sky-marked and compact —
/// just the Solo's name, always after the real scripts.
pub fn solo_script_chip(slug: impl Into<SharedString>, cx: &App) -> Div {
    let sky = crate::ui::design::sky(cx);
    h_flex()
        .px_2()
        .py(px(3.))
        .gap_1p5()
        .items_center()
        .rounded(px(RADIUS_SM))
        .bg(crate::ui::design::surface(cx))
        .text_size(design::text_ui())
        .text_color(sky)
        .child(div().size(px(7.)).flex_none().rounded_full().bg(sky))
        .child(div().max_w(px(72.)).truncate().child(slug.into()))
}

// ---- status letter ----

/// The single-letter change indicator in the git status list (M / A / D / U).
/// A bare, colored glyph — no chip fill or border — so the change color reads as
/// the indicator itself (Linear-style), consistent on light themes and dark.
pub fn status_letter(letter: impl Into<SharedString>, color: Hsla, _cx: &App) -> Div {
    div()
        .size(crate::ui::design::icon())
        .flex()
        .items_center()
        .justify_center()
        .text_size(design::text_ui())
        .font_weight(FontWeight::BOLD)
        .text_color(color)
        .child(letter.into())
}

// ---- empty state ----

/// A centered "nothing here yet" screen: a badged icon, a title, and a
/// description. The caller appends an optional action button with `.child(...)`
/// (it inherits the same centering and gap). Used for every empty center
/// screen so they all look identical.
pub fn empty_state(
    icon: IconName,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    cx: &App,
) -> Div {
    v_flex()
        .size_full()
        .min_h(px(280.))
        .items_center()
        .justify_center()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(crate::ui::design::line(cx).opacity(0.28))
                .bg(crate::ui::design::surface(cx))
                .p_4()
                .child(
                    Icon::new(icon)
                        .size_8()
                        .text_color(crate::ui::design::t3(cx)),
                ),
        )
        .child(
            v_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(design::text_title())
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(crate::ui::design::t1(cx))
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child(description.into()),
                ),
        )
}

/// Like `empty_state`, but the circle holds the animated app spinner — for
/// genuinely-loading states (fetching a board, indexing …).
pub fn loading_state(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    seed: usize,
    cx: &App,
) -> Div {
    v_flex()
        .size_full()
        .min_h(px(280.))
        .items_center()
        .justify_center()
        .gap_4()
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border_1()
                .border_color(crate::ui::design::line(cx).opacity(0.28))
                .bg(crate::ui::design::surface(cx))
                .size(px(64.))
                .child(crate::ui::logo_spinner::logo_spinner(
                    26.,
                    "loading-state",
                    seed,
                    crate::ui::design::t3(cx),
                )),
        )
        .child(
            v_flex()
                .items_center()
                .gap_1()
                .child(
                    div()
                        .text_size(design::text_title())
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(crate::ui::design::t1(cx))
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child(description.into()),
                ),
        )
}

// ---- attention badge ----

/// The "needs you" badge shown on a project/agent row when agents are waiting.
/// Solid `warning` so it reads on every theme.
pub fn waiting_badge(count: usize, cx: &App) -> Div {
    h_flex()
        .h(px(16.))
        .min_w(px(16.))
        .gap_0p5()
        .items_center()
        .justify_center()
        .when(count > 1, |badge| badge.px_1())
        .rounded_full()
        .bg(crate::ui::design::amber(cx))
        .text_size(crate::ui::design::text_label())
        .font_weight(FontWeight::BOLD)
        .text_color(crate::ui::design::on_amber(cx))
        .child("!")
        .when(count > 1, |badge| badge.child(count.to_string()))
}

// ---- chat surfaces ----
//
// The agent conversation's building blocks. All composed from the same tokens
// (`border`, `secondary` for the surface-2 fill, `card` for the surface) so the
// whole chat holds together across every theme.

/// An elevated content card in the chat — code blocks, the changed-files card,
/// the plan card. Visible neutral border, large radius, clipped corners. The
/// caller adds a `chat_card_head` and body children.
pub fn chat_card(cx: &App) -> Div {
    v_flex()
        .w_full()
        .overflow_hidden()
        .rounded(design::r_lg())
        .border_1()
        .border_color(design::line(cx))
        .bg(design::surface(cx))
}

/// The header strip of a `chat_card`: a `secondary` (surface-2) fill with a
/// bottom divider. The caller fills it with a label / icon / actions.
pub fn chat_card_head(cx: &App) -> Div {
    h_flex()
        .w_full()
        .h(design::chat_card_head_h())
        .flex_none()
        .items_center()
        .gap(design::chat_card_head_gap())
        .px(design::chat_card_head_pad_x())
        .rounded_t(design::r_lg())
        .bg(design::surface_2(cx))
        .border_b_1()
        .border_color(design::line(cx))
        .text_size(design::text_ui())
        .text_color(design::t2(cx))
}

/// Edge-to-edge body row used by file lists and other compact card lists.
pub fn chat_card_row(cx: &App) -> Div {
    h_flex()
        .w_full()
        .items_center()
        .gap(design::chat_card_row_gap())
        .px(design::chat_card_row_pad_x())
        .py(design::chat_card_row_pad_y())
        .bg(design::surface(cx))
        .text_size(design::text_head())
        .text_color(design::t2(cx))
}

/// Quiet neutral action used inside chat-card headers. A chat card body sits on
/// `surface`, so the control steps off that plane.
pub fn chat_card_action_variant(cx: &App) -> ButtonCustomVariant {
    let plane = design::surface(cx);
    ButtonCustomVariant::new(cx)
        .color(design::control_on(plane, cx))
        .foreground(design::t2(cx))
        .border(design::control_line(cx))
        .hover(design::control_on_hover(plane, cx))
        .active(design::control_on_hover(plane, cx))
}

/// A bordered inline row in the chat — a tool call, a source/citation, a
/// multiple-choice option. Neutral border + standard radius. The caller adds
/// `.bg(...)`/selection styling and any `.on_click(...)`.
pub fn chat_row(cx: &App) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_1p5()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(border(cx))
        .text_color(crate::ui::design::t1(cx))
}

/// The human's chat message bubble: a `secondary` (surface-2) fill with a
/// neutral border and the speech-bubble corner. Right-aligned by the caller.
pub fn chat_user_bubble(cx: &App) -> Div {
    // Matches the design's `.uturn .bubble`: surface-2 fill, a neutral `line`
    // border, large radius with the tucked bottom-right speech corner.
    div()
        .max_w(px(520.))
        .px_3()
        .py_2()
        .bg(crate::ui::design::surface_2(cx))
        .border_1()
        .border_color(crate::ui::design::line(cx))
        .rounded(px(RADIUS_LG))
        .rounded_br(px(RADIUS_SM))
        .text_color(crate::ui::design::t1(cx))
}

/// An inline error / failed-turn notice. Solid `danger` border + `danger` text
/// — no translucent tint, so it reads on every theme.
pub fn chat_notice(cx: &App) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(RADIUS))
        .border_1()
        .border_color(crate::ui::design::rose(cx))
        .text_size(design::text_ui())
        .text_color(crate::ui::design::rose(cx))
}

/// The small numbered badge in a multiple-choice option. Selected = solid
/// `primary` fill; otherwise a neutral `secondary` chip.
/// A circular, severity-tinted number badge for code-review findings. Reads as
/// a peer of [`chat_num_badge`] (the 1·2·3 option badges) but carries the
/// finding's severity colour so the list scans by urgency at a glance.
pub fn severity_num_badge(n: impl Into<SharedString>, color: Hsla, _cx: &App) -> Div {
    div()
        .size(px(20.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .text_size(design::text_ui())
        .font_weight(FontWeight::SEMIBOLD)
        .bg(color.opacity(0.16))
        .text_color(color)
        .child(n.into())
}

pub fn chat_num_badge(n: impl Into<SharedString>, selected: bool, cx: &App) -> Div {
    div()
        .size(px(20.))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_full()
        .text_size(design::text_ui())
        .map(|el| {
            if selected {
                el.bg(soft_primary(cx))
                    .text_color(crate::ui::design::on_accent(cx))
            } else {
                el.bg(crate::ui::design::surface(cx))
                    .text_color(crate::ui::design::t3(cx))
            }
        })
        .child(n.into())
}
