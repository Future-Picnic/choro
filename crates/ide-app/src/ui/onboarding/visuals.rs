use super::*;

pub fn target_marker(target: SpotlightTarget, cx: &App) -> gpui::AnyElement {
    let Some(tour) = scoped_tour(cx) else {
        // Target markers are overlays and must never consume flex space. A
        // plain empty div here still participates in rows and introduces their
        // configured gap before the first real indicator.
        return div().absolute().inset_0().into_any_element();
    };
    canvas(
        move |bounds, _, cx| {
            tour.update(cx, |tour, cx| tour.register_target(target, bounds, cx));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

// ---- tour surfaces -------------------------------------------------------
//
// The tour floats above the live app, so it speaks the app's own elevated-card
// language instead of inventing one: the `focus` plane, a visible edge, and the
// canonical soft shadow (design law 2 — elevation is border + shadow, never a
// bright lift). The accent never outlines a box; it stays in the ink (eyebrow,
// medallion glyph, the one primary action) and on the spotlight ring, which is
// the single thing on screen that must pull the eye.

/// Scrim behind a full-takeover step — nothing under it is actionable.
pub(super) const SHADE_MODAL: f32 = 0.92;
/// Scrim behind a spotlight step — the app must stay readable beneath it.
pub(super) const SHADE_SPOTLIGHT: f32 = 0.84;
/// An agent card on the provider step. The stack inside it measures ~114px —
/// a 38px plate, the name and status pair, and the Default slot, with 10px
/// between the three groups — so the card is sized to leave a clear 14px of air
/// above the plate and below the badge. At 118 the content filled the card to
/// within 3px and the badge read as falling out of the bottom edge.
pub(super) const PROVIDER_CARD_H: f32 = 142.0;
/// The Default badge's row, held open on every card whether or not it carries
/// the badge. Without it the badge would push one card's name and status up
/// while its neighbours sat lower — the misalignment that made the first pass
/// look accidental.
pub(super) const PROVIDER_BADGE_H: f32 = 18.0;
/// Rough height of the instruction card, for centring the map's copy. Only ever
/// an estimate — the card sizes to its text — so it is used for placement, never
/// for clipping.
pub(super) const MAP_CARD_H: f32 = 210.0;

/// The instruction card's step glyph plate. Sized against the 11px label it now
/// sits beside, not against the title it used to tower over.
const PLATE: f32 = 22.0;
/// The tour's chapters. One segment each on the ribbon, one row each on the
/// waiting card — the single place the tour's length is decided.
pub(super) const STEPS: usize = 4;

/// The chapters, named. A ribbon says *how far*; only names can say *what* — so
/// while an agent works, the sidebar card lists all six and answers the three
/// questions a tour actually has to answer: what's done, what's happening now,
/// and what's coming.
pub(super) const STEP_NAMES: [&str; STEPS] = [
    "Your first agent",
    "Anything into an agent",
    "Ship it",
    "See it run",
];

// ---- the two takeover moments --------------------------------------------
//
// Welcome and finale are not instruction cards and shouldn't be dressed like
// them. They own the whole screen, they're read once, and they're the only two
// places the tour gets to make an argument rather than give an order — so they
// take the display type, a real hero CTA, and room to breathe.

pub(super) const WELCOME_W: f32 = 640.0;
/// The hero CTA — taller and wider than the app's 28px control, because these
/// two screens have exactly one thing to press.
const HERO_CTA_H: f32 = 36.0;
/// Beak geometry: how far it juts from the card, and how wide its base is.
const BEAK_D: f32 = 8.0;
const BEAK_W: f32 = 16.0;
/// Keeps the beak off the card's rounded corners, and — paired with
/// [`BEAK_SAFE_SPAN`] — inside the shortest card the tour renders.
pub(super) const BEAK_INSET: f32 = 24.0;
pub(super) const BEAK_SAFE_SPAN: f32 = 120.0;

/// Breathing room between a revealed element and the shade around it.
pub(super) const SPOTLIGHT_MARGIN: f32 = 7.0;

/// The beacon: a dot that ripples on the thing you're meant to press.
///
/// The ring says "this is highlighted"; that's an annotation, and in testing it
/// wasn't enough — people read the card, found no obvious control on it, and
/// pressed the only button they could see, which was Exit. A beacon is not an
/// annotation, it's an instruction: *press this*. It only appears on steps whose
/// answer is out in the app rather than on the card ([`Phase::has_continue`]),
/// and it loops, because it is asking for something and shouldn't stop until it
/// gets it. It rides the halo's existing frame cost — the spotlight already
/// animates, so this adds no new class of work.
const BEACON_MS: u64 = 1800;
const BEACON_DOT: f32 = 7.0;
const BEACON_REACH: f32 = 11.0;

pub(super) fn beacon(at: (f32, f32), color: gpui::Hsla) -> gpui::AnyElement {
    let (bx, by) = at;
    div()
        .absolute()
        .with_animation(
            "onboarding-beacon",
            Animation::new(Duration::from_millis(BEACON_MS)).repeat(),
            move |_, delta| {
                // gpui can't scale a div, so the ripple grows by recomputing its
                // box each frame. Two of them, half a cycle apart, so there's
                // always one travelling.
                let ripple = |phase: f32| {
                    let t = (delta + phase) % 1.0;
                    let r = BEACON_DOT / 2.0 + (BEACON_REACH - BEACON_DOT / 2.0) * t;
                    div()
                        .absolute()
                        .left(px(bx - r))
                        .top(px(by - r))
                        .size(px(r * 2.0))
                        .rounded_full()
                        .bg(color.opacity(0.45 * (1.0 - t)))
                };
                div()
                    .absolute()
                    .child(ripple(0.0))
                    .child(ripple(0.5))
                    .child(
                        div()
                            .absolute()
                            .left(px(bx - BEACON_DOT / 2.0))
                            .top(px(by - BEACON_DOT / 2.0))
                            .size(px(BEACON_DOT))
                            .rounded_full()
                            .bg(color),
                    )
            },
        )
        .into_any_element()
}

/// The shade, as the complement of any number of holes.
///
/// A step can need to light several things at once that aren't neighbours — the
/// agent's header, the Git panel down the right, the scripts up top. That is the
/// "nothing scattered" claim made by showing rather than saying, and one rect
/// can't do it: the union of those swallows the whole window.
///
/// This was horizontal bands, which quietly assumed no two holes ever shared a
/// row — the Git panel runs the full height, so it overlapped everything and the
/// later holes were silently *lost* (dimmed, not revealed). So instead: cut the
/// screen on every hole edge, keep the cells no hole covers, and merge each row
/// back into runs. Handles any arrangement, overlapping or not, and emits a
/// handful of rects rather than a grid of them.
pub(super) fn shade_holes(
    holes: &[Bounds<Pixels>],
    screen: gpui::Size<Pixels>,
    shade: gpui::Hsla,
) -> Vec<gpui::AnyElement> {
    let (sw, sh) = (f32::from(screen.width), f32::from(screen.height));
    let cuts: Vec<(f32, f32, f32, f32)> = holes
        .iter()
        .map(|b| {
            (
                (f32::from(b.left()) - SPOTLIGHT_MARGIN).max(0.0),
                (f32::from(b.top()) - SPOTLIGHT_MARGIN).max(0.0),
                (f32::from(b.right()) + SPOTLIGHT_MARGIN).min(sw),
                (f32::from(b.bottom()) + SPOTLIGHT_MARGIN).min(sh),
            )
        })
        .filter(|(l, t, r, b)| r > l && b > t)
        .collect();
    if cuts.is_empty() {
        return vec![div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(shade)
            .into_any_element()];
    }

    let axis = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        v
    };
    let xs = axis(
        std::iter::once(0.0)
            .chain(std::iter::once(sw))
            .chain(cuts.iter().flat_map(|c| [c.0, c.2]))
            .collect(),
    );
    let ys = axis(
        std::iter::once(0.0)
            .chain(std::iter::once(sh))
            .chain(cuts.iter().flat_map(|c| [c.1, c.3]))
            .collect(),
    );

    let mut out = Vec::new();
    for row in ys.windows(2) {
        let (t, b) = (row[0], row[1]);
        let (cy, h) = (0.5 * (t + b), b - t);
        if h <= 0.0 {
            continue;
        }
        // Walk the row and merge consecutive un-holed cells into one rect.
        let mut run: Option<f32> = None;
        for i in 0..xs.len() - 1 {
            let (l, r) = (xs[i], xs[i + 1]);
            let cx = 0.5 * (l + r);
            let covered = cuts
                .iter()
                .any(|(hl, ht, hr, hb)| cx > *hl && cx < *hr && cy > *ht && cy < *hb);
            match (covered, run) {
                (false, None) => run = Some(l),
                (true, Some(start)) => {
                    out.push((start, t, l - start, h));
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(start) = run {
            out.push((start, t, sw - start, h));
        }
    }
    out.into_iter()
        .filter(|(_, _, w, h)| *w > 0.5 && *h > 0.5)
        .map(|(x, y, w, h)| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .occlude()
                .bg(shade)
                .into_any_element()
        })
        .collect()
}

/// Which edge of the card the beak sits on — the edge facing the target.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Beak {
    Left,
    Right,
    Top,
    Bottom,
}

/// The beak: the bit that turns a card sitting near a thing into a card that is
/// clearly *about* that thing. gpui can only rotate `svg()`, not a `Div`, so the
/// triangle is painted by hand — filled with the card's own plane, then stroked
/// down its two outer edges to continue the card's border around the point. The
/// base deliberately overhangs into the card by a pixel so the card's own border
/// doesn't draw a line across the beak's mouth.
pub(super) fn beak(
    side: Beak,
    at: f32,
    card_left: f32,
    card_top: f32,
    card_w: f32,
    cx: &App,
) -> Div {
    let fill = crate::ui::design::focus(cx);
    let edge = crate::ui::design::line_2(cx);
    let (w, h) = match side {
        Beak::Left | Beak::Right => (BEAK_D + 1.5, BEAK_W),
        Beak::Top | Beak::Bottom => (BEAK_W, BEAK_D + 1.5),
    };
    let (x, y) = match side {
        Beak::Left => (card_left - BEAK_D, at - BEAK_W / 2.0),
        Beak::Right => (card_left + card_w - 1.5, at - BEAK_W / 2.0),
        Beak::Top => (at - BEAK_W / 2.0, card_top - BEAK_D),
        Beak::Bottom => (at - BEAK_W / 2.0, card_top - 1.5),
    };
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .child(canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let o = bounds.origin;
                let ox = f32::from(o.x);
                let oy = f32::from(o.y);
                // tip, then the two base corners that meet the card edge.
                let (tip, a, b) = match side {
                    Beak::Left => ((ox, oy + BEAK_W / 2.0), (ox + w, oy), (ox + w, oy + BEAK_W)),
                    Beak::Right => ((ox + w, oy + BEAK_W / 2.0), (ox, oy), (ox, oy + BEAK_W)),
                    Beak::Top => ((ox + BEAK_W / 2.0, oy), (ox, oy + h), (ox + BEAK_W, oy + h)),
                    Beak::Bottom => ((ox + BEAK_W / 2.0, oy + h), (ox, oy), (ox + BEAK_W, oy)),
                };
                let pt = |p: (f32, f32)| gpui::point(px(p.0), px(p.1));
                let mut face = gpui::PathBuilder::fill();
                face.move_to(pt(a));
                face.line_to(pt(tip));
                face.line_to(pt(b));
                face.close();
                if let Ok(path) = face.build() {
                    window.paint_path(path, fill);
                }
                let mut rim = gpui::PathBuilder::stroke(px(1.0));
                rim.move_to(pt(a));
                rim.line_to(pt(tip));
                rim.line_to(pt(b));
                if let Ok(path) = rim.build() {
                    window.paint_path(path, edge);
                }
            },
        ))
}
/// Breathing room between the spotlight ring and the halo that softens it.
pub(super) const HALO_SPREAD: f32 = 5.0;
/// The halo's breath. One slow, shallow cycle — the only motion in the tour, so
/// it has to read as alive rather than impatient. These three are the switch:
/// set `HALO_MIN == HALO_MAX` to hold it still.
pub(super) const HALO_CYCLE: Duration = Duration::from_millis(2600);
pub(super) const HALO_MIN: f32 = 0.18;
pub(super) const HALO_MAX: f32 = 0.5;
/// Instruction-card width. Shared by the card and the placement math that has
/// to know where it will land, so the two can never disagree. Sized by the
/// footer rather than the prose — Previous + Exit tour + the longest continue
/// label ("Meet your first agent") is the widest row the card must hold, and
/// the card clips whatever it can't fit.
pub(super) const CARD_W: f32 = 400.0;

/// The dimmed backdrop a tour step sits on.
pub(super) fn scrim(shade: f32, cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(crate::ui::design::base(cx).opacity(shade))
}

/// The tour's elevated card — one plane, edge, radius, and shadow for every
/// step, so the welcome, provider, finale, waiting, and instruction cards read
/// as the same object moving through the tour.
pub(super) fn tour_card(cx: &App) -> Div {
    v_flex()
        .occlude()
        .overflow_hidden()
        .rounded(crate::ui::design::r_lg())
        .border_1()
        .border_color(crate::ui::design::line_2(cx))
        .bg(crate::ui::design::focus(cx))
        .shadow(crate::ui::design::shadow())
}

/// A card's action tray. Separated from the body by a plane step rather than a
/// rule: `base` sits below `focus` on every shipped theme, so the tray recedes
/// by the same amount in light and dark without a stroke.
pub(super) fn tour_footer(cx: &App) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .bg(crate::ui::design::base(cx))
}

/// The one action on a takeover screen. Same accent recipe as every primary in
/// the app — just given the room the moment deserves.
pub(super) fn hero_cta(
    id: &'static str,
    label: &'static str,
    cx: &App,
) -> gpui_component::button::Button {
    style::primary_button(id, label, cx)
        .h(px(HERO_CTA_H))
        .px_5()
        .icon(IconName::ArrowRight)
}

/// The display line that opens a takeover screen — the tour's one hero voice.
pub(super) fn hero_title(text: &'static str, cx: &App) -> Div {
    div()
        .text_size(crate::ui::design::text_display())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .line_height(gpui::relative(1.18))
        .text_color(crate::ui::design::t1(cx))
        .child(text)
}

/// The uppercase brand line above a card title.
pub(super) fn eyebrow(text: &'static str, cx: &App) -> Div {
    div()
        .text_size(crate::ui::design::text_label())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::accent(cx))
        .child(text)
}

// ---- the welcome hub ----------------------------------------------------
//
// Choro's icon is already the argument the tour is making: a C wrapping four
// strands that converge into one bright node — many threads in, one thing out,
// nothing dropped. So the hero doesn't illustrate the idea beside the brand; it
// draws the brand, and the brand says it. Around it, the project's tools stream
// in, in the same visual language as the landing page's manifesto hub.
//
// This is the mark rendered flat and line-first, not the glossy 3D of the app
// icon — the identity in the app's own language. The purples are the brand ramp
// the spinner already uses: here Choro is Choro on every theme, the way a logo
// doesn't restyle itself per palette.
//
// Everything is ONE-SHOT. gpui only re-requests frames while an animation runs
// (`if !done { request_animation_frame() }`), so once the last pulse lands the
// hero stops asking for frames entirely and the screen costs nothing to sit on.
// A looping flow would repaint at refresh rate for as long as the card is open.

const HUB_W: f32 = WELCOME_W - 64.0;
const HUB_H: f32 = 224.0;
const HUB_MID_Y: f32 = 112.0;
/// The mark is authored in its own 112-unit box, then placed into hub space.
const MARK_SCALE: f32 = 0.786;
const MARK_OX: f32 = 244.0;
const MARK_OY: f32 = 68.0;

const MARK_CX: f32 = 52.0;
const MARK_CY: f32 = 56.0;
const MARK_R: f32 = 34.0;
const MARK_STROKE: f32 = 12.0;
/// Half the C's opening, centred due east — where the node sits.
const MARK_GAP_DEG: f32 = 46.0;
const STRAND_X0: f32 = 25.0;
const STRAND_Y: [f32; 4] = [45.0, 51.0, 61.0, 67.0];
const STRAND_W: f32 = 3.2;
const MARK_NODE_X: f32 = 69.0;
const MARK_NODE_R: f32 = 5.2;
/// The mark's trailing square, sitting just outside the C's mouth.
const MARK_SQUARE_X: f32 = 81.5;
const MARK_SQUARE: f32 = 7.5;
const SAMPLES: usize = 40;

/// Tool chips: four down each side, streaming into the mark.
const CHIP_H: f32 = 26.0;
const CHIP_ROWS: [f32; 4] = [32.0, 88.0, 144.0, 196.0];
/// The inner edge each column of chips aligns to — and where its stream starts.
const CHIP_EDGE_L: f32 = 108.0;
const CHIP_EDGE_R: f32 = 470.0;
/// Where the streams land on the mark. Both are derived from the mark itself
/// rather than eyeballed beside it: the left column meets the C's outer edge,
/// the right column runs into the trailing square and tucks under it (the square
/// paints after the canvas, so the tips disappear beneath it). Hard-coding these
/// left the right-hand streams stopping ~15px short, pointing at nothing.
const LAND_L: f32 = MARK_OX + (MARK_CX - MARK_R - MARK_STROKE / 2.0) * MARK_SCALE;
const LAND_R: f32 = MARK_OX + MARK_SQUARE_X * MARK_SCALE;
/// The chips arrive alternating side to side, so the hub fills evenly instead of
/// sweeping one flank and then the other. Position in the queue, per chip.
const ARRIVAL: [usize; 8] = [0, 2, 4, 6, 1, 3, 5, 7];
/// How much of a stream the travelling pulse occupies.
const PULSE_LEN: f32 = 0.07;

/// The pulse beats, named rather than inlined so [`HUB_MS`] can be derived from
/// them instead of guessed alongside them.
const QUEUE_STEP: f32 = 100.0;
const PULSE_START: f32 = 2150.0;
const PULSE_TRAVEL: f32 = 950.0;
const REST_START: f32 = 2050.0;
const REST_FADE: f32 = 600.0;
const CHIP_START: f32 = 2000.0;
const CHIP_FADE: f32 = 440.0;

/// A held breath before anything draws. The card lands, you read the greeting,
/// and *then* the brand assembles — without it the animation is half over before
/// your eye has arrived, which is no use to the one screen that has to land.
/// gpui's `Animation` has no delay, so the whole timeline shifts inside [`at`]
/// instead and every beat below keeps its own honest millisecond. A second is
/// the whole budget: long enough for the card to land and the eye to settle,
/// short enough that an empty hero still reads as anticipation rather than as
/// something that failed to load.
const HUB_DELAY: f32 = 1000.0;

/// The whole sequence — long enough to outlast its own last beat, plus a beat of
/// air. This is load-bearing: a one-shot animation clamps at `delta = 1.0` and
/// stops asking for frames, so anything still mid-flight at that instant freezes
/// on screen *permanently*. When `HUB_MS` was 3500 the last three pulses were
/// still travelling at the cutoff and stayed stranded on their streams forever.
const HUB_MS: f32 = HUB_DELAY + PULSE_START + QUEUE_STEP * 7.0 + PULSE_TRAVEL + 100.0;
const HUB_DRAW: Duration = Duration::from_millis(HUB_MS as u64);

pub(super) fn at(ms: f32) -> f32 {
    (HUB_DELAY + ms) / HUB_MS
}

/// Progress of a beat running from `start_ms` to `end_ms`, smoothstepped.
pub(super) fn beat(delta: f32, start_ms: f32, end_ms: f32) -> f32 {
    let t = ((delta - at(start_ms)) / (at(end_ms) - at(start_ms))).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear form — for the pulse, which should travel at a steady clip.
pub(super) fn beat_linear(delta: f32, start_ms: f32, end_ms: f32) -> f32 {
    ((delta - at(start_ms)) / (at(end_ms) - at(start_ms))).clamp(0.0, 1.0)
}

pub(super) fn hub_tools() -> [(lucide_icons::Icon, &'static str); 8] {
    use lucide_icons::Icon as L;
    [
        (L::Bot, "agents"),
        (L::BookOpen, "docs"),
        (L::ListTodo, "tasks"),
        (L::Code, "code"),
        (L::GitBranch, "git"),
        (L::Play, "scripts"),
        (L::Database, "database"),
        (L::Image, "designs"),
    ]
}

/// Each tool keeps the colour it carries everywhere else in the app.
pub(super) fn tool_tint(index: usize, cx: &App) -> gpui::Hsla {
    match index {
        0 | 1 => crate::ui::design::accent(cx),
        2 | 4 => crate::ui::design::amber(cx),
        3 => crate::ui::design::sky(cx),
        5 | 6 => crate::ui::design::sage(cx),
        _ => crate::ui::design::rose(cx),
    }
}

/// Mark-space point into hub space.
pub(super) fn m(p: (f32, f32)) -> (f32, f32) {
    (MARK_OX + p.0 * MARK_SCALE, MARK_OY + p.1 * MARK_SCALE)
}

pub(super) fn cubic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
) -> Vec<(f32, f32)> {
    (0..=SAMPLES)
        .map(|i| {
            let t = i as f32 / SAMPLES as f32;
            let u = 1.0 - t;
            let axis = |a: f32, b: f32, c: f32, d: f32| {
                u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
            };
            (axis(p0.0, p1.0, p2.0, p3.0), axis(p0.1, p1.1, p2.1, p3.1))
        })
        .collect()
}

/// Every polyline in the hero, flattened once and reused for the life of the
/// process. These were being rebuilt inside the paint closure — five fresh
/// allocations per frame, for geometry that never changes.
static HUB_GEOMETRY: std::sync::LazyLock<HubGeometry> = std::sync::LazyLock::new(|| {
    let arc = (0..=SAMPLES)
        .map(|i| {
            let t = i as f32 / SAMPLES as f32;
            // Written the way a hand writes a C: from the upper-right tip,
            // anticlockwise round to the lower-right.
            let a0 = (360.0 - MARK_GAP_DEG).to_radians();
            let a1 = MARK_GAP_DEG.to_radians();
            let a = a0 + (a1 - a0) * t;
            m((MARK_CX + MARK_R * a.cos(), MARK_CY + MARK_R * a.sin()))
        })
        .collect();
    let strands = STRAND_Y.map(|y0| {
        let end = (MARK_NODE_X - MARK_NODE_R, MARK_CY);
        cubic(
            m((STRAND_X0, y0)),
            m((STRAND_X0 + 23.0, y0)),
            m((50.0, MARK_CY)),
            m(end),
        )
    });
    let streams = std::array::from_fn(|i| {
        let y = CHIP_ROWS[i % 4];
        if i < 4 {
            cubic(
                (CHIP_EDGE_L, y),
                (CHIP_EDGE_L + 72.0, y),
                (LAND_L - 56.0, HUB_MID_Y),
                (LAND_L, HUB_MID_Y),
            )
        } else {
            cubic(
                (CHIP_EDGE_R, y),
                (CHIP_EDGE_R - 72.0, y),
                (LAND_R + 56.0, HUB_MID_Y),
                (LAND_R, HUB_MID_Y),
            )
        }
    });
    HubGeometry {
        arc,
        strands,
        streams,
    }
});

struct HubGeometry {
    arc: Vec<(f32, f32)>,
    strands: [Vec<(f32, f32)>; 4],
    streams: [Vec<(f32, f32)>; 8],
}

/// Strokes the slice of `pts` between two fractions of its own arc length. gpui
/// has no `stroke-dasharray`/`dashoffset`, so both the draw-on (`0..t`) and the
/// travelling pulse (`t-len..t`) are the same walk with different bounds.
pub(super) fn draw_range(
    pts: &[(f32, f32)],
    from: f32,
    to: f32,
    width: f32,
    origin: (f32, f32),
    bg: impl Into<gpui::Background>,
    window: &mut Window,
) {
    let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
    if to - from < 1e-3 {
        return;
    }
    let dist = |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let total: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    if total <= 0.0 {
        return;
    }
    let (a, b) = (from * total, to * total);
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let len = dist(w[0], w[1]);
        if len <= f32::EPSILON {
            continue;
        }
        if acc + len >= a && acc <= b {
            let lerp = |t: f32| {
                (
                    w[0].0 + (w[1].0 - w[0].0) * t,
                    w[0].1 + (w[1].1 - w[0].1) * t,
                )
            };
            if out.is_empty() {
                out.push(lerp(((a - acc) / len).clamp(0.0, 1.0)));
            }
            out.push(lerp(((b - acc) / len).clamp(0.0, 1.0)));
        }
        acc += len;
    }
    if out.len() < 2 {
        return;
    }
    let (ox, oy) = origin;
    let mut pb = gpui::PathBuilder::stroke(px(width));
    pb.move_to(gpui::point(px(ox + out[0].0), px(oy + out[0].1)));
    for q in &out[1..] {
        pb.line_to(gpui::point(px(ox + q.0), px(oy + q.1)));
    }
    if let Ok(path) = pb.build() {
        window.paint_path(path, bg);
    }
}

pub(super) fn paint_hub(bounds: Bounds<Pixels>, delta: f32, line: gpui::Hsla, window: &mut Window) {
    let g = &*HUB_GEOMETRY;
    let origin = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    // The C, in the spinner's own gradient — lit from the top-left, deepening away.
    let body = gpui::linear_gradient(
        135.0,
        gpui::linear_color_stop(
            gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_BRIGHT),
            0.0,
        ),
        gpui::linear_color_stop(
            gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_DEEP),
            1.0,
        ),
    );
    draw_range(
        &g.arc,
        0.0,
        beat(delta, 0.0, 1150.0),
        MARK_STROKE * MARK_SCALE,
        origin,
        body,
        window,
    );
    let strand: gpui::Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_TRACK).into();
    for (i, pts) in g.strands.iter().enumerate() {
        let start = 520.0 + i as f32 * 90.0;
        draw_range(
            pts,
            0.0,
            beat(delta, start, start + 1000.0),
            STRAND_W * MARK_SCALE,
            origin,
            strand,
            window,
        );
    }
    // The resting connection, and the one pulse that travels it. The pulse runs
    // past the end and vanishes, leaving the quiet line behind — which is why
    // nothing has to keep moving.
    let pulse: gpui::Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_BRIGHT).into();
    for (i, pts) in g.streams.iter().enumerate() {
        let queue = ARRIVAL[i] as f32;
        let rest = beat(
            delta,
            REST_START + queue * QUEUE_STEP,
            REST_START + REST_FADE + queue * QUEUE_STEP,
        );
        if rest > 0.0 {
            draw_range(
                pts,
                0.0,
                1.0,
                1.5,
                origin,
                line.opacity(0.45 * rest),
                window,
            );
        }
        let travel = beat_linear(
            delta,
            PULSE_START + queue * QUEUE_STEP,
            PULSE_START + PULSE_TRAVEL + queue * QUEUE_STEP,
        );
        if travel > 0.0 && travel < 1.0 {
            let head = travel * (1.0 + PULSE_LEN);
            draw_range(pts, head - PULSE_LEN, head, 2.6, origin, pulse, window);
        }
    }
}

/// One tool chip, faded in on its own beat.
pub(super) fn hub_chip(index: usize, cx: &App) -> Div {
    let (icon, label) = hub_tools()[index];
    let tint = tool_tint(index, cx);
    let surface = crate::ui::design::surface(cx);
    let edge = crate::ui::design::line_2(cx);
    let ink = crate::ui::design::t2(cx);
    let left = index < 4;
    let queue = ARRIVAL[index] as f32;
    let row = CHIP_ROWS[index % 4];
    // gpui has no `translate(-50%, -50%)`, and a chip's width is its text's. So
    // each column is a full-width lane that aligns its chip to the inner edge —
    // which is exactly where that chip's stream begins.
    div()
        .absolute()
        .top(px(row - CHIP_H / 2.0))
        .flex()
        .when(left, |lane| {
            lane.left(px(0.)).w(px(CHIP_EDGE_L)).justify_end()
        })
        .when(!left, |lane| {
            lane.left(px(CHIP_EDGE_R))
                .w(px(HUB_W - CHIP_EDGE_R))
                .justify_start()
        })
        .child(div().with_animation(
            ("onboarding-hub-chip", index),
            Animation::new(HUB_DRAW),
            move |_, delta| {
                let a = beat(
                    delta,
                    CHIP_START + queue * QUEUE_STEP,
                    CHIP_START + CHIP_FADE + queue * QUEUE_STEP,
                );
                h_flex()
                    .h(px(CHIP_H))
                    .items_center()
                    .gap_1p5()
                    .px_2p5()
                    .rounded(crate::ui::design::r_sm())
                    .bg(surface.opacity(a))
                    .border_1()
                    .border_color(edge.opacity(a))
                    .text_size(crate::ui::design::text_label())
                    .text_color(ink.opacity(a))
                    .child(crate::ui::design::indicator::lucide_icon(
                        icon,
                        tint.opacity(a),
                        crate::ui::design::icon_sm(),
                    ))
                    .child(label)
            },
        ))
}

/// The welcome hero: the mark draws itself, then the project streams into it.
pub(super) fn welcome_hub(cx: &App) -> Div {
    let node: gpui::Hsla = gpui::rgb(crate::ui::design::palette::MARK_NODE).into();
    let line = crate::ui::design::t3(cx);
    div()
        .relative()
        .w(px(HUB_W))
        .h(px(HUB_H))
        .flex_none()
        .child(
            canvas(|_, _, _| (), |_, _, _, _| ())
                .absolute()
                .inset_0()
                .with_animation(
                    "onboarding-hub",
                    Animation::new(HUB_DRAW),
                    move |_, delta| {
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| paint_hub(bounds, delta, line, window),
                        )
                        .absolute()
                        .inset_0()
                    },
                ),
        )
        // The node, its halo, and the mark's trailing square land last. Divs
        // rather than paint: a filled circle is just a rounded box.
        .child(div().absolute().with_animation(
            "onboarding-hub-node",
            Animation::new(HUB_DRAW),
            move |_, delta| {
                let a = beat(delta, 1720.0, 2100.0);
                let (nx, ny) = m((MARK_NODE_X, MARK_CY));
                let r = MARK_NODE_R * MARK_SCALE;
                let (sx, sy) = m((MARK_SQUARE_X, MARK_CY));
                let sq = MARK_SQUARE * MARK_SCALE;
                div()
                    .absolute()
                    .child(
                        div()
                            .absolute()
                            .left(px(nx - r * 2.3))
                            .top(px(ny - r * 2.3))
                            .size(px(r * 4.6))
                            .rounded_full()
                            .bg(node.opacity(0.16 * a)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(nx - r))
                            .top(px(ny - r))
                            .size(px(r * 2.0))
                            .rounded_full()
                            .bg(node.opacity(a)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(sx - sq / 2.0))
                            .top(px(sy - sq / 2.0))
                            .size(px(sq))
                            .rounded(px(2.2 * MARK_SCALE))
                            .bg(gpui::Hsla::from(gpui::rgb(
                                crate::ui::design::palette::SPINNER_TRACK,
                            ))
                            .opacity(a)),
                    )
            },
        ))
        .children((0..8).map(|i| hub_chip(i, cx)))
}

/// A stack tool's colour — matched to the same node's colour in the hero
/// constellation, so a tool you pick here glows the same hue when it lights up
/// on the page you build.
pub(super) fn stack_tool_color(tool: StackTool, cx: &App) -> gpui::Hsla {
    use crate::ui::design as d;
    match tool {
        StackTool::Database | StackTool::Editor => d::sky(cx),
        StackTool::GitHub | StackTool::Issues => d::amber(cx),
        StackTool::Docs | StackTool::Agents => d::accent(cx),
        StackTool::Design => d::rose(cx),
        StackTool::Scripts => d::sage(cx),
    }
}

/// The real brand mark where the app ships one — GitHub's octocat, Claude's
/// spark, Figma's, Linear's, and Postgres from the bundled Devicon font — and a
/// clean Lucide glyph for the categories that have no single logo (Docs, Editor,
/// Scripts). All tint to the passed colour, so the grid still reads as one set.
pub(super) fn stack_tool_mark(
    tool: StackTool,
    color: gpui::Hsla,
    size: Pixels,
) -> gpui::AnyElement {
    let svg = |path: &'static str| {
        gpui_component::Icon::empty()
            .path(path)
            .size(size)
            .text_color(color)
            .into_any_element()
    };
    match tool {
        StackTool::GitHub => Icon::new(IconName::GitHub)
            .size(size)
            .text_color(color)
            .into_any_element(),
        StackTool::Agents => svg("agent-icons/claude.svg"),
        StackTool::Issues => svg("brand/jira.svg"),
        // The Postgres elephant from the Devicon databases subset — the same
        // font the DB panel renders provider marks with.
        StackTool::Database => div()
            .font_family(crate::theme::DEVICON_FONT_FAMILY)
            .text_size(size)
            .line_height(gpui::relative(1.))
            .text_color(color)
            .child('\u{eaf5}'.to_string())
            .into_any_element(),
        _ => {
            crate::ui::design::indicator::lucide_icon(tool.glyph(), color, size).into_any_element()
        }
    }
}

/// The instruction card's glyph plate — the medallion's small sibling, same
/// accent-tint language, carrying the face of the step you're on.
pub(super) fn step_plate(icon: lucide_icons::Icon, cx: &App) -> Div {
    div()
        .flex_none()
        .size(px(PLATE))
        .rounded(crate::ui::design::r_sm())
        .flex()
        .items_center()
        .justify_center()
        .bg(crate::ui::design::accent(cx).opacity(0.12))
        .border_1()
        .border_color(crate::ui::design::accent(cx).opacity(0.22))
        .child(crate::ui::design::indicator::lucide_icon(
            icon,
            crate::ui::design::accent(cx),
            crate::ui::design::icon_sm(),
        ))
}

/// The plate wash behind a provider's brand mark. Claude ships a colored glyph,
/// so its plate carries the same orange; the OpenAI and OpenCode marks are
/// monochrome and inherit `t1`, so theirs is a neutral wash of the same ink
/// rather than an invented brand color.
pub(super) fn provider_tint(provider: ide_core::AgentKind, cx: &App) -> gpui::Hsla {
    match provider {
        ide_core::AgentKind::Claude => crate::ui::design::palette::claude_brand(),
        ide_core::AgentKind::Codex | ide_core::AgentKind::OpenCode => crate::ui::design::t1(cx),
    }
}
