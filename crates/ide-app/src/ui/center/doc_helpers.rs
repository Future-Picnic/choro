#![allow(dead_code, reason = "retained document-assistant helpers")]

use super::*;

pub(super) fn doc_label_accent(label: &str) -> gpui::Hsla {
    crate::ui::design::palette::doc_label(label)
}

pub(super) fn doc_label_dot(label: &str, size: f32) -> gpui::AnyElement {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(doc_label_accent(label))
        .into_any_element()
}

pub(super) fn doc_label_menu_row(label: &str, cx: &mut App) -> gpui::AnyElement {
    let accent = doc_label_accent(label);
    h_flex()
        .w_full()
        .min_w(px(148.))
        .items_center()
        .gap_2()
        .child(doc_label_dot(label, 7.))
        .child(
            div()
                .flex_1()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t1(cx))
                .child(label.to_string()),
        )
        .child(
            div()
                .h(px(1.))
                .w(px(18.))
                .rounded_full()
                .bg(accent.opacity(0.28)),
        )
        .into_any_element()
}

pub(super) fn short_doc_chip_label(path: &Path) -> String {
    let name = if let Some(stem) = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
    {
        stem.to_string()
    } else {
        path.to_string_lossy().to_string()
    };
    let mut chars = name.chars();
    let prefix = chars.by_ref().take(10).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}...")
    } else {
        prefix
    }
}

pub(super) fn latest_doc_proposal(messages: &[DocAssistantMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter(|message| matches!(message.role, DocAssistantRole::Assistant))
        .find_map(|message| {
            doc_assistant::extract_doc_proposal(&message.text)
                .ok()
                .flatten()
        })
}

#[derive(Clone)]
pub(super) struct DocAssistantDisplayMessage {
    pub(super) role: DocAssistantRole,
    pub(super) text: String,
    pub(super) pending: bool,
}

pub(super) fn doc_assistant_terminal_submit(prompt: &str) -> String {
    // Ctrl-U clears stale text that may have been pasted into the CLI prompt
    // before submitting with carriage return, which interactive TUIs treat as Enter.
    format!("\u{15}{prompt}\r")
}

pub(super) fn doc_assistant_display_text(message: &DocAssistantMessage) -> Option<String> {
    let text = match message.role {
        DocAssistantRole::User => doc_assistant_user_display_text(&message.text),
        DocAssistantRole::Assistant => doc_assistant_assistant_display_text(&message.text),
    };
    (!text.trim().is_empty()).then(|| text)
}

pub(super) fn doc_assistant_display_text_from_agent_message(
    message: &AgentChatMessage,
) -> Option<DocAssistantDisplayMessage> {
    match message {
        AgentChatMessage::User { text, .. } => {
            let text = doc_assistant_user_display_text(text);
            (!text.trim().is_empty()).then_some(DocAssistantDisplayMessage {
                role: DocAssistantRole::User,
                text,
                pending: false,
            })
        }
        AgentChatMessage::Assistant { text, .. } => {
            let text = doc_assistant_assistant_display_text(text);
            (!text.trim().is_empty()).then_some(DocAssistantDisplayMessage {
                role: DocAssistantRole::Assistant,
                text,
                pending: false,
            })
        }
        AgentChatMessage::Thought { .. } => None,
    }
}

pub(super) fn doc_assistant_user_display_text(text: &str) -> String {
    let marker = "User request:";
    let mut display = text
        .find(marker)
        .map(|index| &text[index + marker.len()..])
        .unwrap_or(text);
    if let Some(index) = display.find("You are helping plan this project doc:") {
        display = &display[..index];
    }
    display.trim().to_string()
}

pub(super) fn doc_assistant_assistant_display_text(text: &str) -> String {
    compact_doc_assistant_reply(text.trim())
}

fn compact_doc_assistant_reply(text: &str) -> String {
    let (without_code, removed_blocks) = remove_fenced_code_blocks(text);
    let (without_doc_echo, removed_doc_echo) = remove_numbered_doc_echo_blocks(&without_code);
    let display = without_doc_echo.trim().to_string();
    if display.is_empty() && (removed_blocks > 0 || removed_doc_echo > 0) {
        return String::new();
    }
    if removed_doc_echo > 0 && is_doc_read_preamble_only(&display) {
        return String::new();
    }

    let line_count = display.lines().count();
    if looks_like_code_dump(&display) || looks_like_doc_echo(&display) {
        return String::new();
    }

    if display.len() > 1800 || line_count > 28 {
        let mut lines = display.lines().take(14).collect::<Vec<_>>();
        while matches!(lines.last(), Some(line) if line.trim().is_empty()) {
            lines.pop();
        }
        lines.join("\n").trim().to_string()
    } else {
        display
    }
}

fn remove_fenced_code_blocks(text: &str) -> (String, usize) {
    let mut output = String::new();
    let mut removed = 0usize;
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            if in_fence {
                removed += 1;
            }
            continue;
        }
        if !in_fence {
            output.push_str(line);
            output.push('\n');
        }
    }
    (output, removed)
}

fn remove_numbered_doc_echo_blocks(text: &str) -> (String, usize) {
    let lines = text.lines().collect::<Vec<_>>();
    let mut output = String::new();
    let mut removed = 0usize;
    let mut index = 0usize;

    while index < lines.len() {
        if numbered_doc_line_run_len(&lines, index) >= 5 {
            removed += 1;
            index += 1;
            while index < lines.len() && looks_like_numbered_doc_line(lines[index]) {
                index += 1;
            }
            continue;
        }
        output.push_str(lines[index]);
        output.push('\n');
        index += 1;
    }

    (output, removed)
}

fn numbered_doc_line_run_len(lines: &[&str], start: usize) -> usize {
    lines[start..]
        .iter()
        .take_while(|line| looks_like_numbered_doc_line(line))
        .count()
}

fn looks_like_numbered_doc_line(line: &str) -> bool {
    let line = line.trim_start();
    let Some((number, rest)) = line.split_once(char::is_whitespace) else {
        return false;
    };
    !number.is_empty()
        && number.len() <= 4
        && number.chars().all(|ch| ch.is_ascii_digit())
        && (rest.trim().is_empty()
            || rest.trim_start().starts_with('#')
            || rest.trim_start().starts_with('-')
            || rest.trim_start().starts_with('*')
            || rest.trim_start().starts_with(char::is_alphanumeric))
}

fn is_doc_read_preamble_only(text: &str) -> bool {
    let normalized = text.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return true;
    }
    let word_count = normalized.split_whitespace().count();
    word_count <= 18
        && (normalized.contains("take a look at the current document")
            || normalized.contains("look at the current document")
            || normalized.contains("read the current document")
            || normalized.contains("get context"))
}

fn looks_like_doc_echo(text: &str) -> bool {
    let total = text.lines().filter(|line| !line.trim().is_empty()).count();
    if total < 5 {
        return false;
    }
    let numbered = text
        .lines()
        .filter(|line| looks_like_numbered_doc_line(line))
        .count();
    numbered * 2 >= total
}

fn looks_like_code_dump(text: &str) -> bool {
    let lines = text.lines().filter(|line| !line.trim().is_empty()).count();
    if lines < 12 {
        return false;
    }
    let codeish = text
        .lines()
        .filter(|line| {
            let line = line.trim();
            line.ends_with(';')
                || line.ends_with('{')
                || line.ends_with('}')
                || line.starts_with("function ")
                || line.starts_with("const ")
                || line.starts_with("let ")
                || line.starts_with("var ")
                || line.starts_with("class ")
                || line.starts_with('<')
        })
        .count();
    codeish * 2 >= lines
}

pub(super) fn active_composer_doc_mention(input: &InputState) -> Option<ComposerDocMention> {
    let text = input.value().to_string();
    let cursor = input.cursor().min(text.len());
    let prefix = &text[..cursor];
    let start = prefix.rfind("@@")?;
    let query = &prefix[start + 2..];
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    if start > 0 {
        let previous = prefix[..start].chars().next_back();
        if previous.is_some_and(|ch| !ch.is_whitespace()) {
            return None;
        }
    }
    Some(ComposerDocMention {
        range: start..cursor,
        query: query.to_string(),
    })
}

pub(super) fn doc_matches_composer_mention(doc: &WorkspaceDocEntry, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    doc.title.to_ascii_lowercase().contains(&query)
        || doc
            .relative_path
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains(&query)
}

/// Docs matching the mention query, sorted by title and capped to a short list.
/// Shared by the picker render and the keyboard navigation so both agree on the
/// row order and count.
pub(super) fn composer_doc_mention_matches(
    mention: &ComposerDocMention,
    docs: &[WorkspaceDocEntry],
) -> Vec<WorkspaceDocEntry> {
    let mut matches = docs
        .iter()
        .filter(|doc| doc_matches_composer_mention(doc, &mention.query))
        .cloned()
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        left.title
            .to_ascii_lowercase()
            .cmp(&right.title.to_ascii_lowercase())
    });
    matches.truncate(8);
    matches
}

pub(super) fn input_position_for_byte_offset(text: &str, offset: usize) -> Position {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let row = before.chars().filter(|ch| *ch == '\n').count();
    let column = before
        .rsplit_once('\n')
        .map(|(_, tail)| tail)
        .unwrap_or(before)
        .chars()
        .count();
    Position::new(row as u32, column as u32)
}

/// Which tab of the files section is selected.
#[derive(Clone, PartialEq, Eq)]
pub enum FileSel {
    Editor(PathBuf),
    Diff(String),
}

/// One open diff view in the files section.
pub(super) struct DiffItem {
    pub(super) project: ProjectId,
    pub(super) key: String,
    pub(super) title: SharedString,
    pub(super) view: Entity<DiffPane>,
}

/// One virtualized diff view presented in an agent's bottom drawer.
#[derive(Clone)]
pub(super) struct AgentDiffDrawer {
    pub(super) project: ProjectId,
    pub(super) key: String,
    pub(super) title: SharedString,
    pub(super) kind: DiffKind,
    pub(super) view: Entity<DiffPane>,
}

/// One open database object view in the files section.
pub(super) struct DbItem {
    pub(super) project: ProjectId,
    pub(super) key: String,
    pub(super) meta: crate::ui::db::workspace::DbTabMeta,
    pub(super) view: Entity<DatabasePane>,
    pub(super) connection: ide_core::DatabaseHandle,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentDetailTab {
    Terminal,
    Files,
    Notes,
    Plan,
    Diff,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentRuntime {
    NotStarted,
    Working,
    Waiting,
    Open,
    Idle,
    Ended,
}
