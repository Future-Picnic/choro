use super::*;

#[derive(Clone)]
pub(super) enum AttachmentRemoval {
    AgentChat { agent_id: Uuid, path: PathBuf },
    NewAgent { path: PathBuf },
}

impl CenterArea {
    pub(super) fn render_agent_attachment_pending(
        &self,
        id: (&'static str, usize),
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .id(id)
            .h(px(34.))
            .flex_none()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::sage(cx).opacity(0.28))
            .bg(crate::ui::design::sage(cx).opacity(0.10))
            .px_2()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::style::focus_text(cx))
            .child(gpui_component::spinner::Spinner::new().xsmall())
            .child("Attaching image…")
            .into_any_element()
    }

    fn remove_composer_attachment(&mut self, removal: &AttachmentRemoval) {
        match removal {
            AttachmentRemoval::AgentChat { agent_id, path } => {
                if let Some(files) = self.agent_chat_attached_files.get_mut(agent_id) {
                    files.retain(|candidate| candidate != path);
                    if files.is_empty() {
                        self.agent_chat_attached_files.remove(agent_id);
                    }
                }
            }
            AttachmentRemoval::NewAgent { path } => {
                if let Some(composer) = self.new_agent_composer.as_mut() {
                    composer
                        .attached_files
                        .retain(|candidate| candidate != path);
                    composer.error = None;
                }
            }
        }
    }

    pub(super) fn render_agent_attachment_preview(
        &self,
        id: (&'static str, usize),
        path: PathBuf,
        removable: Option<AttachmentRemoval>,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if image_format_for_path(&path).is_none() {
            return self.render_agent_attached_file_chip(id, path, removable, width, height, cx);
        }

        let preview_path = path.clone();
        let label = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Attached image")
            .to_string();

        div()
            .id(id)
            .relative()
            .w(px(width))
            .h(px(height))
            .flex_none()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::style::border(cx))
            .bg(crate::ui::design::surface(cx))
            .overflow_hidden()
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
            .hover(|style| style.border_color(crate::ui::design::accent(cx).opacity(0.42)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_agent_image_preview(preview_path.clone(), window, cx);
            }))
            .child(
                img(path.clone())
                    .size_full()
                    .rounded(crate::ui::design::r_lg())
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(|| {
                        div()
                            .size_full()
                            .rounded(crate::ui::design::r_lg())
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui_component::Icon::new(IconName::Frame)
                                    .size(crate::ui::design::icon_lg()),
                            )
                            .into_any_element()
                    }),
            )
            .when_some(removable, |thumb, removal| {
                thumb.child(
                    div().absolute().top_1().right_1().child(
                        crate::ui::style::attachment_remove_button(
                            ("remove-agent-chat-attachment", id.1),
                            cx,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.remove_composer_attachment(&removal);
                            cx.notify();
                        })),
                    ),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_agent_attached_file_chip(
        &self,
        id: (&'static str, usize),
        path: PathBuf,
        removable: Option<AttachmentRemoval>,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Attached file")
            .to_string();
        let tooltip_path = path.to_string_lossy().to_string();
        let copy_path = tooltip_path.clone();

        h_flex()
            .id(id)
            .max_w(px((width * 2.8).max(168.)))
            .h(px(height.min(36.).max(30.)))
            .flex_none()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::sage(cx).opacity(0.28))
            .bg(crate::ui::design::sage(cx).opacity(0.12))
            .px_2()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::style::focus_text(cx))
            .cursor_pointer()
            .tooltip(move |window, cx| Tooltip::new(tooltip_path.clone()).build(window, cx))
            .hover(|style| style.border_color(crate::ui::design::sage(cx).opacity(0.45)))
            .on_click(move |_, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(copy_path.clone()));
            })
            .child(
                gpui_component::Icon::new(IconName::File)
                    .size(crate::ui::design::icon_md())
                    .text_color(crate::ui::design::sage(cx)),
            )
            .child(div().min_w(px(0.)).truncate().child(file_name))
            .when_some(removable, |chip, removal| {
                chip.child(
                    crate::ui::style::attachment_remove_button(
                        ("remove-agent-chat-file-attachment", id.1),
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.remove_composer_attachment(&removal);
                        cx.notify();
                    })),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_agent_chat_pasted_text_block(
        &self,
        agent_id: Uuid,
        block: &PastedTextBlock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let block_id = block.id;
        let display_text = if block.expanded {
            block.text.clone()
        } else {
            truncate_text_lines(&block.text, PASTED_BLOCK_PREVIEW_LINES)
        };

        v_flex()
            .w_full()
            .rounded(px(crate::ui::style::RADIUS_LG))
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::surface(cx).opacity(0.46))
            .p_2()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        gpui_component::Icon::new(IconName::File)
                            .size(crate::ui::design::icon_sm()),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::style::focus_text(cx))
                            .child(format!("Pasted text · {} lines", block.line_count)),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(("copy-pasted-chat-text", block_id.as_u128() as u64))
                            .ghost()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_xs())
                            .icon(IconName::Copy)
                            .tooltip("Copy pasted text")
                            .on_click({
                                let text = block.text.clone();
                                move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()))
                                }
                            }),
                    )
                    .child(
                        Button::new(("remove-pasted-chat-text", block_id.as_u128() as u64))
                            .ghost()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_xs())
                            .label("Remove")
                            .tooltip("Remove pasted text")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(blocks) =
                                    this.agent_chat_pasted_text_blocks.get_mut(&agent_id)
                                {
                                    blocks.retain(|block| block.id != block_id);
                                    if blocks.is_empty() {
                                        this.agent_chat_pasted_text_blocks.remove(&agent_id);
                                    }
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .max_h(if block.expanded { px(420.) } else { px(180.) })
                    .overflow_y_scrollbar()
                    .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.32))
                    .child(
                        TextView::markdown(
                            ("pasted-chat-text-preview", block_id.as_u128() as u64),
                            display_text,
                            window,
                            cx,
                        )
                        .selectable(true)
                        .style(chat_message_text_style()),
                    ),
            )
            .when(block.line_count > PASTED_BLOCK_PREVIEW_LINES, |card| {
                let expanded = block.expanded;
                card.child(
                    h_flex().w_full().justify_start().child(
                        Button::new(("toggle-pasted-chat-text", block_id.as_u128() as u64))
                            .ghost()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_xs())
                            .label(if expanded { "Show less" } else { "Show more" })
                            .dropdown_caret(true)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(blocks) =
                                    this.agent_chat_pasted_text_blocks.get_mut(&agent_id)
                                {
                                    if let Some(block) =
                                        blocks.iter_mut().find(|block| block.id == block_id)
                                    {
                                        block.expanded = !block.expanded;
                                    }
                                }
                                cx.notify();
                            })),
                    ),
                )
            })
            .into_any_element()
    }
}
