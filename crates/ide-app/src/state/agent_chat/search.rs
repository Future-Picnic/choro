use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use unicase::UniCase;

use super::{
    split_code_review, split_review_checklist, split_verification, AgentChatMessage,
    REVIEW_CHECKLIST_REQUEST_MARKER,
};

pub(crate) const TIMELINE_SEARCH_TEXT_VERSION: u64 = 1;

const SUMMARY_REQUEST_MARKER: &str = "[Choro Brain summary checkpoint]";
const BACKGROUND_SUMMARY_REQUEST_MARKER: &str = "<!-- choro:background-summary-maintenance -->";
const TEAMMATE_RESULT_MARKER: &str = "<!-- choro:teammate-result -->";
const AGENT_REQUEST_MARKER: &str = "<choro-agent-request>\nRequest id:";
const CODE_REVIEW_REQUEST_MARKER: &str = "<!-- choro:code-review -->";
const CODE_REVIEW_FIX_PREFIX: &str = "Apply the fixes for these code-review findings";
const VERIFY_REQUEST_MARKER: &str = "<!-- choro:verify -->";
const POCKETCOMET_HANDOFF_REQUEST_MARKER: &str = "<!-- choro:pocketcomet-handoff -->";
const VERIFY_FIX_PREFIX: &str = "Address these unmet requirements from the verification";
const VERIFY_DISMISS_MARKER: &str = "<!-- choro:verify-dismissed -->";
const REVERIFY_DISMISS_MARKER: &str = "<!-- choro:reverify-dismissed -->";

const STRIPPED_CONTEXT_TAGS: &[(&str, &str)] = &[
    ("<choro-orbit-context>", "</choro-orbit-context>"),
    ("<choro-riff-context>", "</choro-riff-context>"),
    ("<choro-preview-context>", "</choro-preview-context>"),
    ("<choro-memory-context>", "</choro-memory-context>"),
    (
        "<choro-memory-save-context>",
        "</choro-memory-save-context>",
    ),
    (
        "<choro-rejoin-conflict-context>",
        "</choro-rejoin-conflict-context>",
    ),
    ("<choro-project-context>", "</choro-project-context>"),
];

pub(crate) fn fold_search_text(text: &str) -> String {
    UniCase::new(text).to_folded_case()
}

pub(crate) fn search_turn_is_hidden(user_text: &str) -> bool {
    user_text.starts_with(REVIEW_CHECKLIST_REQUEST_MARKER)
        || (user_text.starts_with(SUMMARY_REQUEST_MARKER)
            && user_text.contains(BACKGROUND_SUMMARY_REQUEST_MARKER))
}

pub(crate) fn searchable_message_text(message: &AgentChatMessage) -> Option<String> {
    let text = match message {
        AgentChatMessage::User {
            text, display_text, ..
        } => searchable_user_message_text(text, display_text.as_deref())?,
        AgentChatMessage::Assistant { text, .. } => searchable_assistant_message_text(text),
        AgentChatMessage::Thought { .. } => return None,
    };
    (!text.is_empty()).then_some(text)
}

fn searchable_user_message_text(text: &str, display_text: Option<&str>) -> Option<String> {
    if search_turn_is_hidden(text)
        || text.contains(AGENT_REQUEST_MARKER)
        || text.contains(TEAMMATE_RESULT_MARKER)
    {
        return None;
    }

    if text.starts_with(SUMMARY_REQUEST_MARKER) {
        return Some("Brain summary requested".to_string());
    }
    if text == ide_core::config::DEFAULT_CODE_REVIEW_PROMPT
        || text.starts_with(CODE_REVIEW_REQUEST_MARKER)
        || is_legacy_code_review_request(text)
    {
        return Some("Sent for code review".to_string());
    }
    if text.starts_with(CODE_REVIEW_FIX_PREFIX) {
        return Some("Applying review fixes".to_string());
    }
    if text.starts_with(VERIFY_REQUEST_MARKER) {
        return Some("Sent for verification".to_string());
    }
    if text.starts_with(POCKETCOMET_HANDOFF_REQUEST_MARKER) {
        return Some("Preparing PocketComet update".to_string());
    }
    if text.starts_with(VERIFY_FIX_PREFIX) {
        return Some("Addressing verification gaps".to_string());
    }
    if text.starts_with(VERIFY_DISMISS_MARKER) || text.starts_with(REVERIFY_DISMISS_MARKER) {
        return Some("Verification skipped".to_string());
    }

    let visible = display_text.unwrap_or_else(|| visible_submission_text(text));
    Some(markdown_visible_text(strip_attachment_block(visible)))
}

fn searchable_assistant_message_text(text: &str) -> String {
    let (text, _) = split_code_review(text);
    let (text, _) = split_verification(&text);
    let (text, _) = split_review_checklist(&text);
    let visible = text
        .lines()
        .filter(|line| !line.trim_start().starts_with("::codex-inline-vis{"))
        .collect::<Vec<_>>()
        .join("\n");
    markdown_visible_text(&visible)
}

fn is_legacy_code_review_request(text: &str) -> bool {
    text.strip_prefix(ide_core::config::DEFAULT_CODE_REVIEW_PROMPT)
        .and_then(|rest| rest.strip_prefix("\n\n"))
        == Some(ide_core::config::DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS)
}

fn visible_submission_text(text: &str) -> &str {
    let mut visible = text;
    loop {
        let Some((_, closing)) = STRIPPED_CONTEXT_TAGS
            .iter()
            .find(|(opening, _)| visible.starts_with(opening))
        else {
            break;
        };
        let Some(end) = visible.find(closing) else {
            break;
        };
        visible = visible[end + closing.len()..].trim_start_matches(['\r', '\n']);
    }
    visible
}

fn strip_attachment_block(text: &str) -> &str {
    for marker in ["\n\nAttached files:\n", "\n\nAttached images:\n"] {
        if let Some((body, attachment_block)) = text.rsplit_once(marker) {
            if attachment_block
                .lines()
                .any(|line| line.trim().starts_with("- "))
            {
                return body.trim_end();
            }
        }
    }
    text
}

fn markdown_visible_text(markdown: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut plain = String::new();
    let mut image_depth = 0usize;

    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth = image_depth.saturating_sub(1),
            Event::Text(text)
            | Event::Code(text)
            | Event::InlineMath(text)
            | Event::DisplayMath(text)
                if image_depth == 0 =>
            {
                plain.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak => plain.push(' '),
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::Item
                | TagEnd::TableCell
                | TagEnd::TableRow,
            ) => plain.push(' '),
            _ => {}
        }
    }

    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searchable_text_matches_rendered_markdown_instead_of_source_syntax() {
        let message = AgentChatMessage::Assistant {
            message_id: Some("turn-1".into()),
            text: "Use **fast search** with [details](https://hidden.test) and ![plot](plot.png)."
                .into(),
            created_at: 1,
        };

        assert_eq!(
            searchable_message_text(&message).as_deref(),
            Some("Use fast search with details and .")
        );
    }

    #[test]
    fn searchable_text_omits_hidden_structured_and_maintenance_content() {
        let assistant = AgentChatMessage::Assistant {
            message_id: Some("turn-2".into()),
            text: "Visible answer.\n\n<code_review>secret finding</code_review>".into(),
            created_at: 2,
        };
        let maintenance = AgentChatMessage::User {
            text: format!("{REVIEW_CHECKLIST_REQUEST_MARKER}\nSource turn: 1"),
            display_text: None,
            tags: Vec::new(),
            created_at: 3,
        };

        assert_eq!(
            searchable_message_text(&assistant).as_deref(),
            Some("Visible answer.")
        );
        assert!(searchable_message_text(&maintenance).is_none());
    }

    #[test]
    fn unicode_fold_uses_full_case_folding() {
        assert_eq!(
            fold_search_text("Maße ÉLAN"),
            fold_search_text("MASSE élan")
        );
    }
}
