//! Theme-aware empty-state illustrations.
//!
//! Each illustration is an SVG *template* (`assets/illustrations/*.svg`) whose
//! colors are placeholders like `{{spark}}`. At render time we derive a small
//! palette from the active theme's tokens ([`token::base`], [`token::t1`],
//! [`token::accent`]) and substitute, then hand the resulting SVG bytes to
//! [`gpui::img`]. gpui keys the rasterization cache on the byte hash
//! ([`gpui::Image::from_bytes`]), so a given theme only rasterizes once — yet
//! every Choro theme (Dark, Twilight, Dusk, Light, …) paints the art in its own
//! surface, ink, and accent instead of shipping one fixed-color image.
//!
//! Colors are baked opaque (blended against the canvas) so resvg never has to
//! composite alpha at stroke edges. The single [`Slot::Spark`] is the theme's
//! live accent — the one bright focal point, per the design system's accent law.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use gpui::{img, App, Hsla, Img, Rgba};

use crate::ui::design::token;

/// Runtime-generated themed illustration SVGs, keyed by a content-derived asset
/// path. `AppAssets::load` serves these, so the illustrations render through
/// gpui's normal SVG asset pipeline — which supersamples at `SMOOTH_SVG_SCALE_FACTOR`
/// and tags the resulting image's device density. That's what keeps them crisp
/// on every display (a hand-built `RenderImage` can't set that density factor).
/// A theme change substitutes new colors → new bytes → new path → gpui re-renders.
static THEMED_SVGS: LazyLock<Mutex<HashMap<String, Vec<u8>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Serve a runtime themed illustration by its generated asset path. Called by the
/// app asset source for any `illustrations/themed/*.svg` request.
pub fn themed_svg(path: &str) -> Option<Vec<u8>> {
    THEMED_SVGS.lock().ok()?.get(path).cloned()
}

/// An empty-state illustration. One template SVG each; colored per theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Illustration {
    Database,
    Resume,
    Tasks,
    Docs,
    Assets,
    Services,
    CodeTerminal,
    Agents,
}

impl Illustration {
    fn template(self) -> &'static str {
        match self {
            Illustration::Database => {
                include_str!("../../assets/illustrations/database-connection.svg")
            }
            Illustration::Resume => {
                include_str!("../../assets/illustrations/resume-conversation-history.svg")
            }
            Illustration::Tasks => {
                include_str!("../../assets/illustrations/tasks-first-task.svg")
            }
            Illustration::Docs => {
                include_str!("../../assets/illustrations/docs-new-document.svg")
            }
            Illustration::Assets => {
                include_str!("../../assets/illustrations/assets-add-reference.svg")
            }
            Illustration::Services => {
                include_str!("../../assets/illustrations/services-connected.svg")
            }
            Illustration::CodeTerminal => {
                include_str!("../../assets/illustrations/code-terminal.svg")
            }
            Illustration::Agents => {
                include_str!("../../assets/illustrations/agents-empty.svg")
            }
        }
    }
}

/// A themed illustration as an `img` element. Chain `.size_full()` /
/// `.object_fit(..)` at the call site, exactly like the old `img(path)`.
pub fn illustration(kind: Illustration, cx: &App) -> Img {
    let svg = Palette::from_theme(cx).apply(kind.template());
    // Register the themed SVG under a content-keyed path so gpui's asset pipeline
    // renders it (crisp, device-density-aware). Same theme → same bytes → same
    // path → gpui reuses its cached render.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg.hash(&mut hasher);
    let path = format!(
        "illustrations/themed/{:?}-{:016x}.svg",
        kind,
        hasher.finish()
    );
    if let Ok(mut map) = THEMED_SVGS.lock() {
        map.entry(path.clone()).or_insert_with(|| svg.into_bytes());
    }
    img(path)
}

/// Blend `a` toward `b` by `t` (0..1) in RGB — mirrors `design::token`'s private
/// mixer so derived tiers still track the theme.
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

/// A neutral structure tone: mix the canvas toward ink, then pull most of the
/// saturation out. Warm-tinted themes (Dusk, Twilight) otherwise render the
/// linework muddy/brown — this keeps each theme's light/dark *level* but drops
/// its color *cast*, so the structure reads as clean slate and the lone accent
/// spark stays the only color in the frame.
fn neutral(bg: Hsla, ink: Hsla, t: f32) -> Hsla {
    let m = mix(bg, ink, t);
    Hsla { s: m.s * 0.40, ..m }
}

fn hex(c: Hsla) -> String {
    let c = c.to_rgb();
    let ch = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", ch(c.r), ch(c.g), ch(c.b))
}

/// The theme accent, re-tuned to read against the canvas. Choro's accent is a
/// pale fill (light-lavender button with dark ink) — bright on dark themes but
/// nearly invisible as a thin stroke on a light one. So we keep the accent's
/// *hue* but pin its *lightness*: deeper on a light canvas, bright on a dark one.
/// `min_s` floors saturation so the mark still reads as the accent, not gray.
fn accent_readable(acc: Hsla, light_canvas: bool, light_l: f32, dark_l: f32, min_s: f32) -> Hsla {
    Hsla {
        h: acc.h,
        s: acc.s.max(min_s),
        l: if light_canvas { light_l } else { dark_l },
        a: 1.0,
    }
}

/// The illustration palette for one theme. Fills are subtle tints of the canvas;
/// strokes track the theme's ink; the spark and accented cards track its accent.
struct Palette {
    ground: String,
    back_fill: String,
    back_stroke: String,
    connector: String,
    card_fill: String,
    card_stroke: String,
    content_line: String,
    chip_glyph: String,
    ambient: String,
    raised_fill: String,
    accent_stroke: String,
    ring: String,
    cap_highlight: String,
    spark: String,
}

impl Palette {
    fn from_theme(cx: &App) -> Self {
        let bg = token::base(cx);
        let ink = token::t1(cx);
        let acc = token::accent(cx);
        let light = bg.l > 0.5;
        // The clean accent line color, reused so accent fills tint toward the same
        // hue rather than muddying against a warm-tinted background.
        // Contrast is what reads as "crisp": faint low-contrast linework looks soft
        // no matter how sharply it's rasterized. These tiers sit high enough off the
        // canvas to read as clean, definite lines.
        let acc_line = accent_readable(acc, light, 0.50, 0.76, 0.26);
        Self {
            ground: hex(neutral(bg, ink, 0.12)),
            back_fill: hex(neutral(bg, ink, 0.06)),
            back_stroke: hex(neutral(bg, ink, 0.32)),
            connector: hex(neutral(bg, ink, 0.28)),
            card_fill: hex(neutral(bg, ink, 0.09)),
            card_stroke: hex(neutral(bg, ink, 0.62)),
            content_line: hex(neutral(bg, ink, 0.40)),
            chip_glyph: hex(neutral(bg, ink, 0.50)),
            ambient: hex(neutral(bg, ink, 0.36)),
            // Accent-tinted fills = clean neutral base + a touch of the clean
            // accent, so they read as a crisp tint, never a warm-bg smear.
            raised_fill: hex(mix(
                neutral(bg, ink, 0.09),
                acc_line,
                if light { 0.24 } else { 0.20 },
            )),
            cap_highlight: hex(mix(
                neutral(bg, ink, 0.12),
                acc_line,
                if light { 0.30 } else { 0.26 },
            )),
            // Accent lines + the one spark are lightness-tuned for legibility, so
            // the spark stays the brightest mark on every theme.
            accent_stroke: hex(acc_line),
            ring: hex(accent_readable(acc, light, 0.60, 0.62, 0.16)),
            spark: hex(accent_readable(acc, light, 0.42, 0.85, 0.40)),
        }
    }

    fn apply(&self, template: &str) -> String {
        let mut s = template.to_string();
        for (slot, value) in [
            ("{{ground}}", &self.ground),
            ("{{back_fill}}", &self.back_fill),
            ("{{back_stroke}}", &self.back_stroke),
            ("{{connector}}", &self.connector),
            ("{{card_fill}}", &self.card_fill),
            ("{{card_stroke}}", &self.card_stroke),
            ("{{content_line}}", &self.content_line),
            ("{{chip_glyph}}", &self.chip_glyph),
            ("{{ambient}}", &self.ambient),
            ("{{raised_fill}}", &self.raised_fill),
            ("{{accent_stroke}}", &self.accent_stroke),
            ("{{ring}}", &self.ring),
            ("{{cap_highlight}}", &self.cap_highlight),
            ("{{spark}}", &self.spark),
        ] {
            s = s.replace(slot, value);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_readable_preserves_lavender_hue() {
        // Choro Indigo / Dark primary.
        let acc: Hsla = gpui::rgb(0xCAC9EE).into();
        eprintln!("acc:    h={:.3} s={:.3} l={:.3}", acc.h, acc.s, acc.l);
        let stroke = accent_readable(acc, false, 0.54, 0.68, 0.22);
        let spark = accent_readable(acc, false, 0.44, 0.80, 0.36);
        eprintln!(
            "stroke: h={:.3} s={:.3} l={:.3} -> {}",
            stroke.h,
            stroke.s,
            stroke.l,
            hex(stroke)
        );
        eprintln!(
            "spark:  h={:.3} s={:.3} l={:.3} -> {}",
            spark.h,
            spark.s,
            spark.l,
            hex(spark)
        );
        // Hue must stay in the blue-violet band (~0.62..0.72), never near red (~0).
        assert!(
            (0.60..0.75).contains(&stroke.h),
            "accent hue drifted off lavender: {}",
            stroke.h
        );
    }
}
