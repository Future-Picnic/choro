// Not a product limit: this only protects rendering and persistence from a
// malformed or adversarial provider response. Real large features may need
// dozens of checks.
const PARSER_SAFETY_ITEM_LIMIT: usize = 100;
const MAX_ACTION_CHARS: usize = 120;
const MAX_EXPECTED_CHARS: usize = 180;
const MAX_FLOW_CHARS: usize = 80;
pub const REVIEW_CHECKLIST_REQUEST_MARKER: &str = "<!-- choro:review-checklist -->";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewChecklistStatus {
    Pending,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewChecklistItem {
    pub id: String,
    pub flow: Option<String>,
    pub action: String,
    pub expected: Option<String>,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewChecklist {
    pub id: String,
    pub source_turn_id: String,
    pub status: ReviewChecklistStatus,
    pub items: Vec<ReviewChecklistItem>,
    pub expanded: bool,
    pub created_at: u64,
}

impl ReviewChecklist {
    pub fn pending(source_turn_id: impl Into<String>, created_at: u64) -> Self {
        let source_turn_id = source_turn_id.into();
        Self {
            id: format!("review-checklist-{source_turn_id}"),
            source_turn_id,
            status: ReviewChecklistStatus::Pending,
            items: Vec::new(),
            expanded: true,
            created_at,
        }
    }

    pub fn ready(source_turn_id: impl Into<String>, markdown: &str, created_at: u64) -> Self {
        let source_turn_id = source_turn_id.into();
        Self {
            id: format!("review-checklist-{source_turn_id}"),
            source_turn_id,
            status: ReviewChecklistStatus::Ready,
            items: parse_review_checklist_items(markdown),
            expanded: true,
            created_at,
        }
    }

    pub fn completed_count(&self) -> usize {
        self.items.iter().filter(|item| item.checked).count()
    }

    pub fn is_complete(&self) -> bool {
        !self.items.is_empty() && self.completed_count() == self.items.len()
    }
}

pub fn split_review_checklist(text: &str) -> (String, Option<String>) {
    const OPEN: &str = "<review_checklist>";
    const CLOSE: &str = "</review_checklist>";
    let Some(open) = text.find(OPEN) else {
        return (text.to_string(), None);
    };
    let after_open = open + OPEN.len();
    let Some(relative_close) = text[after_open..].find(CLOSE) else {
        return (text.to_string(), None);
    };
    let close = after_open + relative_close;
    let body = text[after_open..close].trim();
    let mut cleaned = String::new();
    cleaned.push_str(text[..open].trim_end());
    let suffix = text[close + CLOSE.len()..].trim_start();
    if !cleaned.is_empty() && !suffix.is_empty() {
        cleaned.push_str("\n\n");
    }
    cleaned.push_str(suffix);
    (cleaned.trim().to_string(), Some(body.to_string()))
}

pub fn parse_review_checklist_items(markdown: &str) -> Vec<ReviewChecklistItem> {
    let trimmed = markdown.trim();
    if trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("no manual checks needed")
    {
        return Vec::new();
    }

    let mut items = Vec::new();
    let mut current_flow: Option<String> = None;
    for line in markdown.lines() {
        let line = line.trim();
        if let Some(heading) = markdown_heading(line) {
            current_flow = Some(bounded(heading, MAX_FLOW_CHARS)).filter(|value| !value.is_empty());
            continue;
        }
        let parsed = (|| {
            let content = ["- ", "* ", "• ", "+ "]
                .iter()
                .find_map(|marker| line.strip_prefix(marker))?
                .trim();
            if content.is_empty() {
                return None;
            }
            let (action, expected) = split_action_expected(content);
            let action = bounded(action.trim_matches('*').trim(), MAX_ACTION_CHARS);
            if action.is_empty() {
                return None;
            }
            let expected = expected
                .map(|value| bounded(value.trim_matches('*').trim(), MAX_EXPECTED_CHARS))
                .filter(|value| !value.is_empty());
            Some((action, expected))
        })();
        let Some((action, expected)) = parsed else {
            continue;
        };
        items.push(ReviewChecklistItem {
            id: format!("check-{}", items.len() + 1),
            flow: current_flow.clone(),
            action,
            expected,
            checked: false,
        });
        if items.len() >= PARSER_SAFETY_ITEM_LIMIT {
            break;
        }
    }
    items
}

fn markdown_heading(line: &str) -> Option<&str> {
    let heading = line.strip_prefix('#')?.trim_start_matches('#').trim();
    (!heading.is_empty()).then_some(heading.trim_matches('*').trim())
}

fn split_action_expected(content: &str) -> (&str, Option<&str>) {
    for separator in [" — ", " – ", " - "] {
        if let Some((action, expected)) = content.split_once(separator) {
            return (action, Some(expected));
        }
    }
    (content, None)
}

fn bounded(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bounded_manual_checks() {
        let items = parse_review_checklist_items(
            "- **Open Settings** — The new toggle is visible\n- Resize the window – Content stays readable",
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].action, "Open Settings");
        assert_eq!(
            items[0].expected.as_deref(),
            Some("The new toggle is visible")
        );
        assert_eq!(items[0].flow, None);
    }

    #[test]
    fn accepts_an_explicit_empty_result() {
        assert!(parse_review_checklist_items("No manual checks needed").is_empty());
    }

    #[test]
    fn keeps_large_feature_checklists_with_a_rendering_safety_bound() {
        let markdown = (0..120)
            .map(|index| format!("- {} — {}", "a".repeat(300), index))
            .collect::<Vec<_>>()
            .join("\n");
        let items = parse_review_checklist_items(&markdown);
        assert_eq!(items.len(), 100);
        assert_eq!(items[0].action.chars().count(), 120);
    }

    #[test]
    fn headings_group_checks_into_optional_flows() {
        let items = parse_review_checklist_items(
            "## Tasks\n- Add a task — It appears\n- Change view — The view updates\n\n## Team members\n- Invite a member — They appear",
        );

        assert_eq!(items.len(), 3);
        assert_eq!(items[0].flow.as_deref(), Some("Tasks"));
        assert_eq!(items[1].flow.as_deref(), Some("Tasks"));
        assert_eq!(items[2].flow.as_deref(), Some("Team members"));
    }

    #[test]
    fn splits_tagged_output_without_leaking_it_into_chat() {
        let (cleaned, checklist) = split_review_checklist(
            "before\n<review_checklist>\n- Open it — It works\n</review_checklist>\nafter",
        );
        assert_eq!(cleaned, "before\n\nafter");
        assert_eq!(checklist.as_deref(), Some("- Open it — It works"));
    }
}
