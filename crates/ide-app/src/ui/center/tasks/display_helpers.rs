use super::*;

/// How many description lines to show before "Show more".
pub(super) const TASK_DESC_PREVIEW_LINES: usize = 8;

/// Turn a raw provider timestamp (ISO-8601 or epoch-millis) into a friendly
/// relative label ("2h ago", "3d ago", "Jul 5, 2026"). Falls back to the raw
/// string if it can't be parsed.
pub(super) fn format_timestamp(raw: &str) -> String {
    use chrono::{DateTime, Local, TimeZone, Utc};

    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let parsed: Option<DateTime<Utc>> = if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        trimmed
            .parse::<i64>()
            .ok()
            .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
    } else {
        // Jira returns a non-colon offset ("+0300") that isn't strict RFC-3339,
        // so fall back to a %z parse before giving up.
        DateTime::parse_from_rfc3339(trimmed)
            .or_else(|_| DateTime::parse_from_str(trimmed, "%Y-%m-%dT%H:%M:%S%.f%z"))
            .ok()
            .map(|dt| dt.with_timezone(&Utc))
    };
    let Some(dt) = parsed else {
        return trimmed.to_string();
    };
    let delta = Utc::now().signed_duration_since(dt);
    let seconds = delta.num_seconds();
    if seconds < 45 {
        return "just now".to_string();
    }
    let minutes = delta.num_minutes();
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = delta.num_hours();
    if hours < 24 {
        return format!("{hours}h ago");
    }
    let days = delta.num_days();
    if days < 7 {
        return format!("{days}d ago");
    }
    dt.with_timezone(&Local).format("%b %-d, %Y").to_string()
}

/// Whether a provider-supplied metadata value carries information. Trackers
/// spell an empty field as a word ("No priority", "None", "-") rather than
/// omitting it; those are nulls, and the header renders nothing for a null.
pub(super) fn is_meaningful_value(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "-"
            | "—"
            | "n/a"
            | "na"
            | "none"
            | "null"
            | "unset"
            | "not set"
            | "no priority"
            | "unassigned"
            | "unspecified"
    )
}

/// The glyph for an issue type. The icon carries the category, so the header
/// never has to spend a word on the label "Type".
pub(super) fn task_type_icon(issue_type: &str) -> lucide_icons::Icon {
    let value = issue_type.to_ascii_lowercase();
    if value.contains("bug") || value.contains("defect") {
        lucide_icons::Icon::Bug
    } else if value.contains("epic") {
        lucide_icons::Icon::Layers
    } else if value.contains("story") || value.contains("feature") {
        lucide_icons::Icon::Sparkles
    } else if value.contains("issue") {
        lucide_icons::Icon::CircleDot
    } else {
        lucide_icons::Icon::CheckSquare
    }
}

pub(super) fn priority_color(priority: &str, cx: &App) -> gpui::Hsla {
    let value = priority.to_ascii_lowercase();
    if value.contains("high") || value.contains("urgent") || value.contains("critical") {
        crate::ui::design::rose(cx)
    } else if value.contains("medium") || value.contains("normal") {
        crate::ui::design::amber(cx)
    } else {
        crate::ui::design::t3(cx)
    }
}

pub(super) fn priority_pill(priority: &str, cx: &mut Context<CenterArea>) -> gpui::AnyElement {
    // A bare indicator: the priority colour reads as the indicator itself, no
    // pill fill or border (Linear-style), matching the design system's law.
    div()
        .flex_none()
        .text_size(crate::ui::design::text_ui())
        .text_color(priority_color(priority, cx))
        .child(SharedString::from(priority.to_string()))
        .into_any_element()
}

pub(super) fn avatar_badge(name: &str) -> gpui::AnyElement {
    avatar_badge_sized(name, px(22.), crate::ui::design::text_ui())
}

/// The header-subline avatar. Sized to sit on the indicator row's 12.5px
/// baseline instead of stretching it to the comment-list avatar's height.
pub(super) fn avatar_badge_sm(name: &str) -> gpui::AnyElement {
    avatar_badge_sized(name, px(18.), crate::ui::design::text_label())
}

fn avatar_badge_sized(name: &str, size: gpui::Pixels, text_size: gpui::Pixels) -> gpui::AnyElement {
    let initials = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    let initials = if initials.is_empty() {
        "?".to_string()
    } else {
        initials
    };
    let idx = name.bytes().map(usize::from).sum::<usize>();
    let bg = crate::ui::design::palette::task_avatar(idx);
    div()
        .flex_none()
        .size(size)
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .text_color(crate::ui::design::palette::ink_on_dark())
        .text_size(text_size)
        .child(SharedString::from(initials))
        .into_any_element()
}

pub(super) fn personal_status_color(status: ide_core::PersonalTaskStatus, cx: &App) -> gpui::Hsla {
    match status {
        ide_core::PersonalTaskStatus::Todo => crate::ui::design::amber(cx),
        ide_core::PersonalTaskStatus::InProgress => crate::ui::design::sky(cx),
        ide_core::PersonalTaskStatus::Done => crate::ui::design::sage(cx),
    }
}

pub(super) fn task_status_menu_row(name: &str, is_current: bool, cx: &mut App) -> gpui::AnyElement {
    let dot = if is_current {
        crate::ui::design::accent(cx)
    } else {
        crate::ui::design::t3(cx).opacity(0.6)
    };
    h_flex()
        .w_full()
        .min_w(px(160.))
        .items_center()
        .gap_2()
        .child(div().size(px(6.)).rounded_full().bg(dot))
        .child(
            div()
                .flex_1()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t1(cx))
                .child(SharedString::from(name.to_string())),
        )
        .into_any_element()
}

/// The read-only status badge, used when no board statuses are known to offer.
pub(super) fn task_status_static_badge(
    label: &str,
    color: gpui::Hsla,
    error: Option<String>,
    cx: &App,
) -> gpui::AnyElement {
    let badge =
        crate::ui::design::indicator::status(SharedString::from(label.to_string()), color, cx)
            .h(crate::ui::design::control_h())
            .px_2();
    match error {
        Some(error) => v_flex()
            .gap_1()
            .child(badge)
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::rose(cx))
                    .child(SharedString::from(error)),
            )
            .into_any_element(),
        None => badge.into_any_element(),
    }
}

pub(super) fn personal_status_menu_row(
    status: ide_core::PersonalTaskStatus,
    cx: &mut App,
) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .min_w(px(140.))
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(personal_status_color(status, cx)),
        )
        .child(
            div()
                .flex_1()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t1(cx))
                .child(status.label()),
        )
        .into_any_element()
}

pub(super) fn personal_priority_menu_row(
    priority: ide_core::PersonalTaskPriority,
    cx: &mut App,
) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .min_w(px(140.))
        .items_center()
        .gap_2()
        .child(
            div()
                .size(px(6.))
                .rounded_full()
                .bg(priority_color(priority.label(), cx)),
        )
        .child(
            div()
                .flex_1()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t1(cx))
                .child(priority.label()),
        )
        .into_any_element()
}
