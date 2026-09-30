//! Shared readable skill instructions for Settings pages.
use gpui_component::text::TextViewStyle;

/// The catalog already presents title, source and license. YAML is metadata,
/// not part of the readable instructions. Leave malformed/non-leading fences alone.
pub(super) fn skill_document_body(content: &str) -> &str {
    let source = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = source.split_inclusive('\n');
    if lines.next().map(str::trim_end) != Some("---") {
        return content;
    }
    let mut offset = source.len() - lines.clone().map(str::len).sum::<usize>();
    for line in lines {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return source[offset..].trim_start_matches(['\r', '\n']);
        }
    }
    content
}

pub(super) fn skill_markdown_style() -> TextViewStyle {
    TextViewStyle::default()
        .paragraph_gap(gpui::rems(0.6))
        .heading_font_size(|level, _| match level {
            1 => crate::ui::design::text_head(),
            _ => crate::ui::design::text_body(),
        })
}
