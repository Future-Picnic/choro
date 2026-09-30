//! Shared building blocks for the ⌘K / ⌘P / ⇧⌘F palette dialogs.
//!
//! The palette look is fill-first: the dialog is the `focus` plane behind a
//! neutral hairline and the one soft shadow, rows separate by spacing and tiny
//! group labels rather than dividers, and the accent never outlines a box — it
//! lives in the ink (the matched query letters, the selected row's glyph).
//!
//! Every dialog shares the same rhythm: a tall header band, a padded list of
//! 36px rows, and a quiet footer of keycap hints. The numbers live here so the
//! three dialogs can't drift apart.

use std::ops::Range;

use std::cell::Cell;

use gpui::{
    div, hsla, prelude::FluentBuilder, px, App, Div, HighlightStyle, Hsla, InteractiveElement,
    IntoElement, ParentElement, SharedString, Styled, Window,
};
use gpui_component::{dialog::Dialog, h_flex, v_flex, Theme, WindowExt};

use crate::ui::design;

/// 720px — the ⌘P / ⌘K dialog width.
pub const DIALOG_W: f32 = 720.0;
/// 820px — the ⇧⌘F dialog width (previews want the extra room).
pub const DIALOG_W_WIDE: f32 = 820.0;
/// 84px — distance from the window top to the dialog.
pub const DIALOG_TOP: f32 = 84.0;
/// 50px — the header band: input row with breathing room above and below.
pub const HEADER_H: f32 = 50.0;
/// 40px — the footer band carrying the keycap hints.
pub const FOOTER_H: f32 = 40.0;
/// 14px — header/footer horizontal inset.
pub const PAD_X: f32 = 14.0;
/// 6px — the gutter around the results list.
pub const LIST_PAD: f32 = 6.0;
/// 10px — extra air under the last row, before the footer band.
pub const LIST_PAD_BOTTOM: f32 = 10.0;
/// 36px — one result row.
pub const ROW_H: f32 = 36.0;
/// 10px — row horizontal inset; group labels share it so text columns align.
pub const ROW_PAD_X: f32 = 10.0;
/// 10px — icon-to-title rhythm inside a row.
pub const ROW_GAP: f32 = 10.0;
/// The palette scrim: a whisper of dim, nowhere near the stock overlay's
/// full-modal blackout, so the app stays readable while the palette leads.
pub const SCRIM_ALPHA: f32 = 0.15;

/// The shared palette dialog chrome, applied to the stock dialog panel
/// itself: the `focus` plane behind a neutral hairline, `r_lg` corners, no
/// stock title or close button. Styling the panel (rather than an inner box)
/// keeps fill, border, and radius on one rect — a nested frame left a sliver
/// of the panel's own darker background visible inside the border.
pub fn styled_dialog(dialog: Dialog, width: f32, cx: &App) -> Dialog {
    dialog
        .w(px(width))
        .margin_top(px(DIALOG_TOP))
        .overlay(true)
        .on_close(|_, _, cx| restore_overlay(cx))
        .close_button(false)
        .keyboard(true)
        .p_0()
        .bg(design::focus(cx))
        .border_color(design::line(cx))
        .rounded(design::r_lg())
}

/// The palette content column inside [`styled_dialog`] — the panel carries
/// the visual frame; this only sets the text tier.
pub fn frame(cx: &App) -> Div {
    v_flex().w_full().text_color(design::t1(cx))
}

thread_local! {
    /// The theme's own overlay color, stashed while a palette has softened it.
    static STASHED_OVERLAY: Cell<Option<Hsla>> = const { Cell::new(None) };
}

/// Swap the global dialog overlay for the palette's whisper of dim. The stock
/// value is the full-attention modal blackout; a palette wants focus without
/// taking the app away. Every close path must put it back: pass
/// `.on_close(|_, _, cx| palette_ui::restore_overlay(cx))` to the dialog
/// (covers esc and click-away) and use [`close`] instead of `close_dialog`
/// when a row is confirmed.
pub fn soften_overlay(cx: &mut App) {
    let theme = Theme::global_mut(cx);
    STASHED_OVERLAY.with(|stash| {
        if stash.get().is_none() {
            stash.set(Some(theme.overlay));
        }
    });
    theme.overlay = hsla(0., 0., 0., SCRIM_ALPHA);
}

/// Put the theme's own overlay back for the true modals.
pub fn restore_overlay(cx: &mut App) {
    if let Some(original) = STASHED_OVERLAY.with(|stash| stash.take()) {
        Theme::global_mut(cx).overlay = original;
    }
}

/// Close a palette dialog, restoring the modal overlay first.
pub fn close(window: &mut Window, cx: &mut App) {
    restore_overlay(cx);
    window.close_dialog(cx);
}

/// The header band — no divider below; the list separates by spacing.
pub fn header_band() -> Div {
    h_flex()
        .flex_none()
        .h(px(HEADER_H))
        .w_full()
        .px(px(PAD_X))
        .gap(px(ROW_GAP))
        .items_center()
}

/// The footer band — no divider above.
pub fn footer_band() -> Div {
    h_flex()
        .flex_none()
        .h(px(FOOTER_H))
        .w_full()
        .px(px(PAD_X))
        .gap_3()
        .items_center()
}

/// One result row: strokeless, selection is a plane-aware fill step and hover
/// is the quiet table wash. Callers add `.id(..)`, `.on_click(..)`, children.
/// `flex_none` is load-bearing: without it an overflowing scroll list shrinks
/// every row and the 36px rhythm collapses.
pub fn row(selected: bool, cx: &App) -> Div {
    h_flex()
        .flex_none()
        .h(px(ROW_H))
        .w_full()
        .px(px(ROW_PAD_X))
        .gap(px(ROW_GAP))
        .items_center()
        .rounded(design::r_sm())
        .cursor_pointer()
        .when(selected, |row| {
            row.bg(design::control_on(design::focus(cx), cx))
        })
        .when(!selected, |row| row.hover(|row| row.bg(design::hover(cx))))
}

/// Byte ranges of `query` inside `text`, case-insensitive. Prefers the
/// contiguous hit; otherwise falls back to the greedy character subsequence
/// the fuzzy filters accept. Empty when the text itself doesn't match — a row
/// can still be listed via its description, category, or path.
pub fn match_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }

    if text.is_ascii() && query.is_ascii() {
        if let Some(start) = text.to_ascii_lowercase().find(&query.to_ascii_lowercase()) {
            return vec![start..start + query.len()];
        }
    }

    let needle: Vec<char> = query.to_lowercase().chars().collect();
    let mut matched = 0;
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (ix, ch) in text.char_indices() {
        if matched == needle.len() {
            break;
        }
        let lowered = ch.to_lowercase().next().unwrap_or(ch);
        if lowered != needle[matched] {
            continue;
        }
        let end = ix + ch.len_utf8();
        match ranges.last_mut() {
            Some(last) if last.end == ix => last.end = end,
            _ => ranges.push(ix..end),
        }
        matched += 1;
    }

    if matched == needle.len() {
        ranges
    } else {
        Vec::new()
    }
}

/// Text whose given byte ranges carry the accent ink — the accent tracks the
/// match instead of outlining the row. Out-of-bounds ranges are clamped.
pub fn highlighted_ranges(
    text: impl Into<SharedString>,
    ranges: impl IntoIterator<Item = Range<usize>>,
    cx: &App,
) -> gpui::StyledText {
    let text = text.into();
    let ink = design::accent_ink(design::focus(cx), cx);
    let len = text.len();
    let highlights: Vec<(Range<usize>, HighlightStyle)> = ranges
        .into_iter()
        .map(|range| range.start.min(len)..range.end.min(len))
        .filter(|range| range.start < range.end)
        .map(|range| {
            (
                range,
                HighlightStyle {
                    color: Some(ink),
                    ..Default::default()
                },
            )
        })
        .collect();
    gpui::StyledText::new(text).with_highlights(highlights)
}

/// A row title whose query-matched letters carry the accent ink.
pub fn highlighted_title(
    title: impl Into<SharedString>,
    query: &str,
    cx: &App,
) -> gpui::StyledText {
    let title = title.into();
    let ranges = match_ranges(title.as_ref(), query);
    highlighted_ranges(title, ranges, cx)
}

/// Tiny muted section label; groups separate by spacing, never by a divider.
pub fn group_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .flex_none()
        .px(px(ROW_PAD_X))
        .pt(px(10.))
        .pb(px(3.))
        .text_size(design::text_label())
        .text_color(design::t4(cx))
        .child(text.into())
}

/// A sunken keycap well — the palette's one inset element. Buttons lift;
/// a keycap is a thing you press *into*, so it sinks to the `base` plane.
pub fn keycap(label: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_1p5()
        .py_0p5()
        .rounded(design::r_xs())
        .bg(design::base(cx))
        .font_family(design::FONT_MONO)
        .text_size(design::text_label())
        .text_color(design::t2(cx))
        .child(label.into())
}

/// Footer hint — a keycap and its muted verb ("↩ run").
pub fn key_hint(key: &str, label: &'static str, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(keycap(key.to_string(), cx))
        .child(
            div()
                .text_size(design::text_ui())
                .text_color(design::t3(cx))
                .child(label),
        )
}
