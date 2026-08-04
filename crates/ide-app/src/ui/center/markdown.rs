use super::*;
use gpui::{HighlightStyle, StyledText};
use gpui_component::{
    highlighter::{Language, SyntaxHighlighter},
    Rope,
};

const CHAT_CODE_HIGHLIGHT_MAX_BYTES: usize = 50 * 1024;
const CHAT_CODE_HIGHLIGHT_MAX_LINES: usize = 500;

pub(super) fn chat_message_text_style() -> TextViewStyle {
    TextViewStyle::default().paragraph_gap(rems(crate::ui::design::CHAT_PARAGRAPH_GAP_REMS))
}

#[derive(Clone, Copy)]
enum ChatTextRole {
    Body,
    Heading,
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
    if is_file_label(plain.trim()) {
        format!("**{}**", plain.trim())
    } else {
        normalized
    }
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

        if let Some((header, rows, consumed)) = markdown_table(&lines, ix) {
            elements.push(render_markdown_table(
                &header,
                &rows,
                element_seed.wrapping_add(elements.len() as u64),
                window,
                cx,
            ));
            ix += consumed;
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
            elements.push(render_chat_text_block(
                format!("**{}**", strip_inline_markdown(heading)),
                ChatTextRole::Heading,
                element_seed,
                elements.len(),
                window,
                cx,
            ));
            ix += 1;
            continue;
        }

        if markdown_list_item(line).is_some() {
            let mut items = Vec::new();
            while ix < lines.len() {
                let next = lines[ix].trim();
                let Some(item) = markdown_list_item(next) else {
                    break;
                };
                items.push(item.to_string());
                ix += 1;
            }
            elements.push(render_chat_list_block(
                items,
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
        let is_final = lines[ix..].iter().all(|line| line.trim().is_empty());
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

    div()
        .w_full()
        .min_w(px(0.))
        .text_size(crate::ui::design::text_body())
        .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
        .text_color(text_color)
        .child(StyledText::new(plain).with_highlights(highlights))
        .into_any_element()
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
    items: Vec<String>,
    element_seed: u64,
    block_index: usize,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let mut rows = Vec::new();
    for (item_index, item) in items.into_iter().enumerate() {
        rows.push(
            h_flex()
                .w_full()
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
                        .child(
                            TextView::markdown(
                                (
                                    "agent-chat-message-list-item",
                                    element_seed
                                        .wrapping_add((block_index as u64).saturating_mul(1000))
                                        .wrapping_add(item_index as u64),
                                ),
                                soften_file_code_spans(&item),
                                window,
                                cx,
                            )
                            .selectable(true)
                            .style(chat_message_text_style()),
                        ),
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
            div().w_full().overflow_x_scrollbar().px_4().py_3().child(
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
        let cells = markdown_table_cells(line);
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

        row = row.child(
            div()
                .flex_1()
                .min_w(px(120.))
                .px_3()
                .py_2()
                .text_size(crate::ui::design::text_ui())
                .line_height(gpui::relative(1.45))
                .font_weight(weight)
                .text_color(color)
                .when(index + 1 < cell_count, |cell| {
                    cell.border_r_1().border_color(divider)
                })
                .child(
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
                    .selectable(true),
                ),
        );
    }

    row.into_any_element()
}

pub(super) fn render_markdown_table(
    header: &[String],
    rows: &[Vec<String>],
    seed: u64,
    window: &mut Window,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let mut table = v_flex()
        .w_full()
        .min_w(px(0.))
        .flex_none()
        .rounded(crate::ui::design::r_sm())
        .border_1()
        .border_color(crate::ui::design::line(cx))
        .bg(crate::ui::style::surface(cx))
        // Deliberately x-only. `overflow_hidden` also sets `overflow.y`, and
        // taffy zeroes the automatic minimum size on any axis that is not
        // `Visible` — on the column main axis that let the chat list (which
        // measures rows at `MinContent` height) size the table short and clip
        // its last rows mid-row.
        .overflow_x_hidden()
        .child(render_markdown_table_row(
            header,
            true,
            rows.is_empty(),
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
