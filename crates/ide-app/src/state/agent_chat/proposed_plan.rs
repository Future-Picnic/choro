use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposedPlan {
    pub id: String,
    pub title: String,
    pub markdown: String,
    pub expanded: bool,
    pub implemented_at: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposedPlanSubmission {
    pub text: String,
    pub refine_in_plan_mode: bool,
}

impl ProposedPlan {
    pub fn new(id: impl Into<String>, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        Self {
            id: id.into(),
            title: proposed_plan_title(&markdown).unwrap_or_else(|| "Proposed plan".to_string()),
            markdown,
            expanded: false,
            implemented_at: None,
        }
    }

    pub fn mark_implemented(&mut self) {
        self.implemented_at = Some(unix_now());
    }

    pub fn display_markdown(&self) -> String {
        strip_displayed_plan_markdown(&self.markdown)
    }

    pub fn collapsed_preview(&self, max_lines: usize) -> String {
        build_collapsed_proposed_plan_preview_markdown(&self.markdown, max_lines)
    }

    pub fn should_collapse(&self) -> bool {
        self.markdown.len() > 900 || self.markdown.lines().count() > 20
    }

    pub fn resolve_submission(&self, draft_text: &str) -> ProposedPlanSubmission {
        let trimmed = draft_text.trim();
        if trimmed.is_empty() {
            ProposedPlanSubmission {
                text: build_plan_implementation_prompt(&self.markdown),
                refine_in_plan_mode: false,
            }
        } else {
            ProposedPlanSubmission {
                text: trimmed.to_string(),
                refine_in_plan_mode: true,
            }
        }
    }
}

/// Split an assistant message into its prose and an optional proposed-plan
/// block. Live turns get the plan from a streaming event, but a *resumed*
/// transcript carries the raw `<proposed_plan>…</proposed_plan>` text — this
/// lets the resume path rebuild the plan card and drop the raw tags from the
/// visible message. Returns `(cleaned_text, plan_markdown)`.
pub fn split_proposed_plan(text: &str) -> (String, Option<String>) {
    const OPEN: &str = "<proposed_plan>";
    const CLOSE: &str = "</proposed_plan>";
    let Some(open) = text.find(OPEN) else {
        return (text.to_string(), None);
    };
    let after_open = open + OPEN.len();
    let Some(close_rel) = text[after_open..].find(CLOSE) else {
        return (text.to_string(), None);
    };
    let close = after_open + close_rel;
    let plan = text[after_open..close].trim();
    let before = text[..open].trim();
    let after = text[close + CLOSE.len()..].trim();
    let cleaned = match (before.is_empty(), after.is_empty()) {
        (true, true) => String::new(),
        (false, true) => before.to_string(),
        (true, false) => after.to_string(),
        (false, false) => format!("{before}\n\n{after}"),
    };
    let plan = (!plan.is_empty()).then(|| plan.to_string());
    (cleaned, plan)
}

pub fn proposed_plan_title(markdown: &str) -> Option<String> {
    markdown.lines().find_map(|line| {
        let trimmed = line.trim();
        let heading = trimmed.strip_prefix('#')?;
        let heading = heading.trim_start_matches('#').trim();
        (!heading.is_empty()).then(|| heading.to_string())
    })
}

pub fn strip_displayed_plan_markdown(markdown: &str) -> String {
    let mut lines = markdown.lines().peekable();

    while matches!(lines.peek(), Some(line) if line.trim().is_empty()) {
        lines.next();
    }

    if matches!(lines.peek(), Some(line) if line.trim_start().starts_with('#')) {
        lines.next();
        while matches!(lines.peek(), Some(line) if line.trim().is_empty()) {
            lines.next();
        }
    }

    if matches!(lines.peek(), Some(line) if markdown_heading_text(line).eq_ignore_ascii_case("summary"))
    {
        lines.next();
        while matches!(lines.peek(), Some(line) if line.trim().is_empty()) {
            lines.next();
        }
    }

    lines.collect::<Vec<_>>().join("\n").trim().to_string()
}

pub fn build_collapsed_proposed_plan_preview_markdown(markdown: &str, max_lines: usize) -> String {
    let stripped = strip_displayed_plan_markdown(markdown);
    let mut lines = stripped.lines().take(max_lines).collect::<Vec<_>>();
    while matches!(lines.last(), Some(line) if line.trim().is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

pub fn build_plan_implementation_prompt(markdown: &str) -> String {
    let markdown = markdown.trim();
    if markdown.is_empty() {
        return "Implement this plan.".to_string();
    }
    format!(
        "Implement the approved plan below. The complete plan is repeated here so implementation does not depend on prior session memory.\n\n<approved_plan>\n{markdown}\n</approved_plan>"
    )
}

fn markdown_heading_text(line: &str) -> String {
    line.trim_start_matches('#').trim().to_string()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_title_and_strips_summary_heading() {
        let markdown = "# Add pages\n\n## Summary\n\nBuild menu and contact pages.";
        let plan = ProposedPlan::new("p1", markdown);
        assert_eq!(plan.title, "Add pages");
        assert_eq!(plan.display_markdown(), "Build menu and contact pages.");
    }

    #[test]
    fn empty_follow_up_implements_plan() {
        let plan = ProposedPlan::new("p1", "# Plan\nDo it");
        let submission = plan.resolve_submission(" ");
        assert!(!submission.refine_in_plan_mode);
        assert!(submission
            .text
            .starts_with("Implement the approved plan below."));
        assert!(submission
            .text
            .contains("<approved_plan>\n# Plan\nDo it\n</approved_plan>"));
    }

    #[test]
    fn implementation_prompt_keeps_the_complete_approved_plan() {
        let markdown = "# Ship it\n\n1. Add the feature.\n2. Run the tests.";
        let prompt = build_plan_implementation_prompt(markdown);

        assert!(prompt.contains(markdown));
        assert!(prompt.contains("does not depend on prior session memory"));
    }
}
