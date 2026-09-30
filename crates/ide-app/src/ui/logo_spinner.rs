use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::time::Duration;

use gpui::{
    canvas, div, linear_color_stop, linear_gradient, point, px, Animation, AnimationExt,
    AnyElement, Background, Bounds, Hsla, IntoElement, ParentElement, PathBuilder, Pixels, Styled,
    Window,
};

/// One full revolution of the arc.
const CYCLE: Duration = Duration::from_millis(1600);
/// Fraction of the circle the moving arc covers (the rest shows the track).
const SWEEP: f32 = 0.70 * TAU;
/// Stroke width as a fraction of the diameter.
const LW_FRAC: f32 = 0.15;
/// Twelve o'clock — the arc's leading edge starts here.
const TOP: f32 = -FRAC_PI_2;

/// Paint a stroked circular arc of radius `r` and width `w`, from angle `a0`
/// to `a1` (the long way round), filled with `bg`.
fn stroke_arc(
    window: &mut Window,
    cx: f32,
    cy: f32,
    r: f32,
    w: f32,
    a0: f32,
    a1: f32,
    bg: impl Into<Background>,
) {
    let mut pb = PathBuilder::stroke(px(w));
    pb.move_to(point(px(cx + r * a0.cos()), px(cy + r * a0.sin())));
    pb.arc_to(
        point(px(r), px(r)),
        px(0.0),
        (a1 - a0).abs() > PI,
        true,
        point(px(cx + r * a1.cos()), px(cy + r * a1.sin())),
    );
    if let Ok(path) = pb.build() {
        window.paint_path(path, bg);
    }
}

/// Paint a full stroked circle (two half-arcs) — the background track.
fn stroke_circle(window: &mut Window, cx: f32, cy: f32, r: f32, w: f32, bg: impl Into<Background>) {
    let mut pb = PathBuilder::stroke(px(w));
    pb.move_to(point(px(cx + r), px(cy)));
    pb.arc_to(
        point(px(r), px(r)),
        px(0.0),
        false,
        true,
        point(px(cx - r), px(cy)),
    );
    pb.arc_to(
        point(px(r), px(r)),
        px(0.0),
        false,
        true,
        point(px(cx + r), px(cy)),
    );
    if let Ok(path) = pb.build() {
        window.paint_path(path, bg);
    }
}

/// Paint the spinner at rotation `rot` within `bounds`.
fn paint_spinner(bounds: Bounds<Pixels>, rot: f32, window: &mut Window) {
    let width = f32::from(bounds.size.width);
    let height = f32::from(bounds.size.height);
    let s = width.min(height);
    let cx = f32::from(bounds.origin.x) + width / 2.0;
    let cy = f32::from(bounds.origin.y) + height / 2.0;
    let lw = (s * LW_FRAC).max(2.4);
    let r = s / 2.0 - lw / 2.0 - 1.0;

    // Faint full-circle track underneath.
    let track: Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_TRACK).into();
    stroke_circle(window, cx, cy, r, lw, track.opacity(0.16));

    // The 70% purple arc: a 3D lavender gradient (lit top-left, deeper toward
    // the bottom-right) with a bright outer rim and a shadowed inner edge.
    let head = TOP + rot;
    let a0 = head - SWEEP;
    stroke_arc(
        window,
        cx,
        cy,
        r,
        lw,
        a0,
        head,
        linear_gradient(
            135.0,
            linear_color_stop(
                gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_BRIGHT),
                0.0,
            ),
            linear_color_stop(
                gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_DEEP),
                1.0,
            ),
        ),
    );
    let light: Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_LIGHT).into();
    stroke_arc(
        window,
        cx,
        cy,
        r + lw * 0.30,
        (lw * 0.24).max(0.7),
        a0 + 0.06,
        head - 0.06,
        light.opacity(0.45),
    );
    let shadow: Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_SHADOW).into();
    stroke_arc(
        window,
        cx,
        cy,
        r - lw * 0.30,
        (lw * 0.22).max(0.7),
        a0 + 0.06,
        head - 0.06,
        shadow.opacity(0.4),
    );
}

/// A loading spinner in the app's colors: a 70% purple gradient arc sweeping
/// around a faint circular track.
pub fn logo_spinner(
    diameter: f32,
    namespace: &'static str,
    seed: usize,
    _line_color: Hsla,
) -> AnyElement {
    let s = diameter;
    div()
        .relative()
        .size(px(s))
        .flex_shrink_0()
        .child(
            canvas(|_, _, _| (), |_, _, _, _| ())
                .size(px(s))
                .with_animation(
                    (namespace, seed),
                    Animation::new(CYCLE).repeat(),
                    move |_canvas, delta| {
                        let rot = TAU * delta;
                        canvas(
                            move |_, _, _| (),
                            move |bounds, _, window, _| paint_spinner(bounds, rot, window),
                        )
                        .size(px(s))
                    },
                ),
        )
        .into_any_element()
}
