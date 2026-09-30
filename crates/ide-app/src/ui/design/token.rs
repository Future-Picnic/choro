//! Color tokens — the single source of truth for every color in the app.
//!
//! The Choro themes (generated from design tokens into `assets/themes/choro.json`)
//! populate the gpui-component `Theme`; these accessors expose our design
//! vocabulary on top of it. **No component reads `cx.theme().<field>` directly** —
//! it calls one of these, so the whole app speaks one token language and every
//! theme flows through unchanged.
//!
//! Vocabulary (see `choro-design-system.html`):
//! - surface ramp: [`sink`] · [`nav`] · [`base`] · [`surface`] · [`surface_2`] · [`focus`]
//! - text tiers:   [`t1`] · [`t2`] · [`t3`] · [`t4`]
//! - lines:        [`line`] · [`line_2`]
//! - accent:       [`accent`] · [`accent_2`] · [`on_accent`] · [`accent_soft`] · [`accent_line`]
//! - semantics:    [`amber`] · [`sage`] · [`rose`] · [`sky`] (+ `*_soft` for controls)

use gpui::{App, Hsla, Rgba};
use gpui_component::ActiveTheme;

// ---- helpers -------------------------------------------------------------

/// Blend `a` toward `b` by `t` (0..1) in linear-ish RGB — used to derive the
/// tiers the theme schema doesn't carry directly, so they still track the theme.
fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let l = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: 1.0,
    }
    .into()
}

// ---- surface ramp (recede → forward) -------------------------------------

/// Deepest recede plane — the activity rail.
pub fn sink(cx: &App) -> Hsla {
    cx.theme().title_bar
}
/// Recede plane — sidebars and the docked drawer.
pub fn nav(cx: &App) -> Hsla {
    cx.theme().sidebar
}
/// Theme-colored cast at the sidebar's upper-left edge. This is deliberately
/// mixed into the plane rather than painted as a translucent overlay: every
/// theme owns the resulting color and foreground contrast stays predictable.
///
/// The blend target is not the raw accent (which is pale by design) but its
/// deep chromatic sibling — same hue, dark body — so the cast is a true
/// color, dosed lightly enough to stay a whisper away from the plain
/// sidebar plane it fades into.
pub fn nav_glow(cx: &App) -> Hsla {
    nav_glow_color(nav(cx), accent(cx), cx.theme().is_dark())
}

/// The accent's deep sibling: keep the hue, pin saturation and lightness so
/// every theme lands on a comparably weighted cast.
fn accent_depth(accent: Hsla, l: f32) -> Hsla {
    Hsla {
        h: accent.h,
        s: 0.42,
        l,
        a: 1.0,
    }
}

fn nav_glow_color(plane: Hsla, accent: Hsla, is_dark: bool) -> Hsla {
    if is_dark {
        // The sibling sits a fixed step above the plane's own lightness, so
        // lighter dark themes (Dusk) get as perceptible a cast as deep ones.
        mix(plane, accent_depth(accent, plane.l + 0.18), 0.15)
    } else {
        // Light planes sit near-white, so the sibling is lighter and the
        // blend shallower to keep dark foreground text comfortably readable.
        mix(plane, accent_depth(accent, 0.45), 0.08)
    }
}

/// The sidebar surface for the user's chosen [`SidebarStyle`]: a flat plane or
/// the theme-accent cast falling from the top and resolving into [`nav`] by
/// mid-height (strictly vertical: GPUI distorts angled gradients on tall
/// panels). The cast is chromatic on purpose — a colorless lift spans only a
/// handful of 8-bit levels and this GPUI version does not dither, so it
/// rendered as visible bands.
pub fn sidebar_background(style: ide_core::config::SidebarStyle, cx: &App) -> gpui::Background {
    use ide_core::config::SidebarStyle;
    match style {
        SidebarStyle::Flat => gpui::solid_background(nav(cx)),
        SidebarStyle::Colorful => gpui::linear_gradient(
            180.0,
            gpui::linear_color_stop(nav_glow(cx), 0.0),
            gpui::linear_color_stop(nav(cx), 0.55),
        )
        .color_space(gpui::ColorSpace::Oklab),
    }
}
/// The canvas everything sits on.
pub fn base(cx: &App) -> Hsla {
    cx.theme().background
}
/// A content object — cards, chips, secondary buttons.
pub fn surface(cx: &App) -> Hsla {
    cx.theme().secondary
}
/// Raised detail on a surface — hover/active fills, card headers, user bubbles.
pub fn surface_2(cx: &App) -> Hsla {
    cx.theme().accent
}
/// The "now" plane — modals, plan card, composer, the live AI turn. Elevation
/// is carried by border + shadow, never a bright lift (the "dim" rule).
pub fn focus(cx: &App) -> Hsla {
    cx.theme().popover
}

// ---- control fills (fill-first buttons) -----------------------------------
//
// Buttons carry no resting stroke: a control reads as a button because its fill
// sits a step off the plane it lives on. A control on the app canvas lifts off
// the background via [`control_raised`]. A control *inside* a lifted box — a
// card, the composer, the commit box, a dialog — steps off that box with
// [`control_on`], which takes the plane so the step is measured against the
// surface actually beneath it.
//
// Buttons lift; only wells sink. An inset reads as something you put things
// *into* (a field, a track), so it is the wrong metaphor for a control — and
// because the dialog plane is the app's lightest, a control that sank from it
// landed below the sidebar and punched a hole in the box.
//
// The step is expressed as a blend toward [`t1`] rather than a raw lightness
// offset. The theme's own text color carries the polarity — light in dark
// themes, dark in light ones — so one recipe separates correctly on both, and
// near-white planes can't reveal a hidden hue the way a lightness step does.
//
// The solid accent fill stays reserved for the one primary action (send, Save);
// [`control_accent`] is the quieter accent tier for the single most important
// action in a view (today: Ship).

/// Resting border on a fill-first control: none. Kept as a named token so a
/// theme — or a future "outlined" preference — can bring a stroke back in one
/// place instead of touching every recipe.
pub fn control_line(cx: &App) -> Hsla {
    base(cx).opacity(0.0)
}

/// Fill for a control sitting on the app canvas or a panel — a step up off the
/// background. Aliases [`surface`], the ramp's "secondary button" plane.
pub fn control_raised(cx: &App) -> Hsla {
    surface(cx)
}

/// Hover fill for a [`control_raised`] control — one plane brighter.
pub fn control_raised_hover(cx: &App) -> Hsla {
    surface_2(cx)
}

/// Fill for a control sitting *inside* a lifted box — pass the plane the
/// control lands on (`focus(cx)` for a dialog, the composer, or the commit box;
/// `surface(cx)` for a card). Steps off that plane toward the viewer so the
/// control separates without a stroke.
pub fn control_on(plane: Hsla, cx: &App) -> Hsla {
    control_on_fill(plane, t1(cx))
}

/// Hover fill for a [`control_on`] control — one step further off its plane.
pub fn control_on_hover(plane: Hsla, cx: &App) -> Hsla {
    mix(plane, t1(cx), 0.18)
}

/// Pure form of [`control_on`]: the plane and the theme's text color in, the
/// control fill out. Split from the `cx` accessor so the recipe can be checked
/// against every shipped theme without an `App` — see this module's tests.
fn control_on_fill(plane: Hsla, ink: Hsla) -> Hsla {
    mix(plane, ink, 0.10)
}

/// How far the accent tier sits from the neutral control beside it, in HSL
/// lightness. Deliberately small — Ship should outrank a neutral without
/// shouting; the accent hue in the ink carries the rest of the signal.
const ACCENT_TIER_STEP: f32 = 0.075;

/// The contrast the accent ink must reach on its own fill.
const ACCENT_INK_MIN_CONTRAST: f32 = 4.6;

/// The quiet accent tier: the accent blended *into* `plane` rather than laid on
/// top of it, so the control keeps the theme's own ink polarity instead of
/// flipping to dark-on-light. Paired with [`accent_ink`]. Reserved for the
/// single most important action in a view (today: Ship) — the solid
/// [`accent`] fill stays with `primary`.
///
/// The blend is *solved*, not a fixed ratio: the eight shipped accents range
/// from a pale lavender to a mint green, so any one ratio is invisible on some
/// themes and garish on others. Instead the fill is blended until it sits
/// [`ACCENT_TIER_STEP`] off the neutral control beside it — the tier keeps a
/// consistent distance from its neighbour on every theme, and a ninth theme
/// gets the same treatment without being hand-tuned.
pub fn control_accent(plane: Hsla, cx: &App) -> Hsla {
    control_accent_fill(
        plane,
        accent(cx),
        on_accent(cx),
        t1(cx),
        ACCENT_TIER_STEP,
        cx.theme().is_dark(),
    )
}

/// Hover fill for a [`control_accent`] control — one more step off the neutral.
pub fn control_accent_hover(plane: Hsla, cx: &App) -> Hsla {
    control_accent_fill(
        plane,
        accent(cx),
        on_accent(cx),
        t1(cx),
        ACCENT_TIER_STEP * 2.0,
        cx.theme().is_dark(),
    )
}

/// Pure form of [`control_accent`] — see [`control_on_fill`] for why it is split.
fn control_accent_fill(
    plane: Hsla,
    accent: Hsla,
    on_accent: Hsla,
    ink_ref: Hsla,
    step: f32,
    is_dark: bool,
) -> Hsla {
    let neutral = control_on_fill(plane, ink_ref);
    let target = if is_dark {
        neutral.l + step
    } else {
        neutral.l - step
    };
    // A light theme's plane is near-white and its accent is a *lighter* pastel,
    // so blending toward the raw accent can never reach a darker target — aim at
    // a deepened accent instead, which keeps the hue in the brand's family.
    let toward = if is_dark {
        accent
    } else {
        mix(accent, on_accent, 0.45)
    };
    // `mix` is monotonic in `t` here (`toward` is consistently on one side of
    // `plane`), so bisection converges well below one 8-bit level.
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = 0.5 * (lo + hi);
        let l = mix(plane, toward, mid).l;
        let reached = if is_dark { l >= target } else { l <= target };
        if reached {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    mix(plane, toward, hi)
}

/// WCAG 2.x relative luminance.
fn luminance(color: Hsla) -> f32 {
    let c = color.to_rgb();
    let channel = |x: f32| {
        if x <= 0.03928 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
}

/// WCAG 2.x contrast ratio between two colors.
fn contrast(a: Hsla, b: Hsla) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

// ---- text tiers ----------------------------------------------------------

/// Primary text — titles, content.
pub fn t1(cx: &App) -> Hsla {
    cx.theme().foreground
}
/// Secondary text — body, list rows.
pub fn t2(cx: &App) -> Hsla {
    cx.theme().sidebar_foreground
}
/// Resting text for a primary object that is not the selected one — sidebar
/// projects at rest. Derived halfway between `t1` and `t2` so selecting a row
/// can lift it to full strength without the rows around it falling all the way
/// to the weight of their own children.
pub fn t1_soft(cx: &App) -> Hsla {
    mix(t1(cx), t2(cx), 0.5)
}
/// Muted text — labels, steps, inactive rail.
pub fn t3(cx: &App) -> Hsla {
    cx.theme().muted_foreground
}
/// Faint text — timestamps, hints, carets. Derived one step below `t3`.
pub fn t4(cx: &App) -> Hsla {
    mix(t3(cx), base(cx), 0.42)
}

/// Long-form assistant prose. This sits between primary and secondary UI text
/// with an explicit value per Choro palette; it is intentionally chat-only so
/// lifting reading contrast never changes the surrounding application chrome.
pub fn chat_body(cx: &App) -> Hsla {
    let color = match cx.theme().theme_name().as_ref() {
        "Choro Dark" => 0xC7C1C3,
        "Choro Indigo" => 0xC8D0E3,
        "Choro Twilight" => 0xCEC6CA,
        "Choro Dusk" => 0xD7CFCE,
        "Choro Light" => 0x4F484D,
        "Amethyst" | "Ember" => 0xC3C5CC,
        "Mint" => 0xC4C4C9,
        _ if cx.theme().is_dark() => 0xC7C1C3,
        _ => 0x4F484D,
    };
    gpui::rgb(color).into()
}

// ---- lines ---------------------------------------------------------------

/// Hairline — one step off the surface it sits on (region dividers).
pub fn line(cx: &App) -> Hsla {
    cx.theme().border
}
/// Stronger line — control borders, split-button dividers.
pub fn line_2(cx: &App) -> Hsla {
    cx.theme().input
}
/// Subtle but visible hairline inside the `focus` menu plane. Some palettes
/// intentionally place `line` almost directly on `focus`, so menu groups use
/// this derived tier between the focus plane and the stronger control line.
pub fn menu_separator(cx: &App) -> Hsla {
    mix(focus(cx), line_2(cx), 0.38)
}

// ---- accent (live / AI / primary action only) ----------------------------

/// The accent fill.
pub fn accent(cx: &App) -> Hsla {
    cx.theme().primary
}
/// Brighter accent — hover.
pub fn accent_2(cx: &App) -> Hsla {
    cx.theme().primary_hover
}
/// Text/ink that reads on an accent fill (dark ink, never white).
pub fn on_accent(cx: &App) -> Hsla {
    cx.theme().primary_foreground
}
/// Ink for a [`control_accent`] control on `plane` — the accent itself wherever
/// it already carries, otherwise the nearest push that does: lifted in dark
/// themes, deepened toward [`on_accent`] in light ones. Never white, and never
/// the dark-on-bright flip — the accent hue *is* the signal for this tier, so
/// the ink has to stay recognisably the accent while clearing
/// [`ACCENT_INK_MIN_CONTRAST`] on every shipped theme.
pub fn accent_ink(plane: Hsla, cx: &App) -> Hsla {
    accent_ink_color(
        accent(cx),
        on_accent(cx),
        control_accent(plane, cx),
        t1(cx),
        cx.theme().is_dark(),
    )
}

/// Pure form of [`accent_ink`] — see `control_on_fill` for why it is split.
fn accent_ink_color(
    accent: Hsla,
    on_accent: Hsla,
    fill: Hsla,
    fallback: Hsla,
    is_dark: bool,
) -> Hsla {
    for step in 0..=100 {
        let t = step as f32 / 100.0;
        // Dark: lift the accent toward white but hold its hue and saturation —
        // blending toward `t1` would wash the hue out exactly when we need it.
        // Light: deepen toward the theme's own accent ink, which stays in family
        // (dropping lightness at fixed saturation turns the pastel garish).
        let candidate = if is_dark {
            Hsla {
                l: (accent.l + t).min(1.0),
                ..accent
            }
        } else {
            mix(accent, on_accent, t)
        };
        if contrast(candidate, fill) >= ACCENT_INK_MIN_CONTRAST {
            return candidate;
        }
    }
    fallback
}
/// A faint accent wash — selected option rows, live-badge backgrounds.
pub fn accent_soft(cx: &App) -> Hsla {
    accent(cx).opacity(0.13)
}
/// A soft accent border/ring — focus outline, selected pill border.
pub fn accent_line(cx: &App) -> Hsla {
    accent(cx).opacity(0.34)
}

// ---- semantics (state only, on glyphs — never fills except destructive) ---

/// Running / in-progress.
pub fn amber(cx: &App) -> Hsla {
    cx.theme().warning
}
/// Done / additions / success.
pub fn sage(cx: &App) -> Hsla {
    cx.theme().success
}
/// Danger / deletions / blocked.
pub fn rose(cx: &App) -> Hsla {
    cx.theme().danger
}
/// Info / docs / links.
pub fn sky(cx: &App) -> Hsla {
    cx.theme().info
}

/// Soft semantic fills are reserved for compact state controls. The solid
/// semantic color remains the border/glyph, so meaning survives on every theme.
fn semantic_soft(color: Hsla, cx: &App) -> Hsla {
    mix(color, base(cx), 0.78)
}

/// Soft warning tint for selected state controls.
pub fn amber_soft(cx: &App) -> Hsla {
    semantic_soft(amber(cx), cx)
}
/// Soft success/addition tint for selected state controls.
pub fn sage_soft(cx: &App) -> Hsla {
    semantic_soft(sage(cx), cx)
}
/// Soft danger/deletion tint for selected state controls.
pub fn rose_soft(cx: &App) -> Hsla {
    semantic_soft(rose(cx), cx)
}
/// Soft informational tint for selected state controls.
pub fn sky_soft(cx: &App) -> Hsla {
    semantic_soft(sky(cx), cx)
}

/// Contrast ink that reads on a semantic fill (only filled semantic actions use it).
pub fn on_rose(cx: &App) -> Hsla {
    cx.theme().danger_foreground
}
/// Contrast ink on an amber (warning) fill.
pub fn on_amber(cx: &App) -> Hsla {
    cx.theme().warning_foreground
}
/// Contrast ink on a sage (success) fill.
pub fn on_sage(cx: &App) -> Hsla {
    cx.theme().success_foreground
}
/// Contrast ink on a sky (info) fill.
pub fn on_sky(cx: &App) -> Hsla {
    cx.theme().info_foreground
}

// ---- misc surfaces reused across the app ---------------------------------

/// The subtle hover wash for list/table rows (kept translucent so it layers).
pub fn hover(cx: &App) -> Hsla {
    cx.theme().table_hover
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every theme shipped in `choro.json`, reduced to the colors the control
    /// recipes consume. Reading the real asset (rather than fixtures) is the
    /// point: a new theme with an unworkable accent fails these tests.
    struct ThemeColors {
        name: String,
        is_dark: bool,
        focus: Hsla,
        nav: Hsla,
        base: Hsla,
        t1: Hsla,
        t2: Hsla,
        accent: Hsla,
        on_accent: Hsla,
    }

    fn parse(hex: &str) -> Hsla {
        let hex = hex.trim_start_matches('#');
        let value = u32::from_str_radix(&hex[..6], 16).expect("6-digit hex color");
        gpui::rgb(value).into()
    }

    fn themes() -> Vec<ThemeColors> {
        let raw = include_str!("../../../assets/themes/choro.json");
        let root: serde_json::Value = serde_json::from_str(raw).expect("choro.json parses");
        let list = root["themes"].as_array().expect("themes array");
        assert!(!list.is_empty(), "choro.json ships at least one theme");

        list.iter()
            .map(|theme| {
                let c = &theme["colors"];
                let get = |key: &str| parse(c[key].as_str().unwrap_or_else(|| panic!("{key}")));
                ThemeColors {
                    name: theme["name"].as_str().expect("theme name").to_string(),
                    is_dark: theme["mode"] == "dark",
                    focus: get("popover.background"),
                    nav: get("sidebar.background"),
                    base: get("background"),
                    t1: get("foreground"),
                    t2: get("sidebar.foreground"),
                    accent: get("primary.background"),
                    on_accent: get("primary.foreground"),
                }
            })
            .collect()
    }

    fn luminance(color: Hsla) -> f32 {
        let c = color.to_rgb();
        let channel = |x: f32| {
            if x <= 0.03928 {
                x / 12.92
            } else {
                ((x + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b)
    }

    /// WCAG 2.x contrast ratio.
    fn contrast(a: Hsla, b: Hsla) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    const AA: f32 = 4.5;

    #[test]
    fn sidebar_glow_is_theme_colored_restrained_and_readable() {
        for t in themes() {
            let glow = nav_glow_color(t.nav, t.accent, t.is_dark);
            let surface_gap = contrast(glow, t.nav);
            let minimum_gap = if t.is_dark { 1.02 } else { 1.01 };

            assert!(
                surface_gap >= minimum_gap,
                "{}: sidebar glow is imperceptible at {surface_gap:.3}:1",
                t.name
            );
            assert!(
                surface_gap <= 1.50,
                "{}: sidebar glow is too strong at {surface_gap:.3}:1",
                t.name
            );

            let text_ratio = contrast(t.t2, glow);
            assert!(
                text_ratio >= AA,
                "{}: sidebar foreground is only {text_ratio:.2}:1 on the glow",
                t.name
            );
        }
    }

    /// The accent tier's fill as the app builds it, for a parsed theme.
    fn ship_fill(t: &ThemeColors) -> Hsla {
        control_accent_fill(
            t.focus,
            t.accent,
            t.on_accent,
            t.t1,
            ACCENT_TIER_STEP,
            t.is_dark,
        )
    }

    #[test]
    fn dialog_control_ink_clears_aa_on_every_theme() {
        for t in themes() {
            let fill = control_on_fill(t.focus, t.t1);
            let ratio = contrast(t.t1, fill);
            assert!(
                ratio >= AA,
                "{}: dialog control ink {ratio:.2}:1 is below AA {AA}:1",
                t.name
            );
        }
    }

    /// The accent tier's fill and ink are solved against each other, so this is
    /// the check that the solver actually converged on every theme.
    #[test]
    fn ship_wash_ink_clears_aa_on_every_theme() {
        for t in themes() {
            let fill = ship_fill(&t);
            let ink = accent_ink_color(t.accent, t.on_accent, fill, t.t1, t.is_dark);
            let ratio = contrast(ink, fill);
            assert!(
                ratio >= AA,
                "{}: ship wash ink {ratio:.2}:1 is below AA {AA}:1",
                t.name
            );
        }
    }

    /// Regression: the accent tier first shipped as a fixed 0.16 blend, which
    /// left Ship only 1.12:1 from the neutral button beside it and inked in a
    /// near-white `accent_2` — so it read as a slightly purple neutral rather
    /// than its own tier. Ship has to be visibly *not* a neutral control.
    #[test]
    fn ship_is_distinguishable_from_the_neutral_beside_it() {
        for t in themes() {
            let neutral = control_on_fill(t.focus, t.t1);
            let fill = ship_fill(&t);
            let ink = accent_ink_color(t.accent, t.on_accent, fill, t.t1, t.is_dark);

            let fill_gap = contrast(fill, neutral);
            assert!(
                fill_gap >= 1.15,
                "{}: ship fill is only {fill_gap:.3}:1 from the neutral beside it",
                t.name
            );

            // Ship reads as its own tier through the fill, the ink, or both —
            // but at least one has to carry. Which one is a property of the
            // palette, not the recipe: Choro Indigo's accent (#CAC9EE) is within
            // 1.25:1 of its own body text (#DDE3F2), so there the ink can never
            // separate and the fill does all the work. Requiring both would fail
            // that theme for something no tuning here can fix.
            let ink_gap = contrast(ink, t.t1);
            assert!(
                ink_gap >= 1.25 || fill_gap >= 1.25,
                "{}: ship is invisible as a tier — fill {fill_gap:.3}:1 from the \
                 neutral and ink {ink_gap:.3}:1 from ordinary text",
                t.name
            );
        }
    }

    /// A fill-first control carries no stroke, so the fill alone has to read as
    /// a button — it must actually separate from the plane it sits on.
    #[test]
    fn controls_separate_from_their_plane_on_every_theme() {
        for t in themes() {
            for (label, fill) in [
                ("dialog control", control_on_fill(t.focus, t.t1)),
                ("ship wash", ship_fill(&t)),
            ] {
                let ratio = contrast(fill, t.focus);
                assert!(
                    ratio >= 1.15,
                    "{}: {label} only separates {ratio:.3}:1 from its plane",
                    t.name
                );
            }
        }
    }

    /// Regression: `control_recessed` derived its fill from `surface` no matter
    /// which box it sat in, so a dialog control landed a full plane too low —
    /// darker than the sidebar, reading as a hole punched in the app's
    /// lightest surface. A control must step *toward* the viewer from its box.
    #[test]
    fn dialog_control_never_sinks_below_the_nav_plane() {
        for t in themes().into_iter().filter(|t| t.is_dark) {
            let fill = control_on_fill(t.focus, t.t1);
            assert!(
                fill.l > t.focus.l,
                "{}: dialog control ({:.3}) must lift off its plane ({:.3})",
                t.name,
                fill.l,
                t.focus.l
            );
            assert!(
                fill.l > t.nav.l,
                "{}: dialog control ({:.3}) is darker than the sidebar ({:.3})",
                t.name,
                fill.l,
                t.nav.l
            );
        }
    }
}
