use super::*;
use gpui::{HighlightStyle, StyledText};
use gpui_component::{
    highlighter::{Language, SyntaxHighlighter},
    Rope,
};

const CHAT_CODE_HIGHLIGHT_MAX_BYTES: usize = 50 * 1024;
const CHAT_CODE_HIGHLIGHT_MAX_LINES: usize = 500;
const CHAT_LIST_INDENT_PX: f32 = 16.0;
const CHAT_LIST_MAX_DEPTH: usize = 6;

pub(super) fn chat_message_text_style() -> TextViewStyle {
    TextViewStyle::default().paragraph_gap(rems(crate::ui::design::CHAT_PARAGRAPH_GAP_REMS))
}

#[derive(Clone, Copy)]
enum ChatTextRole {
    Body,
    Heading,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ChatListItem {
    markdown: String,
    depth: usize,
}

pub(super) fn chat_message_display_markdown(text: &str) -> String {
    text.lines()
        .map(|line| {
            if let Some(path) = standalone_image_path_from_text(line) {
                return format!("![Generated image]({})", path.display());
            }
            if parse_chat_image_block(line).is_some() {
                return line.to_string();
            }
            let trimmed = line.trim_start();
            let leading = &line[..line.len() - trimmed.len()];
            if let Some(item) = markdown_list_item(trimmed.trim_end()) {
                format!("{leading}- {}", format_chat_list_item(item))
            } else {
                normalize_local_markdown_links(line)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn normalize_local_markdown_links(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;

    while let Some(open) = rest.find('[') {
        output.push_str(&rest[..open]);
        let after_open = &rest[open + 1..];
        let Some(close) = after_open.find(']') else {
            output.push_str(&rest[open..]);
            return output;
        };
        let label = &after_open[..close];
        let after_label = &after_open[close + 1..];
        if !after_label.starts_with('(') {
            output.push_str(&rest[open..open + close + 2]);
            rest = after_label;
            continue;
        }
        let Some(target_close) = after_label[1..].find(')') else {
            output.push_str(&rest[open..]);
            return output;
        };
        let target = &after_label[1..1 + target_close];
        let consumed = open + 1 + close + 1 + 1 + target_close + 1;

        if is_local_code_link(label, target) {
            output.push('`');
            output.push_str(label);
            output.push('`');
        } else {
            output.push_str(&rest[open..consumed]);
        }
        rest = &rest[consumed..];
    }

    output.push_str(rest);
    output
}

fn is_local_code_link(label: &str, target: &str) -> bool {
    let lower_label = label.to_ascii_lowercase();
    let looks_like_file = [".html", ".css", ".js", ".ts", ".tsx", ".rs", ".json", ".md"]
        .iter()
        .any(|extension| lower_label.ends_with(extension));
    looks_like_file
        || label.contains('/')
        || target.starts_with('/')
        || target.starts_with("./")
        || target.starts_with("../")
}

fn format_chat_list_item(item: &str) -> String {
    let normalized = normalize_local_markdown_links(item);
    let plain = strip_inline_markdown(&normalized);
    // A Markdown link target naturally contains `/`. Do not mistake that URL
    // for a file path and wrap the whole link in `**...**`: the Markdown text
    // component cannot retain the nested link mark in that shape, leaving only
    // bold, non-clickable label text.
    if !contains_inline_markdown_link(&normalized) && is_file_label(plain.trim()) {
        format!("**{}**", plain.trim())
    } else {
        normalized
    }
}

fn contains_inline_markdown_link(markdown: &str) -> bool {
    markdown
        .char_indices()
        .filter(|(_, ch)| *ch == ']')
        .any(|(index, _)| markdown[index + 1..].starts_with('('))
}

fn is_file_label(label: &str) -> bool {
    let lower_label = label.to_ascii_lowercase();
    [".html", ".css", ".js", ".ts", ".tsx", ".rs", ".json", ".md"]
        .iter()
        .any(|extension| lower_label.ends_with(extension))
        || label.contains('/')
}

/// Inline file references read more elegantly as light bold text than as a grey
/// code chip — the chip background (`theme().accent`, hardcoded in the markdown
/// component) is heavier than a flowing filename needs. We can only restyle this
/// for spans we recognise as file paths, so other inline code stays code.
/// Applied only to paragraph/list text, never to fenced code blocks.
fn soften_file_code_spans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('`') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let content = &after[..close];
        if is_file_label(content.trim()) && !content.contains(' ') {
            out.push_str("**");
            out.push_str(content);
            out.push_str("**");
        } else {
            out.push('`');
            out.push_str(content);
            out.push('`');
        }
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    out
}

pub(super) fn clean_plan_heading(text: &str) -> String {
    strip_inline_markdown(text)
        .trim_start_matches("Plan:")
        .trim()
        .to_string()
}

pub(super) fn render_chat_message_markdown(
    text: &str,
    element_seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    render_chat_blocks(text, None, None, element_seed, window, cx)
}

pub(super) fn render_agent_chat_message_markdown(
    text: &str,
    visualization: &agent_chat_visualization::ChatVisualizationRenderContext,
    element_seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    render_chat_blocks(text, None, Some(visualization), element_seed, window, cx)
}

/// Like [`render_chat_message_markdown`], but for the message currently
/// streaming: `text` is the revealed prefix, and the last `warm_chars`
/// characters of the trailing paragraph are tinted lavender at `warmth`
/// (0..=1), cooling to the normal text colour — the live "writing" edge.
pub(super) fn render_streaming_chat_message_markdown(
    text: &str,
    warm_chars: usize,
    warmth: f32,
    element_seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let warm = (warm_chars > 0 && warmth > 0.0).then_some((warm_chars, warmth));
    render_chat_blocks(text, warm, None, element_seed, window, cx)
}

pub(super) fn render_streaming_agent_chat_message_markdown(
    text: &str,
    warm_chars: usize,
    warmth: f32,
    visualization: &agent_chat_visualization::ChatVisualizationRenderContext,
    element_seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let warm = (warm_chars > 0 && warmth > 0.0).then_some((warm_chars, warmth));
    render_chat_blocks(text, warm, Some(visualization), element_seed, window, cx)
}

fn render_chat_blocks(
    text: &str,
    warm: Option<(usize, f32)>,
    visualization: Option<&agent_chat_visualization::ChatVisualizationRenderContext>,
    element_seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let normalized = chat_message_display_markdown(text);
    let lines = normalized.lines().collect::<Vec<_>>();
    let list_indent_unit = markdown_list_indent_unit(&lines);
    let mut elements = Vec::new();
    let mut ix = 0;

    while ix < lines.len() {
        let raw = lines[ix];
        let line = raw.trim();

        if line.is_empty() {
            ix += 1;
            continue;
        }

        if line.starts_with("```") {
            let language = line
                .trim_start_matches("```")
                .split_whitespace()
                .next()
                .filter(|value| !value.is_empty())
                .map(str::to_string);
            ix += 1;
            let mut code = Vec::new();
            while ix < lines.len() && !lines[ix].trim_start().starts_with("```") {
                code.push(lines[ix]);
                ix += 1;
            }
            if ix < lines.len() {
                ix += 1;
            }
            elements.push(render_chat_code_block(
                code.join("\n"),
                language,
                element_seed,
                elements.len(),
                cx,
            ));
            continue;
        }

        if let (Some(visualization), Some(path)) = (
            visualization,
            agent_chat_visualization::parse_visualization_directive_line(line),
        ) {
            elements.push(agent_chat_visualization::render_chat_visualization_card(
                visualization,
                &path,
                element_seed.wrapping_add(elements.len() as u64),
                cx,
            ));
            ix += 1;
            continue;
        }

        if let Some((header, mut rows, consumed)) = markdown_table(&lines, ix) {
            let mut next_ix = ix + consumed;
            let mut live_table = warm.filter(|_| trailing_lines_are_empty(&lines, next_ix));
            if live_table.is_none() {
                if let Some((partial_row, live)) =
                    streaming_table_row(header.len(), &lines, next_ix, warm)
                {
                    rows.push(partial_row);
                    next_ix += 1;
                    live_table = Some(live);
                }
            }
            elements.push(render_markdown_table(
                &header,
                &rows,
                live_table,
                element_seed.wrapping_add(elements.len() as u64),
                window,
                cx,
            ));
            ix = next_ix;
            continue;
        }

        if let Some((alt, path)) = parse_chat_image_block(line) {
            elements.push(render_chat_image_block(
                alt,
                path,
                element_seed,
                elements.len(),
                cx,
            ));
            ix += 1;
            continue;
        }

        if let Some((_, heading)) = markdown_heading(line) {
            ix += 1;
            let markdown = format!("**{}**", strip_inline_markdown(heading));
            match warm.filter(|_| trailing_lines_are_empty(&lines, ix)) {
                Some((warm_chars, warmth)) => {
                    elements.push(render_warm_heading_block(markdown, warm_chars, warmth, cx))
                }
                None => elements.push(render_chat_text_block(
                    markdown,
                    ChatTextRole::Heading,
                    element_seed,
                    elements.len(),
                    window,
                    cx,
                )),
            }
            continue;
        }

        if markdown_list_item(line).is_some() {
            let (items, next_ix) = collect_chat_list_items(&lines, ix, list_indent_unit);
            ix = next_ix;
            // Only the final item in the final visible list is still being
            // written. Paint it directly so it follows the reveal every frame;
            // earlier items switch once to their complete Markdown view as soon
            // as the next bullet begins.
            let live_item = streaming_list_item(items.len(), &lines, ix, warm);
            elements.push(render_chat_list_block(
                items,
                live_item,
                element_seed,
                elements.len(),
                window,
                cx,
            ));
            continue;
        }

        let mut paragraph = vec![line.to_string()];
        ix += 1;
        while ix < lines.len() {
            let next = lines[ix].trim();
            if next.is_empty()
                || next.starts_with("```")
                || parse_chat_image_block(next).is_some()
                || (visualization.is_some()
                    && agent_chat_visualization::parse_visualization_directive_line(next).is_some())
                || markdown_table(&lines, ix).is_some()
                || markdown_heading(next).is_some()
                || markdown_list_item(next).is_some()
            {
                break;
            }
            paragraph.push(next.to_string());
            ix += 1;
        }
        // The trailing paragraph of a streaming message gets the warm live edge;
        // everything else renders as normal markdown.
        let is_final = trailing_lines_are_empty(&lines, ix);
        match warm.filter(|_| is_final) {
            Some((warm_chars, warmth)) => elements.push(render_warm_text_block(
                paragraph.join(" "),
                warm_chars,
                warmth,
                cx,
            )),
            None => elements.push(render_chat_text_block(
                paragraph.join(" "),
                ChatTextRole::Body,
                element_seed,
                elements.len(),
                window,
                cx,
            )),
        }
    }

    v_flex()
        .w_full()
        .min_w(px(0.))
        .gap_3()
        .children(elements)
        .into_any_element()
}

fn render_chat_image_block(
    _alt: String,
    path: PathBuf,
    element_seed: u64,
    block_index: usize,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let preview_path = path.clone();

    div()
        .id((
            "agent-chat-image-block",
            element_seed.wrapping_add(block_index as u64),
        ))
        .w_full()
        .max_w(px(520.))
        .h(px(320.))
        .flex()
        .items_center()
        .justify_start()
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, window, cx| {
            this.open_agent_image_preview(preview_path.clone(), window, cx);
        }))
        .child(
            img(path.clone())
                .max_w_full()
                .max_h_full()
                .object_fit(ObjectFit::Contain)
                .with_fallback(|| div().into_any_element()),
        )
        .into_any_element()
}

fn render_chat_text_block(
    markdown: String,
    role: ChatTextRole,
    element_seed: u64,
    block_index: usize,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let (color, weight) = match role {
        ChatTextRole::Body => (crate::ui::design::chat_body(cx), gpui::FontWeight::NORMAL),
        ChatTextRole::Heading => (crate::ui::design::t1(cx), gpui::FontWeight::MEDIUM),
    };
    div()
        .w_full()
        .min_w(px(0.))
        .text_size(crate::ui::design::text_body())
        .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
        .font_weight(weight)
        .text_color(color)
        .child(
            TextView::markdown(
                (
                    "agent-chat-message-block",
                    element_seed.wrapping_add(block_index as u64),
                ),
                soften_file_code_spans(&markdown),
                window,
                cx,
            )
            .selectable(true)
            .style(chat_message_text_style()),
        )
        .into_any_element()
}

/// The trailing paragraph of a streaming message, rendered as plain styled text
/// (not markdown) so the last `warm_chars` characters can each be tinted
/// lavender — the newest character warmest, cooling toward the normal text
/// colour. Inline markdown in this one in-progress paragraph resolves to full
/// formatting once it becomes a completed block or the turn ends.
fn render_warm_text_block(
    markdown: String,
    warm_chars: usize,
    warmth: f32,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let text_color = crate::ui::design::chat_body(cx);

    div()
        .w_full()
        .min_w(px(0.))
        .text_size(crate::ui::design::text_body())
        .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
        .text_color(text_color)
        .child(warm_styled_text(&markdown, warm_chars, warmth, cx))
        .into_any_element()
}

/// A heading that is still being revealed must not reuse a keyed Markdown
/// parser: its delayed updates can leave the first few letters painted after
/// the rest of the heading is already available. Once another block appears,
/// the completed heading mounts its normal Markdown view exactly once.
fn render_warm_heading_block(
    markdown: String,
    warm_chars: usize,
    warmth: f32,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    div()
        .w_full()
        .min_w(px(0.))
        .text_size(crate::ui::design::text_body())
        .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(crate::ui::design::t1(cx))
        .child(warm_styled_text(&markdown, warm_chars, warmth, cx))
        .into_any_element()
}

fn warm_styled_text(
    markdown: &str,
    warm_chars: usize,
    warmth: f32,
    cx: &mut Context<CenterArea>,
) -> StyledText {
    const WARM_MAX: f32 = 0.9;
    let plain = strip_inline_markdown(&markdown);
    let text_color = crate::ui::design::chat_body(cx);
    let warm_color: gpui::Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_BRIGHT).into();

    let offsets = plain
        .char_indices()
        .map(|(byte, _)| byte)
        .collect::<Vec<_>>();
    let total = offsets.len();
    let warm_start = total.saturating_sub(warm_chars);
    let region = (total - warm_start).max(1) as f32;
    let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
    for i in warm_start..total {
        // Nearest the end is warmest (weight → 1); it cools over the region.
        let weight = (((i - warm_start + 1) as f32 / region) * warmth * WARM_MAX).clamp(0.0, 1.0);
        if weight <= 0.02 {
            continue;
        }
        let start = offsets[i];
        let end = offsets.get(i + 1).copied().unwrap_or(plain.len());
        highlights.push((
            start..end,
            HighlightStyle {
                color: Some(blend(text_color, warm_color, weight)),
                ..Default::default()
            },
        ));
    }

    StyledText::new(plain).with_highlights(highlights)
}

/// Linear blend from `a` toward `b` by `t` (0..=1) in RGB — avoids hue-wrap
/// artefacts when one endpoint is near-white.
fn blend(a: gpui::Hsla, b: gpui::Hsla, t: f32) -> gpui::Hsla {
    let (a, b) = (a.to_rgb(), b.to_rgb());
    let t = t.clamp(0.0, 1.0);
    let mix = |x: f32, y: f32| x + (y - x) * t;
    gpui::Rgba {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: mix(a.a, b.a),
    }
    .into()
}

fn render_chat_list_block(
    items: Vec<ChatListItem>,
    live_item: Option<(usize, usize, f32)>,
    element_seed: u64,
    block_index: usize,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let mut rows = Vec::new();
    for (item_index, item) in items.into_iter().enumerate() {
        let body = match live_item.filter(|live| should_render_live_list_item(item_index, *live)) {
            Some((_, warm_chars, warmth)) => {
                warm_styled_text(&item.markdown, warm_chars, warmth, cx).into_any_element()
            }
            None => TextView::markdown(
                (
                    "agent-chat-message-list-item",
                    element_seed
                        .wrapping_add((block_index as u64).saturating_mul(1000))
                        .wrapping_add(item_index as u64),
                ),
                soften_file_code_spans(&item.markdown),
                window,
                cx,
            )
            .selectable(true)
            .style(chat_message_text_style())
            .into_any_element(),
        };
        rows.push(
            h_flex()
                .w_full()
                .pl(px(item.depth as f32 * CHAT_LIST_INDENT_PX))
                .items_start()
                .gap_2()
                .child(
                    div().pt(px(7.)).child(
                        div()
                            .size(px(4.))
                            .rounded_full()
                            .bg(crate::ui::design::t3(cx)),
                    ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .text_size(crate::ui::design::text_body())
                        .line_height(gpui::relative(crate::ui::design::CHAT_LIST_LINE_HEIGHT))
                        .text_color(crate::ui::design::chat_body(cx))
                        .child(body),
                )
                .into_any_element(),
        );
    }

    v_flex()
        .w_full()
        .min_w(px(0.))
        .gap_2()
        .children(rows)
        .into_any_element()
}

fn trailing_lines_are_empty(lines: &[&str], start: usize) -> bool {
    lines
        .get(start..)
        .map_or(true, |rest| rest.iter().all(|line| line.trim().is_empty()))
}

fn markdown_list_indent_columns(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

/// Infer the document's nesting unit from its smallest non-zero list indent.
/// Models commonly emit either two- or four-space Markdown; treating both as
/// one level keeps the visual hierarchy faithful without hard-coding one style.
fn markdown_list_indent_unit(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|line| markdown_list_item(line.trim_start()).is_some())
        .map(|line| markdown_list_indent_columns(line))
        .filter(|columns| *columns > 0)
        .min()
        .unwrap_or(2)
}

fn markdown_list_depth(line: &str, indent_unit: usize) -> usize {
    markdown_list_indent_columns(line)
        .checked_div(indent_unit.max(1))
        .unwrap_or(0)
        .min(CHAT_LIST_MAX_DEPTH)
}

/// Collect one visual list across blank separator lines. Markdown authors often
/// place an empty line before a nested group; keeping those rows in one block
/// preserves the compact parent/child rhythm instead of introducing a section
/// gap at every level.
fn collect_chat_list_items(
    lines: &[&str],
    start: usize,
    indent_unit: usize,
) -> (Vec<ChatListItem>, usize) {
    let mut items = Vec::new();
    let mut cursor = start;
    while cursor < lines.len() {
        let raw = lines[cursor];
        if raw.trim().is_empty() {
            let mut next = cursor + 1;
            while next < lines.len() && lines[next].trim().is_empty() {
                next += 1;
            }
            if next < lines.len() && markdown_list_item(lines[next].trim()).is_some() {
                cursor = next;
                continue;
            }
            break;
        }

        let Some(markdown) = markdown_list_item(raw.trim()) else {
            break;
        };
        items.push(ChatListItem {
            markdown: markdown.to_string(),
            depth: markdown_list_depth(raw, indent_unit),
        });
        cursor += 1;
    }
    (items, cursor)
}

fn streaming_list_item(
    item_count: usize,
    lines: &[&str],
    next_line: usize,
    warm: Option<(usize, f32)>,
) -> Option<(usize, usize, f32)> {
    let (warm_chars, warmth) = warm?;
    if !trailing_lines_are_empty(lines, next_line) {
        return None;
    }
    Some((item_count.checked_sub(1)?, warm_chars, warmth))
}

/// Recognise the single unfinished row at the live edge of a table. A model
/// stream commonly ends mid-cell, so the normal table parser (which accepts
/// only complete rows) would otherwise finalise the table too early and mount
/// keyed Markdown cells that keep repainting stale fragments.
fn streaming_table_row(
    header_len: usize,
    lines: &[&str],
    next_line: usize,
    warm: Option<(usize, f32)>,
) -> Option<(Vec<String>, (usize, f32))> {
    let live = warm?;
    let line = lines.get(next_line)?.trim();
    if !line.contains('|') || !trailing_lines_are_empty(lines, next_line + 1) {
        return None;
    }
    let mut cells = markdown_table_cells(line);
    trim_surplus_empty_table_cells(&mut cells, header_len);
    if cells.is_empty() || cells.len() > header_len {
        return None;
    }
    cells.resize(header_len, String::new());
    Some((cells, live))
}

fn should_render_live_list_item(item_index: usize, live_item: (usize, usize, f32)) -> bool {
    item_index == live_item.0
}

fn render_chat_code_block(
    code: String,
    language: Option<String>,
    element_seed: u64,
    block_index: usize,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let copy_code = code.clone();
    let normalized_language = language.as_deref().and_then(normalize_chat_code_language);
    let label = language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or(normalized_language)
        .unwrap_or("code")
        .to_string();
    let highlights = normalized_language
        .filter(|language| should_highlight_chat_code(&code, language))
        .map(|language| highlight_chat_code(&code, language, cx))
        .unwrap_or_default();
    let block_bg: gpui::Hsla = crate::ui::style::surface(cx);
    let header_bg: gpui::Hsla = crate::ui::design::surface(cx);
    let border: gpui::Hsla = crate::ui::design::line(cx);
    let code_text: gpui::Hsla = crate::ui::style::focus_text(cx);
    let muted_text: gpui::Hsla = crate::ui::design::t3(cx);

    v_flex()
        .id((
            "agent-chat-code-block",
            element_seed.wrapping_add(block_index as u64),
        ))
        .w_full()
        .min_w(px(0.))
        // x-only for the same reason as the markdown table: a non-`Visible`
        // `overflow.y` zeroes the automatic minimum height and lets the chat
        // list measure this block short, clipping its last code lines.
        .overflow_x_hidden()
        .rounded(px(crate::ui::style::RADIUS_LG))
        .border_1()
        .border_color(border)
        .bg(block_bg)
        .shadow_sm()
        .child(
            h_flex()
                .h(px(36.))
                .items_center()
                // gpui's overflow clip mask is rectangular, so the parent's
                // rounded corners don't clip this header's fill — round its own
                // top corners (inset 1px to nest inside the border).
                .rounded_t(px(crate::ui::style::RADIUS_LG - 1.))
                .border_b_1()
                .border_color(border)
                .bg(header_bg)
                .px_3()
                .gap_2()
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_color(muted_text)
                        .child(label),
                )
                .child(div().flex_1())
                .child(
                    Button::new((
                        "copy-agent-code-block",
                        element_seed.wrapping_add(block_index as u64),
                    ))
                    .ghost()
                    .xsmall()
                    .compact()
                    .h(crate::ui::design::control_h_xs())
                    .icon(IconName::Copy)
                    .tooltip("Copy code")
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_code.clone()));
                    }),
                ),
        )
        .child(
            div()
                .w_full()
                .overflow_x_scrollbar()
                .restrict_scroll_to_axis()
                .px_4()
                .py_3()
                .child(
                    div()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_size(crate::ui::design::text_ui())
                        .line_height(gpui::relative(1.62))
                        .whitespace_nowrap()
                        .text_color(code_text)
                        .child(StyledText::new(code).with_highlights(highlights)),
                ),
        )
        .into_any_element()
}

fn should_highlight_chat_code(code: &str, language: &str) -> bool {
    language != "text"
        && code.len() <= CHAT_CODE_HIGHLIGHT_MAX_BYTES
        && code.lines().take(CHAT_CODE_HIGHLIGHT_MAX_LINES + 1).count()
            <= CHAT_CODE_HIGHLIGHT_MAX_LINES
}

thread_local! {
    // Resolved syntax highlights keyed by (language, code). Tree-sitter parsing
    // is far too expensive to redo on every scroll frame, and code blocks
    // re-render constantly — so we memoize the styles. The leading `u64` is a
    // theme fingerprint; the whole cache is dropped when the theme changes.
    static CHAT_CODE_HIGHLIGHTS: std::cell::RefCell<(
        u64,
        std::collections::HashMap<u64, Vec<(Range<usize>, HighlightStyle)>>,
    )> = std::cell::RefCell::new((0, std::collections::HashMap::new()));
}

fn highlight_chat_code(
    code: &str,
    language: &str,
    cx: &mut Context<CenterArea>,
) -> Vec<(Range<usize>, HighlightStyle)> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    language.hash(&mut hasher);
    code.hash(&mut hasher);
    let key = hasher.finish();

    let mix = |c: gpui::Hsla| -> u64 {
        (c.h.to_bits() as u64).rotate_left(13)
            ^ (c.s.to_bits() as u64).rotate_left(7)
            ^ (c.l.to_bits() as u64)
            ^ (c.a.to_bits() as u64).rotate_left(21)
    };
    let fingerprint =
        mix(crate::ui::design::t1(cx)) ^ mix(crate::ui::design::base(cx)).rotate_left(31);

    if let Some(cached) = CHAT_CODE_HIGHLIGHTS.with(|cache| {
        let cache = cache.borrow();
        (cache.0 == fingerprint)
            .then(|| cache.1.get(&key).cloned())
            .flatten()
    }) {
        return cached;
    }

    let rope = Rope::from_str(code);
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &rope);
    let styles = highlighter.styles(&(0..code.len()), &cx.theme().highlight_theme);

    CHAT_CODE_HIGHLIGHTS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.0 != fingerprint {
            cache.0 = fingerprint;
            cache.1.clear();
        }
        if cache.1.len() > 512 {
            cache.1.clear();
        }
        cache.1.insert(key, styles.clone());
    });

    styles
}

fn normalize_chat_code_language(language: &str) -> Option<&'static str> {
    let normalized = language.trim().trim_start_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let language = match normalized.as_str() {
        "text" | "txt" | "plain" | "plaintext" | "log" | "env" | "dotenv" | "ini" => "text",
        "rust" | "rs" => "rust",
        "javascript" | "js" | "jsx" | "mjs" | "cjs" | "node" => "javascript",
        "typescript" | "ts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" => "json",
        "bash" | "sh" | "zsh" | "shell" | "console" | "terminal" => "bash",
        "css" | "scss" | "sass" => "css",
        "html" | "htm" | "xml" | "svg" | "vue" | "svelte" => "html",
        "markdown" | "md" | "mdx" => "markdown",
        "python" | "py" => "python",
        "ruby" | "rb" => "ruby",
        "go" | "golang" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        "csharp" | "cs" => "csharp",
        "swift" => "swift",
        "sql" => "sql",
        "yaml" | "yml" => "yaml",
        "toml" => "toml",
        "diff" | "patch" => "diff",
        "cmake" => "cmake",
        "make" | "makefile" => "make",
        "proto" | "protobuf" => "proto",
        "graphql" | "gql" => "graphql",
        "elixir" | "ex" | "exs" => "elixir",
        "scala" => "scala",
        "zig" => "zig",
        other if Language::from_str(other).name() != "text" => Language::from_str(other).name(),
        _ => return None,
    };

    Some(language)
}

fn parse_chat_image_block(line: &str) -> Option<(String, PathBuf)> {
    let line = line.trim();
    if !line.starts_with("![") {
        return None;
    }
    let alt_end = line[2..].find(']')? + 2;
    let alt = line[2..alt_end].to_string();
    let rest = line[alt_end + 1..].trim_start();
    if !rest.starts_with('(') || !rest.ends_with(')') {
        return None;
    }
    let target = rest[1..rest.len() - 1]
        .split_once(" \"")
        .map(|(target, _)| target)
        .unwrap_or(&rest[1..rest.len() - 1])
        .trim();
    let path = local_image_path_from_target(target)?;
    Some((alt, path))
}

fn standalone_image_path_from_text(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim().trim_matches(['"', '\'', '`']);
    let candidate = trimmed
        .strip_prefix("file://")
        .unwrap_or(trimmed)
        .trim_end_matches([',', ')', ']']);
    local_image_path_from_target(candidate)
}

fn local_image_path_from_target(target: &str) -> Option<PathBuf> {
    let target = target.trim().trim_matches(['"', '\'', '`']);
    let target = target.strip_prefix("file://").unwrap_or(target);
    let path = PathBuf::from(target);
    if !path.is_absolute() || !path.exists() || image_format_for_path(&path).is_none() {
        return None;
    }
    Some(path)
}

pub(super) fn render_plan_markdown(
    markdown: &str,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    // Plan/verification bodies carry no element seed of their own, so tables
    // key their selectable cells off the content they render.
    let element_seed = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        markdown.hash(&mut hasher);
        hasher.finish()
    };
    let lines = markdown.lines().collect::<Vec<_>>();
    let list_indent_unit = markdown_list_indent_unit(&lines);
    let mut elements = Vec::new();
    let mut ix = 0;

    while ix < lines.len() {
        let raw = lines[ix];
        let line = raw.trim();

        if line.is_empty() {
            ix += 1;
            continue;
        }

        if line.starts_with("```") {
            ix += 1;
            let mut code = Vec::new();
            while ix < lines.len() && !lines[ix].trim_start().starts_with("```") {
                code.push(lines[ix]);
                ix += 1;
            }
            if ix < lines.len() {
                ix += 1;
            }
            elements.push(
                div()
                    .w_full()
                    .rounded(crate::ui::design::r_sm())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::style::surface(cx))
                    .px_3()
                    .py_2()
                    .text_size(crate::ui::design::text_ui())
                    .font_family(crate::ui::design::FONT_MONO)
                    .line_height(gpui::relative(1.45))
                    .text_color(crate::ui::style::focus_text(cx))
                    .child(code.join("\n"))
                    .into_any_element(),
            );
            continue;
        }

        if let Some((header, rows, consumed)) = markdown_table(&lines, ix) {
            elements.push(render_markdown_table(
                &header,
                &rows,
                None,
                element_seed.wrapping_add(elements.len() as u64),
                window,
                cx,
            ));
            ix += consumed;
            continue;
        }

        if let Some((level, heading)) = markdown_heading(line) {
            let size = if level <= 2 { px(16.) } else { px(14.) };
            elements.push(
                div()
                    .pt(if elements.is_empty() { px(0.) } else { px(12.) })
                    .text_size(size)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .line_height(gpui::relative(1.25))
                    .text_color(crate::ui::style::focus_text(cx))
                    .child(strip_inline_markdown(heading))
                    .into_any_element(),
            );
            ix += 1;
            continue;
        }

        if let Some(item) = markdown_list_item(line) {
            elements.push(
                h_flex()
                    .w_full()
                    .pl(px(
                        markdown_list_depth(raw, list_indent_unit) as f32 * CHAT_LIST_INDENT_PX
                    ))
                    .items_start()
                    .gap_2()
                    .text_size(crate::ui::design::text_body())
                    .line_height(gpui::relative(1.42))
                    .text_color(crate::ui::style::focus_text(cx))
                    .child(
                        div().pt(px(7.)).child(
                            div()
                                .size(px(4.))
                                .rounded_full()
                                .bg(crate::ui::design::t3(cx)),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(strip_inline_markdown(item)),
                    )
                    .into_any_element(),
            );
            ix += 1;
            continue;
        }

        let mut paragraph = vec![line.to_string()];
        ix += 1;
        while ix < lines.len() {
            let next = lines[ix].trim();
            if next.is_empty()
                || next.starts_with("```")
                || markdown_table(&lines, ix).is_some()
                || markdown_heading(next).is_some()
                || markdown_list_item(next).is_some()
            {
                break;
            }
            paragraph.push(next.to_string());
            ix += 1;
        }
        elements.push(
            div()
                .text_size(crate::ui::design::text_body())
                .line_height(gpui::relative(1.48))
                .text_color(crate::ui::style::focus_text(cx))
                .child(strip_inline_markdown(&paragraph.join(" ")))
                .into_any_element(),
        );
    }

    v_flex()
        .w_full()
        .min_w(px(0.))
        .gap_2()
        .children(elements)
        .into_any_element()
}

pub(super) fn markdown_table(
    lines: &[&str],
    start: usize,
) -> Option<(Vec<String>, Vec<Vec<String>>, usize)> {
    let header_line = lines.get(start)?.trim();
    let separator_line = lines.get(start + 1)?.trim();
    if !header_line.contains('|') || !separator_line.contains('|') {
        return None;
    }
    let header = markdown_table_cells(header_line);
    let separator = markdown_table_cells(separator_line);
    if header.len() < 2
        || separator.len() != header.len()
        || !separator.iter().all(|cell| {
            let trimmed = cell.trim_matches(':').trim();
            trimmed.len() >= 3 && trimmed.chars().all(|ch| ch == '-')
        })
    {
        return None;
    }

    let mut rows = Vec::new();
    let mut consumed = 2;
    while let Some(line) = lines.get(start + consumed).map(|line| line.trim()) {
        if line.is_empty() || !line.contains('|') {
            break;
        }
        let mut cells = markdown_table_cells(line);
        // Models occasionally close a row with `| |`, producing one surplus
        // empty cell after an otherwise valid row. Treat empty overflow as
        // delimiter noise, but never discard a non-empty extra cell.
        trim_surplus_empty_table_cells(&mut cells, header.len());
        if cells.len() != header.len() {
            break;
        }
        rows.push(cells);
        consumed += 1;
    }
    Some((header, rows, consumed))
}

pub(super) fn markdown_table_cells(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn trim_surplus_empty_table_cells(cells: &mut Vec<String>, expected: usize) {
    while cells.len() > expected && cells.last().is_some_and(|cell| cell.trim().is_empty()) {
        cells.pop();
    }
}

/// Inner radius of the table's rounded frame — gpui does not clip children to a
/// parent's rounded rect, so the first and last rows round their own corners.
fn table_inner_radius() -> gpui::Pixels {
    px(crate::ui::design::R_SM - 1.)
}

/// A table cell is rendered as markdown so its inline formatting survives and
/// the text stays selectable — but a cell is a *phrase*, never a block. Escape a
/// leading block marker so a placeholder like `-` or a cell such as `1. First`
/// renders literally instead of collapsing into a bullet, heading or quote.
pub(super) fn table_cell_markdown(cell: &str) -> String {
    let trimmed = cell.trim();
    let leading = &cell[..cell.len() - cell.trim_start().len()];

    let marker_len = if let Some(rest) = trimmed.strip_prefix(['#', '-', '*', '+', '>']) {
        (rest.is_empty() || rest.starts_with(' ')).then_some(1)
    } else {
        let digits = trimmed.chars().take_while(char::is_ascii_digit).count();
        let rest = &trimmed[digits..];
        (digits > 0 && (rest == "." || rest.starts_with(". "))).then_some(digits + 1)
    };

    match marker_len {
        Some(len) => format!(
            "{leading}{}\\{}{}",
            &trimmed[..len - 1],
            &trimmed[len - 1..len],
            &trimmed[len..]
        ),
        None => cell.to_string(),
    }
}

fn render_markdown_table_row(
    cells: &[String],
    is_header: bool,
    is_last: bool,
    streaming: bool,
    live_cell: Option<(usize, usize, f32)>,
    seed: u64,
    row_index: usize,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let divider = crate::ui::design::line(cx);
    let cell_count = cells.len();
    // Not `h_flex()`: that centres its items, and cells need to stretch to the
    // row's height so the column rules run edge to edge.
    let mut row = div()
        .flex()
        .flex_row()
        .w_full()
        .flex_none()
        .when(is_header, |row| {
            row.rounded_t(table_inner_radius())
                .bg(crate::ui::design::surface_2(cx))
        })
        .when(is_last, |row| row.rounded_b(table_inner_radius()))
        // Every row but the last carries the hairline, so the final rule never
        // doubles up with the table's own border.
        .when(!is_last, |row| row.border_b_1().border_color(divider));

    for (index, cell) in cells.iter().enumerate() {
        // The leading column reads as the row's key, so it keeps UI weight
        // while the remaining columns render as prose.
        let is_key_column = index == 0 && !is_header;
        let color = if is_header {
            crate::ui::design::t2(cx)
        } else if is_key_column {
            crate::ui::design::t1_soft(cx)
        } else {
            crate::ui::design::chat_body(cx)
        };
        let weight = if is_header || is_key_column {
            gpui::FontWeight::MEDIUM
        } else {
            gpui::FontWeight::NORMAL
        };

        let cell_body = if streaming {
            match live_cell.filter(|(cell_index, _, _)| *cell_index == index) {
                Some((_, warm_chars, warmth)) => {
                    warm_styled_text(cell, warm_chars, warmth, cx).into_any_element()
                }
                None => StyledText::new(strip_inline_markdown(cell)).into_any_element(),
            }
        } else {
            TextView::markdown(
                (
                    "agent-chat-table-cell",
                    // The seed is scattered first so two tables a few
                    // blocks apart can't land on each other's cell ids.
                    seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(
                        (row_index as u64)
                            .wrapping_mul(1024)
                            .wrapping_add(index as u64),
                    ),
                ),
                table_cell_markdown(&soften_file_code_spans(cell)),
                window,
                cx,
            )
            .selectable(true)
            .into_any_element()
        };

        row = row.child(
            div()
                // A definite width, not `flex_1`. Under `flex: 1 1 0%` a cell's
                // width is only known once flex resolution runs, but the cell's
                // *height* depends on where its text wraps at that width — so
                // the chat list (which measures each row once, at `MinContent`)
                // sized every cell as a single line and left the table short by
                // the height of every wrapped second line. Equal definite
                // columns break that circularity: width is known up front, so
                // the wrap height is correct in every measurement pass.
                .w(gpui::relative(1.0 / cell_count as f32))
                .min_w(px(0.))
                .px_3()
                .py_2()
                .text_size(crate::ui::design::text_ui())
                .line_height(gpui::relative(1.45))
                .font_weight(weight)
                .text_color(color)
                .when(index + 1 < cell_count, |cell| {
                    cell.border_r_1().border_color(divider)
                })
                .child(cell_body),
        );
    }

    row.into_any_element()
}

pub(super) fn render_markdown_table(
    header: &[String],
    rows: &[Vec<String>],
    live: Option<(usize, f32)>,
    seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let streaming = live.is_some();
    let live_row = if rows.is_empty() { 0 } else { rows.len() };
    let live_cell = live.map(|(warm_chars, warmth)| {
        let cells = rows.last().map_or(header, Vec::as_slice);
        let index = cells
            .iter()
            .rposition(|cell| !cell.trim().is_empty())
            .unwrap_or(0);
        (index, warm_chars, warmth)
    });
    let mut table = v_flex()
        .w_full()
        .min_w(px(0.))
        .flex_none()
        .rounded(crate::ui::design::r_sm())
        .border_1()
        .border_color(crate::ui::design::line(cx))
        .bg(crate::ui::style::surface(cx))
        // No `overflow_*` here, deliberately. A gpui `ContentMask` is a rect, so
        // any non-`Visible` overflow clips on *both* axes — there is no x-only
        // clip. The chat list measures each row at `MinContent` and caches the
        // height; masking a row while its live text changes can therefore hide
        // its last line until the next measurement. Rows round their own corners
        // (see `render_markdown_table_row`), which is what the mask was for.
        .child(render_markdown_table_row(
            header,
            true,
            rows.is_empty(),
            streaming,
            (live_row == 0).then_some(live_cell).flatten(),
            seed,
            0,
            window,
            cx,
        ));

    for (index, row) in rows.iter().enumerate() {
        table = table.child(render_markdown_table_row(
            row,
            false,
            index + 1 == rows.len(),
            streaming,
            (live_row == index + 1).then_some(live_cell).flatten(),
            seed,
            index + 1,
            window,
            cx,
        ));
    }

    table.into_any_element()
}

pub(super) fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let heading = line.get(hashes..)?.trim();
    (!heading.is_empty()).then_some((hashes, heading))
}

pub(super) fn markdown_list_item(line: &str) -> Option<&str> {
    if let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(item.trim());
    }
    let dot = line.find(". ")?;
    if dot == 0 || !line[..dot].chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    Some(line[dot + 2..].trim())
}

pub(super) fn strip_inline_markdown(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .replace("\\(", "(")
        .replace("\\)", ")")
}

#[cfg(test)]
mod streaming_list_tests {
    use super::*;

    #[test]
    fn only_the_last_item_of_the_trailing_list_streams_live() {
        let lines = ["- First bullet", "- Second bullet"];

        assert_eq!(
            streaming_list_item(2, &lines, lines.len(), Some((16, 1.0))),
            Some((1, 16, 1.0))
        );
        assert_eq!(streaming_list_item(2, &lines, lines.len(), None), None);
    }

    #[test]
    fn a_list_finalizes_when_later_content_becomes_visible() {
        let lines = ["- Finished bullet", "", "Next paragraph"];

        assert_eq!(streaming_list_item(1, &lines, 1, Some((16, 1.0))), None);
    }

    #[test]
    fn the_live_item_advances_once_per_new_bullet() {
        let mut live_indices = Vec::new();
        for item_count in 1..=3 {
            let lines = vec!["- bullet"; item_count];
            let live =
                streaming_list_item(item_count, &lines, lines.len(), Some((16, 1.0))).unwrap();
            live_indices.push(live.0);
            for item_index in 0..item_count {
                assert_eq!(
                    should_render_live_list_item(item_index, live),
                    item_index + 1 == item_count
                );
            }
        }

        assert_eq!(live_indices, vec![0, 1, 2]);
    }

    #[test]
    fn an_out_of_range_trailing_cursor_degrades_to_no_remaining_content() {
        assert!(trailing_lines_are_empty(&["- bullet"], 2));
    }

    #[test]
    fn nested_bullets_preserve_depth_across_blank_separator_lines() {
        let lines = [
            "- Parent",
            "",
            "  - Child",
            "    - Grandchild",
            "",
            "- Next parent",
            "Paragraph",
        ];
        let indent_unit = markdown_list_indent_unit(&lines);
        let (items, next) = collect_chat_list_items(&lines, 0, indent_unit);

        assert_eq!(indent_unit, 2);
        assert_eq!(
            items,
            vec![
                ChatListItem {
                    markdown: "Parent".to_string(),
                    depth: 0,
                },
                ChatListItem {
                    markdown: "Child".to_string(),
                    depth: 1,
                },
                ChatListItem {
                    markdown: "Grandchild".to_string(),
                    depth: 2,
                },
                ChatListItem {
                    markdown: "Next parent".to_string(),
                    depth: 0,
                },
            ]
        );
        assert_eq!(next, 6);
    }

    #[test]
    fn four_space_markdown_uses_one_visual_level_per_indent() {
        let lines = ["- Parent", "    - Child", "        - Grandchild"];
        let indent_unit = markdown_list_indent_unit(&lines);

        assert_eq!(indent_unit, 4);
        assert_eq!(markdown_list_depth(lines[1], indent_unit), 1);
        assert_eq!(markdown_list_depth(lines[2], indent_unit), 2);
    }

    #[test]
    fn pathological_list_depth_is_bounded() {
        let line = format!("{}- Deep", " ".repeat(200));
        assert_eq!(markdown_list_depth(&line, 2), CHAT_LIST_MAX_DEPTH);
    }

    #[test]
    fn an_unfinished_trailing_table_row_stays_live_and_keeps_all_columns() {
        let lines = [
            "| Name | State | Result |",
            "| --- | --- | --- |",
            "| Choro | Run",
        ];

        let (row, live) = streaming_table_row(3, &lines, 2, Some((16, 1.0))).unwrap();

        assert_eq!(row, vec!["Choro", "Run", ""]);
        assert_eq!(live, (16, 1.0));
    }

    #[test]
    fn a_live_table_row_ignores_a_surplus_empty_trailing_cell() {
        let lines = [
            "| Name | State | Result |",
            "| --- | --- | --- |",
            "| Choro | Ready | Passed | |",
        ];

        let (row, live) = streaming_table_row(3, &lines, 2, Some((4, 0.8))).unwrap();

        assert_eq!(row, vec!["Choro", "Ready", "Passed"]);
        assert_eq!(live, (4, 0.8));
    }

    #[test]
    fn a_table_finalizes_when_non_table_content_follows() {
        let lines = [
            "| Name | State |",
            "| --- | --- |",
            "| Choro | Ready",
            "Final paragraph",
        ];

        assert_eq!(streaming_table_row(2, &lines, 2, Some((16, 1.0))), None);
    }
}
