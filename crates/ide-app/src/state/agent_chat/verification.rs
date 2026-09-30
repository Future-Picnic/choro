#![allow(dead_code, reason = "retained verification presentation API")]

/// Outcome of checking one requirement against the delivered work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationStatus {
    Met,
    Unclear,
    Missed,
}

impl VerificationStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Met => "Met",
            Self::Unclear => "Unclear",
            Self::Missed => "Missed",
        }
    }

    /// Parse a status from the first word of a heading line. Tolerant of the
    /// synonyms agents actually reach for.
    fn parse(text: &str) -> Option<Self> {
        let first = text.split_whitespace().next()?;
        if let Some(status) = Self::parse_emoji(first) {
            return Some(status);
        }
        let first = first.to_ascii_lowercase();
        let first = first.trim_end_matches(':');
        match first {
            "met" | "pass" | "passed" | "done" | "complete" | "completed" | "satisfied" => {
                Some(Self::Met)
            }
            "unclear" | "partial" | "partially" | "uncertain" | "unverified" | "unsure" => {
                Some(Self::Unclear)
            }
            "missed" | "missing" | "fail" | "failed" | "unmet" | "not" | "incomplete" => {
                Some(Self::Missed)
            }
            _ => None,
        }
    }

    fn parse_emoji(token: &str) -> Option<Self> {
        let token = token.trim_matches(|c: char| c.is_ascii_punctuation());
        match token {
            "✅" | "✔" | "✔️" => Some(Self::Met),
            "⚠" | "⚠️" | "❓" => Some(Self::Unclear),
            "❌" | "✖" | "✗" => Some(Self::Missed),
            _ => None,
        }
    }
}

/// One verified requirement: its status, the requirement itself, and the
/// one-line evidence the agent gave for the verdict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerificationItem {
    pub status: VerificationStatus,
    pub title: String,
    pub detail: String,
    /// Whether a fix has already been requested for this item — once set, the
    /// item shows as sent and drops out of the fixable set.
    pub fix_requested: bool,
}

impl VerificationItem {
    /// A short one-line label for the item, used when listing it back to the
    /// agent in a fix prompt.
    pub fn summary_line(&self) -> String {
        self.title.clone()
    }
}

/// A verification result the agent produced against the stated intent.
///
/// Display-only (no follow-up state beyond `fix_requested`): it carries the raw
/// verification markdown, the parsed items, and expand/collapse + fix flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verification {
    pub id: String,
    pub markdown: String,
    pub items: Vec<VerificationItem>,
    pub expanded: bool,
}

impl Verification {
    pub fn new(id: impl Into<String>, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let items = parse_verification_items(&markdown);
        Self {
            id: id.into(),
            markdown,
            items,
            expanded: false,
        }
    }

    /// No parseable items — fall back to rendering the raw markdown body.
    pub fn is_unparsed(&self) -> bool {
        self.items.is_empty()
    }

    pub fn met_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.status == VerificationStatus::Met)
            .count()
    }

    pub fn all_met(&self) -> bool {
        !self.items.is_empty() && self.met_count() == self.items.len()
    }

    /// Clearly Missed items that have no fix requested yet. Unclear findings
    /// need user discussion rather than an automatic code-change request.
    pub fn items_to_fix(&self) -> Vec<&VerificationItem> {
        self.items
            .iter()
            .filter(|item| item.status == VerificationStatus::Missed && !item.fix_requested)
            .collect()
    }

    pub fn has_pending_fixes(&self) -> bool {
        !self.items_to_fix().is_empty()
    }

    /// Items ordered for display: grouped Missed, then Unclear, then Met —
    /// most actionable first, so a collapsed card never hides the gaps its
    /// "Ask to fix" button targets.
    pub fn display_items(&self) -> Vec<(usize, &VerificationItem)> {
        let mut ordered: Vec<(usize, &VerificationItem)> = Vec::with_capacity(self.items.len());
        for status in [
            VerificationStatus::Missed,
            VerificationStatus::Unclear,
            VerificationStatus::Met,
        ] {
            ordered.extend(
                self.items
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.status == status),
            );
        }
        ordered
    }

    pub fn should_collapse(&self) -> bool {
        self.items.len() > 5
    }

    pub fn display_markdown(&self) -> String {
        self.markdown.trim().to_string()
    }
}

/// Parse the agent's verification markdown into structured items. Tolerant of
/// the usual shapes: `## Met` / `**Missed**` group headings followed by
/// `- **requirement** — evidence` list items with extra lines beneath.
pub fn parse_verification_items(markdown: &str) -> Vec<VerificationItem> {
    let mut items: Vec<VerificationItem> = Vec::new();
    let mut current_status: Option<VerificationStatus> = None;
    let mut current: Option<VerificationItem> = None;

    for raw in markdown.lines() {
        let line = raw.trim();
        if line.is_empty() {
            if let Some(item) = current.as_mut() {
                if !item.detail.is_empty() && !item.detail.ends_with('\n') {
                    item.detail.push('\n');
                }
            }
            continue;
        }

        if let Some(status) = parse_status_heading(line) {
            flush_item(&mut current, &mut items);
            current_status = Some(status);
            continue;
        }

        let Some(status) = current_status else {
            continue;
        };

        if let Some(content) = strip_list_marker(line) {
            flush_item(&mut current, &mut items);
            let (title, detail) = parse_item_head(content);
            current = Some(VerificationItem {
                status,
                title,
                detail,
                fix_requested: false,
            });
            continue;
        }

        if let Some(item) = current.as_mut() {
            if !item.detail.is_empty() && !item.detail.ends_with('\n') {
                item.detail.push('\n');
            }
            item.detail.push_str(line);
        }
    }
    flush_item(&mut current, &mut items);
    items
}

fn flush_item(current: &mut Option<VerificationItem>, items: &mut Vec<VerificationItem>) {
    if let Some(item) = current.take() {
        items.push(VerificationItem {
            detail: item.detail.trim().to_string(),
            ..item
        });
    }
}

fn parse_status_heading(line: &str) -> Option<VerificationStatus> {
    let had_hash = line.starts_with('#');
    let bold = line.starts_with("**");
    let cleaned = line
        .trim_start_matches('#')
        .trim()
        .trim_matches('*')
        .trim_matches('_')
        .trim()
        .trim_end_matches(':')
        .trim();
    if cleaned.is_empty() {
        return None;
    }
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let heading_shape = had_hash
        || bold
        || words.len() == 1
        || (words.len() == 2
            && matches!(
                words[1].to_ascii_lowercase().as_str(),
                "requirements" | "items" | "checks" | "yet"
            ));
    if !heading_shape {
        return None;
    }
    VerificationStatus::parse(cleaned)
}

fn strip_list_marker(line: &str) -> Option<&str> {
    for marker in ["- ", "* ", "• ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some(rest.trim());
        }
    }
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i > 0 && i < bytes.len() && (bytes[i] == b'.' || bytes[i] == b')') {
        return Some(line[i + 1..].trim_start());
    }
    None
}

/// Split a list item into the requirement title and its evidence line.
/// Handles `**title** — evidence`, `**title — evidence**`, and plain
/// `title — evidence` (em/en/hyphen dash, or a colon).
fn parse_item_head(content: &str) -> (String, String) {
    let content = content.trim();
    if let Some(stripped) = content.strip_prefix("**") {
        if let Some(end) = stripped.find("**") {
            let head = stripped[..end].trim();
            let rest = strip_leading_separator(stripped[end + 2..].trim());
            if rest.is_empty() {
                let (title, detail) = split_title_detail(head);
                return (title, detail);
            }
            return (head.to_string(), rest.to_string());
        }
    }
    split_title_detail(content)
}

fn split_title_detail(head: &str) -> (String, String) {
    for sep in [" — ", " – ", " - ", "—", "–"] {
        if let Some(idx) = head.find(sep) {
            let left = head[..idx].trim();
            let right = head[idx + sep.len()..].trim();
            if !left.is_empty() && !right.is_empty() {
                return (left.to_string(), right.to_string());
            }
        }
    }
    (head.to_string(), String::new())
}

fn strip_leading_separator(text: &str) -> &str {
    let mut rest = text;
    for sep in ["—", "–", "-", ":"] {
        if let Some(stripped) = rest.strip_prefix(sep) {
            rest = stripped.trim_start();
            break;
        }
    }
    rest
}

/// Split an assistant message into its prose and an optional verification
/// block. Live turns get the verification from a streaming event, but a
/// *resumed* transcript carries the raw `<verification>…</verification>` text —
/// this lets the resume path rebuild the card and drop the raw tags from the
/// visible message. Returns `(cleaned_text, verification_markdown)`.
pub fn split_verification(text: &str) -> (String, Option<String>) {
    const OPEN: &str = "<verification>";
    const CLOSE: &str = "</verification>";
    let Some(open) = text.find(OPEN) else {
        return (text.to_string(), None);
    };
    let after_open = open + OPEN.len();
    let Some(close_rel) = text[after_open..].find(CLOSE) else {
        return (text.to_string(), None);
    };
    let close = after_open + close_rel;
    let verification = text[after_open..close].trim();
    let before = text[..open].trim();
    let after = text[close + CLOSE.len()..].trim();
    let cleaned = match (before.is_empty(), after.is_empty()) {
        (true, true) => String::new(),
        (false, true) => before.to_string(),
        (true, false) => after.to_string(),
        (false, false) => format!("{before}\n\n{after}"),
    };
    let verification = (!verification.is_empty()).then(|| verification.to_string());
    (cleaned, verification)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_verification_block_from_prose() {
        let text = "Checked.\n\n<verification>\n## Met\n- **thing** — done\n</verification>";
        let (cleaned, verification) = split_verification(text);
        assert_eq!(cleaned, "Checked.");
        assert_eq!(verification.as_deref(), Some("## Met\n- **thing** — done"));
    }

    #[test]
    fn no_block_returns_text_unchanged() {
        let (cleaned, verification) = split_verification("just prose");
        assert_eq!(cleaned, "just prose");
        assert!(verification.is_none());
    }

    #[test]
    fn split_round_trips_prose_on_both_sides() {
        let text = "Before.\n<verification>\n## Met\n- a\n</verification>\nAfter.";
        let (cleaned, verification) = split_verification(text);
        assert_eq!(cleaned, "Before.\n\nAfter.");
        assert_eq!(verification.as_deref(), Some("## Met\n- a"));
    }

    #[test]
    fn parses_grouped_items_with_evidence() {
        let markdown = "## Met\n- **Dark mode toggle persists** — saved to settings.json on change\n- **Uses design tokens** — all colors via design::*\n\n## Missed\n- **Tests added** — no test was written for the toggle";
        let verification = Verification::new("v1", markdown);
        assert_eq!(verification.items.len(), 3);
        assert_eq!(verification.items[0].status, VerificationStatus::Met);
        assert_eq!(verification.items[0].title, "Dark mode toggle persists");
        assert_eq!(
            verification.items[0].detail,
            "saved to settings.json on change"
        );
        assert_eq!(verification.items[2].status, VerificationStatus::Missed);
        assert_eq!(verification.met_count(), 2);
        assert!(!verification.all_met());
        assert_eq!(verification.items_to_fix().len(), 1);
    }

    #[test]
    fn parses_status_synonyms_and_emoji() {
        let markdown = "## ✅ Passed\n- a — ok\n## Partial\n- b — unsure\n**Failed**\n- c — absent";
        let items = parse_verification_items(markdown);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].status, VerificationStatus::Met);
        assert_eq!(items[1].status, VerificationStatus::Unclear);
        assert_eq!(items[2].status, VerificationStatus::Missed);
    }

    #[test]
    fn evidence_continuation_lines_join_detail() {
        let markdown = "## Met\n- **req** — first line\nsecond line";
        let items = parse_verification_items(markdown);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].detail, "first line\nsecond line");
    }

    #[test]
    fn dash_inside_bold_still_splits_title_and_evidence() {
        let markdown = "## Missed\n- **req — no evidence found**";
        let items = parse_verification_items(markdown);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "req");
        assert_eq!(items[0].detail, "no evidence found");
    }

    #[test]
    fn items_before_any_heading_are_ignored() {
        let markdown = "- floating item\n## Met\n- real item — ok";
        let items = parse_verification_items(markdown);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "real item");
    }

    #[test]
    fn malformed_block_keeps_markdown_but_no_items() {
        let verification = Verification::new("v1", "Everything looks great, ship it.");
        assert!(verification.is_unparsed());
        assert!(!verification.all_met());
        assert_eq!(
            verification.display_markdown(),
            "Everything looks great, ship it."
        );
    }

    #[test]
    fn display_items_group_missed_unclear_met() {
        let markdown = "## Met\n- ok1 — y\n## Missed\n- m1 — x\n## Unclear\n- u1 — z";
        let verification = Verification::new("v1", markdown);
        let ordered: Vec<&str> = verification
            .display_items()
            .iter()
            .map(|(_, item)| item.title.as_str())
            .collect();
        assert_eq!(ordered, vec!["m1", "u1", "ok1"]);
    }

    #[test]
    fn unclear_items_are_not_automatic_fix_targets() {
        let verification =
            Verification::new("v1", "## Unclear\n- discuss first — intent is ambiguous");
        assert!(verification.items_to_fix().is_empty());
        assert!(!verification.has_pending_fixes());
    }

    #[test]
    fn all_met_summary_state() {
        let verification = Verification::new("v1", "## Met\n- a — done\n- b — done");
        assert!(verification.all_met());
        assert!(!verification.has_pending_fixes());
    }
}
