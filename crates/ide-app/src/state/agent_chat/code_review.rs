#![allow(dead_code, reason = "retained code-review presentation API")]

/// Severity of a single code-review finding. Ordered highest-first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeReviewSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

impl CodeReviewSeverity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::Info => "Info",
        }
    }

    /// Parse a severity from the first word of a heading line.
    fn parse(text: &str) -> Option<Self> {
        let first = text.split_whitespace().next()?.to_ascii_lowercase();
        let first = first.trim_end_matches(':');
        match first {
            "critical" | "blocker" | "p0" => Some(Self::Critical),
            "high" | "major" | "p1" => Some(Self::High),
            "medium" | "moderate" | "warning" | "p2" => Some(Self::Medium),
            "low" | "minor" | "p3" => Some(Self::Low),
            "info" | "informational" | "suggestion" | "nit" | "note" => Some(Self::Info),
            _ => None,
        }
    }
}

/// A single finding from the agent's review: a severity, an optional
/// `path:line` location, a short title, and the explanation + suggested fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeReviewFinding {
    pub severity: CodeReviewSeverity,
    pub location: Option<String>,
    pub title: String,
    pub detail: String,
    /// The plain-language consequence, parsed from a `What happens:` line
    /// (`Impact:` accepted). Reviews without labels keep the whole detail here.
    pub impact: String,
    /// The concrete fix, parsed from a `Suggested fix:` line (`Fix:` accepted).
    pub fix: Option<String>,
    /// Whether the user has ticked this finding for a targeted "Fix selected".
    pub selected: bool,
    /// Whether a fix has already been requested for this finding — once set, the
    /// finding shows as done and drops out of the selectable / fixable set.
    pub fix_requested: bool,
}

impl CodeReviewFinding {
    /// A short one-line label for the finding, used when listing it back to the
    /// agent in a fix prompt: `path:line — title`, or just the title.
    pub fn summary_line(&self) -> String {
        match &self.location {
            Some(location) => format!("{location} — {}", self.title),
            None => self.title.clone(),
        }
    }
}

/// A code-review result the agent produced for the working-tree changes.
///
/// Display-only (no follow-up state beyond `fix_requested`): it carries the raw
/// review markdown, the parsed findings, and expand/collapse + fix flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodeReview {
    pub id: String,
    pub markdown: String,
    pub findings: Vec<CodeReviewFinding>,
    pub expanded: bool,
}

impl CodeReview {
    pub fn new(id: impl Into<String>, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        let findings = parse_code_review_findings(&markdown);
        Self {
            id: id.into(),
            markdown,
            findings,
            expanded: false,
        }
    }

    /// Missing findings alone say nothing about whether the review finished.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty() && self.coverage_complete()
    }

    pub fn coverage(&self) -> Option<String> {
        let mut lines = self.markdown.lines();
        lines.find(|line| is_coverage_heading(line.trim()))?;
        Some(
            lines
                .take_while(|line| !line.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string(),
        )
    }

    pub fn coverage_complete(&self) -> bool {
        self.coverage().is_some_and(|coverage| {
            let statuses = coverage
                .lines()
                .map(|line| line.trim().trim_matches('`'))
                .filter(|line| line.to_ascii_lowercase().starts_with("completion:"))
                .collect::<Vec<_>>();
            statuses.len() == 1 && statuses[0].eq_ignore_ascii_case("Completion: complete")
        })
    }

    pub fn selected_count(&self) -> usize {
        self.findings
            .iter()
            .filter(|finding| finding.selected && !finding.fix_requested)
            .count()
    }

    /// At least one finding still has no fix requested.
    pub fn has_pending_fixes(&self) -> bool {
        self.findings.iter().any(|finding| !finding.fix_requested)
    }

    /// The findings to send to a fix prompt: among the not-yet-fixed ones, the
    /// ticked ones (`only_selected`) or — for "Fix all" — every pending one.
    pub fn findings_to_fix(&self, only_selected: bool) -> Vec<&CodeReviewFinding> {
        self.findings
            .iter()
            .filter(|finding| !finding.fix_requested && (!only_selected || finding.selected))
            .collect()
    }

    pub fn should_collapse(&self) -> bool {
        self.findings.len() > 3
    }

    pub fn display_markdown(&self) -> String {
        self.markdown.trim().to_string()
    }
}

/// Parse the agent's review markdown into structured findings. Tolerant of the
/// usual shapes: `## Severity` / `**Severity**` group headings followed by
/// `- **path:line — title**` list items with explanation paragraphs beneath.
pub fn parse_code_review_findings(markdown: &str) -> Vec<CodeReviewFinding> {
    let mut findings: Vec<CodeReviewFinding> = Vec::new();
    let mut current_severity = CodeReviewSeverity::Info;
    let mut current: Option<CodeReviewFinding> = None;
    let mut in_coverage = false;

    for raw in markdown.lines() {
        let line = raw.trim();
        if is_coverage_heading(line) {
            flush_finding(&mut current, &mut findings);
            in_coverage = true;
            continue;
        }
        if in_coverage {
            if parse_severity_heading(line).is_none() {
                continue;
            }
            in_coverage = false;
        }
        if line.is_empty() {
            if let Some(finding) = current.as_mut() {
                if !finding.detail.is_empty() && !finding.detail.ends_with('\n') {
                    finding.detail.push('\n');
                }
            }
            continue;
        }

        if let Some(severity) = parse_severity_heading(line) {
            flush_finding(&mut current, &mut findings);
            current_severity = severity;
            continue;
        }

        if let Some(content) = strip_list_marker(line) {
            flush_finding(&mut current, &mut findings);
            let (location, title, rest) = parse_finding_head(content);
            current = Some(CodeReviewFinding {
                severity: current_severity,
                location,
                title,
                detail: rest,
                impact: String::new(),
                fix: None,
                selected: false,
                fix_requested: false,
            });
            continue;
        }

        if let Some(finding) = current.as_mut() {
            if !finding.detail.is_empty() && !finding.detail.ends_with('\n') {
                finding.detail.push('\n');
            }
            finding.detail.push_str(line);
        }
    }
    flush_finding(&mut current, &mut findings);
    findings
}

fn is_coverage_heading(line: &str) -> bool {
    line.starts_with('#')
        && line
            .trim_matches('#')
            .trim()
            .eq_ignore_ascii_case("Coverage")
}

fn flush_finding(current: &mut Option<CodeReviewFinding>, findings: &mut Vec<CodeReviewFinding>) {
    if let Some(mut finding) = current.take() {
        finding.detail = finding.detail.trim().to_string();
        let (impact, fix) = split_impact_fix(&finding.detail);
        finding.impact = impact;
        finding.fix = fix;
        findings.push(finding);
    }
}

/// Split a finding's detail into the consequence and the suggested fix, keyed
/// by the labeled lines the output instructions request. A detail with no
/// labels at all keeps its full text as the consequence, so reviews from older
/// prompts (and resumed transcripts) still render.
fn split_impact_fix(detail: &str) -> (String, Option<String>) {
    let mut impact = String::new();
    let mut fix = String::new();
    let mut in_fix = false;
    for raw in detail.lines() {
        let line = raw.trim();
        let text = if let Some(rest) = strip_field_label(line, &["what happens", "impact"]) {
            in_fix = false;
            rest
        } else if let Some(rest) = strip_field_label(line, &["suggested fix", "fix"]) {
            in_fix = true;
            rest
        } else {
            line
        };
        if text.is_empty() {
            continue;
        }
        let buf = if in_fix { &mut fix } else { &mut impact };
        if !buf.is_empty() {
            buf.push('\n');
        }
        buf.push_str(text);
    }
    if impact.is_empty() && fix.is_empty() {
        return (detail.trim().to_string(), None);
    }
    (impact, (!fix.is_empty()).then_some(fix))
}

/// Strip a leading `Label:` from a line, tolerating markdown emphasis around
/// the label (`**What happens:** …`). Returns the remainder after the colon.
fn strip_field_label<'a>(line: &'a str, labels: &[&str]) -> Option<&'a str> {
    let trimmed = line.trim_start_matches(['*', '_']).trim_start();
    for label in labels {
        let matches_label = trimmed
            .get(..label.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(label));
        if matches_label {
            let rest = trimmed[label.len()..].trim_start_matches(['*', '_', ' ']);
            if let Some(rest) = rest.strip_prefix(':') {
                return Some(rest.trim_start_matches(['*', '_', ' ']).trim());
            }
        }
    }
    None
}

fn parse_severity_heading(line: &str) -> Option<CodeReviewSeverity> {
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
                "issues" | "severity" | "priority" | "findings" | "risk" | "risks"
            ));
    if !heading_shape {
        return None;
    }
    CodeReviewSeverity::parse(cleaned)
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

fn parse_finding_head(content: &str) -> (Option<String>, String, String) {
    let content = content.trim();
    let (head, rest) = if let Some(stripped) = content.strip_prefix("**") {
        if let Some(end) = stripped.find("**") {
            (
                stripped[..end].trim().to_string(),
                stripped[end + 2..].trim().to_string(),
            )
        } else {
            (content.to_string(), String::new())
        }
    } else {
        (content.to_string(), String::new())
    };
    let (location, title) = split_location_title(&head);
    (location, title, rest)
}

fn split_location_title(head: &str) -> (Option<String>, String) {
    for sep in [" — ", " – ", " - ", "—", "–"] {
        if let Some(idx) = head.find(sep) {
            let left = head[..idx].trim();
            let right = head[idx + sep.len()..].trim();
            if looks_like_location(left) && !right.is_empty() {
                return (Some(left.to_string()), right.to_string());
            }
        }
    }
    (None, head.to_string())
}

fn looks_like_location(text: &str) -> bool {
    !text.is_empty() && !text.contains(' ') && (text.contains('/') || text.contains('.'))
}

/// Split an assistant message into its prose and an optional code-review block.
/// Live turns get the review from a streaming event, but a *resumed* transcript
/// carries the raw `<code_review>…</code_review>` text — this lets the resume
/// path rebuild the review card and drop the raw tags from the visible message.
/// Returns `(cleaned_text, review_markdown)`.
pub fn split_code_review(text: &str) -> (String, Option<String>) {
    const OPEN: &str = "<code_review>";
    const CLOSE: &str = "</code_review>";
    let Some(open) = text.find(OPEN) else {
        return (text.to_string(), None);
    };
    let after_open = open + OPEN.len();
    let Some(close_rel) = text[after_open..].find(CLOSE) else {
        return (text.to_string(), None);
    };
    let close = after_open + close_rel;
    let review = text[after_open..close].trim();
    let before = text[..open].trim();
    let after = text[close + CLOSE.len()..].trim();
    let cleaned = match (before.is_empty(), after.is_empty()) {
        (true, true) => String::new(),
        (false, true) => before.to_string(),
        (true, false) => after.to_string(),
        (false, false) => format!("{before}\n\n{after}"),
    };
    let review = (!review.is_empty()).then(|| review.to_string());
    (cleaned, review)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_review_block_from_prose() {
        let text = "Here you go.\n\n<code_review>\n## Findings\n- bug\n</code_review>";
        let (cleaned, review) = split_code_review(text);
        assert_eq!(cleaned, "Here you go.");
        assert_eq!(review.as_deref(), Some("## Findings\n- bug"));
    }

    #[test]
    fn no_block_returns_text_unchanged() {
        let (cleaned, review) = split_code_review("just prose");
        assert_eq!(cleaned, "just prose");
        assert!(review.is_none());
    }

    #[test]
    fn parses_grouped_findings_with_location_and_detail() {
        let markdown = "## High\n- **src/a.rs:45 — Race on save**\nTwo writers can clash.\nUse an atomic upsert.\n\n## Low\n- **ios/proj.pbxproj:463 — Signing team changed**\nRevert unless intentional.";
        let review = CodeReview::new("r1", markdown);
        assert_eq!(review.findings.len(), 2);
        assert_eq!(review.findings[0].severity, CodeReviewSeverity::High);
        assert_eq!(review.findings[0].location.as_deref(), Some("src/a.rs:45"));
        assert_eq!(review.findings[0].title, "Race on save");
        assert!(review.findings[0].detail.contains("atomic upsert"));
        assert_eq!(review.findings[1].severity, CodeReviewSeverity::Low);
        assert!(!review.is_clean());
    }

    #[test]
    fn clean_review_has_no_findings() {
        let review = CodeReview::new("r1", "## Coverage\nCompletion: complete\nReviewed all 88 feature files; excluded 7 unrelated calendar files.\n\nNo findings in the reviewed code.");
        assert!(review.is_clean());
    }

    #[test]
    fn incomplete_or_legacy_reviews_never_imply_complete_coverage() {
        for markdown in [
            "",
            "The changes look clean.",
            "I could not read the repository.",
            "## Coverage\nCompletion: partial\nReviewed 24 of 88 files. No findings so far.",
            "## Coverage\nCompletion: complete\nCompletion: partial\nSome files remain.",
        ] {
            assert!(!CodeReview::new("partial", markdown).is_clean());
        }
    }

    #[test]
    fn coverage_lists_are_preserved_but_do_not_become_findings() {
        let review = CodeReview::new("r", "## Coverage\nCompletion: partial\n- Reviewed 24 files.\n- 64 files remain.\n## High\n- **src/auth.rs:9 — Access check missing**\nWhat happens: another user can read a private file.\nSuggested fix: check membership.");
        assert_eq!(review.findings.len(), 1);
        assert_eq!(
            review.findings[0].location.as_deref(),
            Some("src/auth.rs:9")
        );
        assert!(review.coverage().unwrap().contains("64 files remain"));
        assert!(!review.coverage_complete());
    }

    #[test]
    fn parses_labeled_impact_and_fix_lines() {
        let markdown = "## Medium\n- **src/a.rs:45 — Saves can overwrite each other**\nWhat happens: two edits at once and the later one silently wins.\nSuggested fix: guard the save with a version check.";
        let review = CodeReview::new("r1", markdown);
        assert_eq!(review.findings.len(), 1);
        assert_eq!(
            review.findings[0].impact,
            "two edits at once and the later one silently wins."
        );
        assert_eq!(
            review.findings[0].fix.as_deref(),
            Some("guard the save with a version check.")
        );
    }

    #[test]
    fn accepts_bolded_and_alias_labels() {
        let markdown = "## High\n- **src/b.rs:12 — Panic on empty input**\n**Impact:** the app crashes on an empty search.\n**Fix:** return early when the query is empty.";
        let review = CodeReview::new("r1", markdown);
        assert_eq!(
            review.findings[0].impact,
            "the app crashes on an empty search."
        );
        assert_eq!(
            review.findings[0].fix.as_deref(),
            Some("return early when the query is empty.")
        );
    }

    #[test]
    fn unlabeled_detail_falls_back_to_impact() {
        let markdown = "## Low\n- **src/c.rs:3 — Old-style finding**\nJust one explanation paragraph with a fix mixed in.";
        let review = CodeReview::new("r1", markdown);
        assert_eq!(
            review.findings[0].impact,
            "Just one explanation paragraph with a fix mixed in."
        );
        assert!(review.findings[0].fix.is_none());
    }

    #[test]
    fn multi_line_fix_keeps_following_lines() {
        let markdown = "## Medium\n- **src/d.rs:9 — Leak on retry**\nWhat happens: memory grows on every retry.\nSuggested fix: drop the old handle first.\nThen reconnect with a fresh client.";
        let review = CodeReview::new("r1", markdown);
        assert_eq!(
            review.findings[0].fix.as_deref(),
            Some("drop the old handle first.\nThen reconnect with a fresh client.")
        );
    }
}
