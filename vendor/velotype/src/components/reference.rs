// Modified by Choro contributors; see vendor/velotype/CHORO_MODIFICATIONS.md.
//! Reference-embed blocks: rich, clickable cards that reference a file, doc, or
//! design from inside a document.
//!
//! Persistence mirrors the Mermaid/Math pattern: the block is a fenced
//! ```` ```reference ```` block whose body is a single line of JSON. The raw
//! fence text round-trips losslessly through `raw_fallback`, while the parsed
//! [`ReferenceData`] drives the rendered card. The host app generates the fence
//! with [`reference_fence_markdown`] and routes clicks on the card's
//! [`ReferenceData::target`] through the editor's `OpenReference` event.

use serde::{Deserialize, Serialize};

/// Structured payload of a reference-embed block.
///
/// `kind` is a free-form discriminator the host understands ("design", "doc",
/// "file"); `target` is the opaque routing string the host resolves on click
/// (e.g. `ref:design:<id>`). Everything else is presentation: optional preview
/// image, badge, status, and tags rendered on the card.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceData {
    pub kind: String,
    pub title: String,
    pub target: String,
    /// Muted secondary line on the compact row (e.g. a relative path or URL).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_label: Option<String>,
}

/// Returns true when a fenced code info string declares reference content.
pub(crate) fn is_reference_info_string(info: Option<&str>) -> bool {
    info.and_then(|info| info.split_whitespace().next())
        .is_some_and(|first| first.eq_ignore_ascii_case("reference"))
}

/// Whether `line` is a bare fence terminator (``` ``` ``` or `~~~`).
fn is_bare_closing_fence(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(marker) = trimmed.chars().next() else {
        return false;
    };
    if marker != '`' && marker != '~' {
        return false;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    len >= 3 && trimmed[marker.len_utf8() * len..].trim().is_empty()
}

/// Parse raw fenced Markdown into the [`ReferenceData`] it carries, or `None`
/// when the fence isn't a reference block / the body isn't valid JSON.
pub(crate) fn parse_reference_fence(raw: &str) -> Option<ReferenceData> {
    let raw = raw.trim_matches('\n');
    let mut lines = raw.split('\n');
    let opening = lines.next()?.trim_end();
    let opening = opening.trim_start();
    let marker = opening.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = opening.chars().take_while(|ch| *ch == marker).count();
    if len < 3 {
        return None;
    }
    let info = opening[marker.len_utf8() * len..].trim();
    if !is_reference_info_string((!info.is_empty()).then_some(info)) {
        return None;
    }

    let rest: Vec<&str> = lines.collect();
    let closing_index = rest.iter().rposition(|line| is_bare_closing_fence(line))?;
    let body = rest[..closing_index].join("\n");
    serde_json::from_str::<ReferenceData>(body.trim()).ok()
}

/// Build the fenced Markdown that encodes a reference embed. The body is a
/// single line of JSON so embedded backticks can never form a closing fence.
#[allow(dead_code, reason = "public standalone serialization helper")]
pub fn reference_fence_markdown(data: &ReferenceData) -> String {
    let json = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    format!("```reference\n{json}\n```")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ReferenceData {
        ReferenceData {
            kind: "design".to_string(),
            title: "Checkout redesign".to_string(),
            target: "ref:design:abc123".to_string(),
            subtitle: Some("figma.com/file/checkout".to_string()),
            preview: Some("/abs/preview.png".to_string()),
            badge: Some("Figma".to_string()),
            status: Some("In review".to_string()),
            tags: vec!["checkout".to_string(), "mobile".to_string()],
            open_label: Some("Open in Figma".to_string()),
        }
    }

    #[test]
    fn round_trips_through_fence() {
        let data = sample();
        let markdown = reference_fence_markdown(&data);
        let parsed = parse_reference_fence(&markdown).expect("parse");
        assert_eq!(parsed, data);
    }

    #[test]
    fn detects_reference_info_string() {
        assert!(is_reference_info_string(Some("reference")));
        assert!(is_reference_info_string(Some("Reference")));
        assert!(!is_reference_info_string(Some("mermaid")));
        assert!(!is_reference_info_string(None));
    }

    #[test]
    fn rejects_non_reference_fence() {
        assert!(parse_reference_fence("```mermaid\nflowchart LR\n```").is_none());
        assert!(parse_reference_fence("plain text").is_none());
    }

    #[test]
    fn title_with_backtick_stays_single_line() {
        let mut data = sample();
        data.title = "weird ``` title".to_string();
        let markdown = reference_fence_markdown(&data);
        // The JSON body is one physical line, so the stray backticks never form
        // a standalone closing fence.
        assert_eq!(markdown.lines().count(), 3);
        let parsed = parse_reference_fence(&markdown).expect("parse");
        assert_eq!(parsed.title, data.title);
    }
}
