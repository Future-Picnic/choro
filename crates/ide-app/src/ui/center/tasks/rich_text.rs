use super::*;

pub(super) fn section_title(label: &'static str, cx: &mut Context<CenterArea>) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::t3(cx))
        .child(label)
        .into_any_element()
}

/// Description and comment bodies are prose, not objects — they read flush on
/// the base plane like the plain-markdown path, with no card around them.
pub(super) fn render_plain_text_block(
    text: &str,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let lines = text.lines().collect::<Vec<_>>();
    v_flex()
        .gap_1()
        .text_size(crate::ui::design::text_body())
        .line_height(gpui::relative(1.42))
        .text_color(crate::ui::design::t1(cx))
        .children(lines.iter().map(|line| {
            div()
                .min_h(px(18.))
                .child(SharedString::from(line.to_string()))
                .into_any_element()
        }))
        .into_any_element()
}

pub(super) fn render_rich_text_block(
    content: Option<&ide_core::TaskRichText>,
    fallback: &str,
    id_prefix: &'static str,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let blocks = content
        .map(|content| content.blocks.as_slice())
        .unwrap_or_default();
    if blocks.is_empty() {
        let text = content
            .map(|content| content.text.trim())
            .filter(|text| !text.is_empty())
            .unwrap_or(fallback);
        return render_plain_text_block(text, cx);
    }

    v_flex()
        .gap_2p5()
        .children(blocks.iter().enumerate().map(|(index, block)| match block {
            ide_core::TaskContentBlock::Text(text) => render_inline_text_block(text, cx),
            ide_core::TaskContentBlock::Image(image) => {
                render_task_image_block(image, id_prefix, index, cx)
            }
        }))
        .into_any_element()
}

pub(super) fn render_inline_text_block(
    text: &str,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let lines = text.lines().collect::<Vec<_>>();
    v_flex()
        .gap_1()
        .text_size(crate::ui::design::text_body())
        .line_height(gpui::relative(1.42))
        .text_color(crate::ui::design::t1(cx))
        .children(lines.iter().map(|line| {
            div()
                .min_h(px(18.))
                .child(SharedString::from(line.to_string()))
                .into_any_element()
        }))
        .into_any_element()
}

pub(super) fn render_task_image_block(
    image: &ide_core::TaskInlineImage,
    id_prefix: &'static str,
    index: usize,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let label = image
        .filename
        .clone()
        .or_else(|| image.alt.clone())
        .or_else(|| image.media_id.clone())
        .unwrap_or_else(|| "Jira image".to_string());
    let path = image.local_path.clone();
    let has_image = path.is_some();

    // The image is the content, so it carries no frame of its own — no stroke,
    // no fill behind it, just its own rounded edge. This mirrors the agent-chat
    // image block: a bounded, invisible box that the image is contained within
    // and left-aligned in, so it lines up with the prose above it.
    let mut frame = div()
        .id(SharedString::from(format!("{id_prefix}:{index}-frame")))
        .w_full()
        .max_w(px(520.))
        .h(px(320.))
        .flex()
        .items_center()
        .justify_start()
        .when_some(path, |container, path| {
            container.child(
                img(path)
                    .max_w_full()
                    .max_h_full()
                    .rounded(px(style::RADIUS))
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(|| task_image_placeholder("Image preview unavailable")),
            )
        })
        .when(!has_image, |container| {
            container.child(task_image_placeholder("Image preview unavailable"))
        });
    // Tap to open the full-size image modal — the same one chat uses. The
    // pointer is the affordance; a hover stroke would put back the edge this
    // change removes.
    if let Some(preview_path) = image.local_path.clone() {
        frame = frame
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_agent_image_preview(preview_path.clone(), window, cx);
            }));
    }

    v_flex()
        .id((id_prefix, index))
        .gap_1p5()
        .child(frame)
        .child(
            h_flex()
                .items_center()
                .gap_1p5()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(Icon::new(IconName::Frame).size(crate::ui::design::icon_md()))
                .child(div().truncate().child(SharedString::from(label))),
        )
        .into_any_element()
}

pub(super) fn task_image_placeholder(label: &'static str) -> gpui::AnyElement {
    v_flex()
        .items_center()
        .justify_center()
        .gap_2()
        .text_size(crate::ui::design::text_body())
        .child(Icon::new(IconName::Frame).size(crate::ui::design::icon_xl()))
        .child(label)
        .into_any_element()
}

pub(super) fn rich_text_image_attachment_ids(detail: &TaskDetail) -> HashSet<String> {
    let mut ids = HashSet::new();
    collect_rich_text_image_attachment_ids(&detail.description, &mut ids);
    for comment in &detail.comments {
        collect_rich_text_image_attachment_ids(&comment.body, &mut ids);
    }
    ids
}

pub(super) fn collect_rich_text_image_attachment_ids(
    content: &ide_core::TaskRichText,
    ids: &mut HashSet<String>,
) {
    for block in &content.blocks {
        if let ide_core::TaskContentBlock::Image(image) = block {
            if let Some(attachment_id) = &image.attachment_id {
                ids.insert(attachment_id.clone());
            }
        }
    }
}
