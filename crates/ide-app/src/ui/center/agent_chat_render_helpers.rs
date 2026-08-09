use super::*;

pub(super) fn work_log_display_detail(
    entry: &crate::state::agent_chat::WorkLogEntry,
) -> Option<String> {
    entry
        .detail
        .as_deref()
        .filter(|detail| !detail.trim().is_empty())
        .or_else(|| {
            let title = entry.title.as_str();
            work_log_text_needs_collapse(title).then_some(title)
        })
        .map(str::to_string)
}

pub(super) fn work_log_text_needs_collapse(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.lines().count() > 1
        || trimmed.len() > 140
        || trimmed.contains("<<")
        || trimmed.contains("node -")
        || trimmed.contains("/bin/")
        || trimmed.contains("python -")
}

pub(super) fn compact_work_log_title(entry: &crate::state::agent_chat::WorkLogEntry) -> String {
    let title = entry.title.trim();
    if title.is_empty() {
        return "Tool call".to_string();
    }

    if !work_log_text_needs_collapse(title) {
        return title.to_string();
    }

    if title.contains("node -") || title.contains("NODE") {
        return "Command · node script".to_string();
    }
    if title.contains("python -") || title.contains("PY") {
        return "Command · python script".to_string();
    }
    if title.contains("/bin/zsh") || title.contains("/bin/bash") || title.contains("/bin/sh") {
        return "Command · shell script".to_string();
    }

    let first_line = title.lines().next().unwrap_or("Tool call").trim();
    let mut compact = first_line.chars().take(96).collect::<String>();
    if first_line.chars().count() > compact.chars().count() {
        compact.push('…');
    }
    compact
}

fn is_legacy_code_review_request(text: &str) -> bool {
    text.strip_prefix(ide_core::config::DEFAULT_CODE_REVIEW_PROMPT)
        .and_then(|rest| rest.strip_prefix("\n\n"))
        == Some(ide_core::config::DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS)
}

/// Recognise current and historical canned code-review and verification turns
/// so the composer's giant instruction prompt renders as a compact chip
/// instead of a wall of text.
pub(super) fn code_review_request_chip(text: &str) -> Option<(&'static str, IconName)> {
    if text == super::agent_chat_runtime::AGENT_CODE_REVIEW_PROMPT
        || text.starts_with(super::agent_chat_runtime::AGENT_CODE_REVIEW_REQUEST_MARKER)
        || is_legacy_code_review_request(text)
    {
        Some(("Sent for code review", IconName::Inspector))
    } else if text.starts_with(super::agent_chat_runtime::AGENT_CODE_REVIEW_FIX_PREFIX) {
        Some(("Applying review fixes", IconName::CircleCheck))
    } else if text.starts_with(super::agent_chat_runtime::AGENT_VERIFY_REQUEST_MARKER) {
        Some(("Sent for verification", IconName::CircleCheck))
    } else if text.starts_with(super::agent_chat_runtime::AGENT_VERIFY_FIX_PREFIX) {
        Some(("Addressing verification gaps", IconName::Replace))
    } else if text.starts_with(super::agent_chat_runtime::AGENT_REVERIFY_DISMISS_MARKER) {
        Some(("Re-verification skipped", IconName::CircleX))
    } else {
        None
    }
}

/// The subtle path shown beside a changed file's name: the last few directory
/// segments above it (e.g. `client/src/hooks`), never the full absolute path.
pub(super) fn changed_file_dir_label(path: &std::path::Path) -> Option<String> {
    const MAX_SEGMENTS: usize = 3;
    let parent = path.parent()?;
    let segments: Vec<String> = parent
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if segments.is_empty() {
        return None;
    }
    let start = segments.len().saturating_sub(MAX_SEGMENTS);
    Some(segments[start..].join("/"))
}

/// Stable element-id key for a review card's buttons (unique per agent+review,
/// so several review cards in one chat don't collide).
pub(super) fn code_review_card_key(agent_id: Uuid, review_id: &str) -> u64 {
    let key = fnv_mix_u64(0xcbf29ce484222325u64, agent_id.as_u128() as u64);
    fnv_mix_str(key, review_id)
}

/// Stable element-id key for a single finding's checkbox.
pub(super) fn code_review_finding_key(agent_id: Uuid, review_id: &str, index: usize) -> u64 {
    fnv_mix_u64(code_review_card_key(agent_id, review_id), index as u64)
}

/// Stable element-id key for a verification card's buttons (unique per
/// agent+verification, so several cards in one chat don't collide).
pub(super) fn verification_card_key(agent_id: Uuid, verification_id: &str) -> u64 {
    let key = fnv_mix_u64(0x100000001b3u64, agent_id.as_u128() as u64);
    fnv_mix_str(key, verification_id)
}

pub(super) fn stable_text_key(text: &str) -> u64 {
    fnv_mix_str(0xcbf29ce484222325u64, text)
}

/// Severity → accent colour for finding badges and tags.
pub(super) fn code_review_severity_color(
    severity: crate::state::agent_chat::CodeReviewSeverity,
    cx: &App,
) -> gpui::Hsla {
    use crate::state::agent_chat::CodeReviewSeverity::*;
    match severity {
        Critical | High => crate::ui::design::rose(cx),
        Medium => crate::ui::design::amber(cx),
        Low | Info => crate::ui::design::t3(cx),
    }
}

pub(super) fn agent_chat_message_render_key(
    agent_id: Uuid,
    index: usize,
    message: &AgentChatMessage,
) -> u64 {
    let mut key = 0xcbf29ce484222325u64;
    key = fnv_mix_u64(key, agent_id.as_u128() as u64);
    key = fnv_mix_u64(key, (agent_id.as_u128() >> 64) as u64);
    key = fnv_mix_u64(key, index as u64);

    match message {
        AgentChatMessage::User {
            text, created_at, ..
        } => {
            key = fnv_mix_u64(key, 1);
            key = fnv_mix_u64(key, *created_at);
            fnv_mix_str(key, text)
        }
        AgentChatMessage::Assistant {
            message_id,
            text,
            created_at,
        } => {
            key = fnv_mix_u64(key, 2);
            key = fnv_mix_u64(key, *created_at);
            if let Some(message_id) = message_id {
                fnv_mix_str(key, message_id)
            } else {
                fnv_mix_str(key, text)
            }
        }
        AgentChatMessage::Thought {
            message_id,
            text,
            created_at,
        } => {
            key = fnv_mix_u64(key, 3);
            key = fnv_mix_u64(key, *created_at);
            if let Some(message_id) = message_id {
                fnv_mix_str(key, message_id)
            } else {
                fnv_mix_str(key, text)
            }
        }
    }
}

pub(super) fn resume_loader_step(
    label: &'static str,
    active: bool,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let color = if active {
        crate::ui::design::accent(cx)
    } else {
        crate::ui::design::t3(cx)
    };
    h_flex()
        .h(crate::ui::design::control_h_xs())
        .gap_1p5()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(if active {
            crate::ui::design::accent(cx).opacity(0.26)
        } else {
            style::border(cx).opacity(0.7)
        })
        .bg(if active {
            crate::ui::design::accent(cx).opacity(0.1)
        } else {
            crate::ui::design::surface(cx)
        })
        .px_2p5()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(color)
        .child(
            gpui_component::Icon::new(if active {
                IconName::CircleCheck
            } else {
                IconName::LoaderCircle
            })
            .size(crate::ui::design::icon_sm())
            .text_color(color),
        )
        .child(label)
        .into_any_element()
}

pub(super) fn fnv_mix_u64(mut key: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        key ^= byte as u64;
        key = key.wrapping_mul(0x100000001b3);
    }
    key
}

pub(super) fn fnv_mix_str(mut key: u64, value: &str) -> u64 {
    for byte in value.as_bytes() {
        key ^= *byte as u64;
        key = key.wrapping_mul(0x100000001b3);
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_code_review_action_still_renders_as_a_chip() {
        let legacy = format!(
            "{}\n\n{}",
            ide_core::config::DEFAULT_CODE_REVIEW_PROMPT,
            ide_core::config::DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS,
        );
        assert_eq!(
            code_review_request_chip(&legacy).map(|(label, _)| label),
            Some("Sent for code review")
        );
    }

    #[test]
    fn ordinary_user_message_is_not_mistaken_for_a_review_request() {
        assert!(code_review_request_chip("Please review this code").is_none());
    }

    #[test]
    fn declined_reverification_renders_as_a_compact_chip() {
        assert_eq!(
            code_review_request_chip(
                super::super::agent_chat_runtime::AGENT_REVERIFY_DISMISS_MARKER
            )
            .map(|(label, _)| label),
            Some("Re-verification skipped")
        );
    }
}
