use super::*;

impl CenterArea {
    pub(super) fn render_agent_resume_saved_session(
        &self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        v_flex()
            .size_full()
            .min_h(px(280.))
            .items_center()
            .justify_center()
            .gap_4()
            .child(
                div()
                    .w(px(160.))
                    .h(px(110.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        crate::ui::illustrations::illustration(
                            crate::ui::illustrations::Illustration::Resume,
                            cx,
                        )
                        .size_full()
                        .object_fit(ObjectFit::Contain),
                    ),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Resume this conversation"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Load the saved chat history and reconnect the agent."),
                    ),
            )
            .child(
                style::primary_button(
                    ("agent-chat-center-resume", agent_id.as_u128() as u64),
                    "Resume",
                    cx,
                )
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.start_agent(agent_id, window, cx);
                })),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_chat_resume_loader(
        &self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let accent = crate::ui::design::accent(cx);
        v_flex()
            .size_full()
            .min_h(px(320.))
            .items_center()
            .justify_center()
            .gap_5()
            .child(
                div()
                    .relative()
                    .size(px(78.))
                    .rounded_full()
                    .border_1()
                    .border_color(style::border(cx))
                    .bg(style::surface(cx))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(logo_spinner(
                        48.,
                        "agent-chat-resume-history",
                        agent.id.as_u128() as usize,
                        accent,
                    ))
                    .child(
                        div()
                            .absolute()
                            .right(px(4.))
                            .bottom(px(4.))
                            .size(px(24.))
                            .rounded_full()
                            .border_1()
                            .border_color(crate::ui::design::base(cx))
                            .bg(crate::ui::design::surface(cx))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                provider_brand_icon(agent.provider)
                                    .size(crate::ui::design::icon_md())
                                    .text_color(crate::ui::design::t1(cx)),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Restoring conversation"),
                    )
                    .child(
                        div()
                            .max_w(px(420.))
                            .text_center()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.45))
                            .text_color(crate::ui::design::t3(cx))
                            .child("Loading the saved timeline in the background, then reconnecting the agent."),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .children([
                        resume_loader_step("History", true, cx),
                        resume_loader_step("Context", true, cx),
                        resume_loader_step("Reconnect", false, cx),
                    ]),
            )
            .into_any_element()
    }

    pub(super) fn open_agent_image_preview(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Attached image")
            .to_string();

        window.open_dialog(cx, move |dialog, _, cx| {
            let title = title.clone();
            let image_path = path.clone();
            let copy_path = path.clone();
            dialog
                .w(px(860.))
                .overlay_closable(true)
                .title(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .child(crate::ui::confirm::icon_badge(
                            IconName::Frame,
                            crate::ui::design::accent(cx),
                            cx,
                        ))
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title)),
                )
                .child(
                    // The image sits straight on the dialog plane: no stroke and
                    // no fill behind it, so the preview is the picture rather
                    // than a picture inside a box.
                    div()
                        .w_full()
                        .h(px(560.))
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            img(image_path.clone())
                                .max_w_full()
                                .max_h_full()
                                .rounded(crate::ui::design::r_md())
                                .object_fit(ObjectFit::Contain)
                                .with_fallback(|| {
                                    div()
                                        .size_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(gpui_component::Icon::new(IconName::Frame).size_8())
                                        .into_any_element()
                                }),
                        ),
                )
                .footer(move |_, _, _, cx| {
                    let copy_path = copy_path.clone();
                    vec![
                        crate::ui::style::dialog_neutral_button(
                            "agent-image-preview-close",
                            "Close",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                        crate::ui::style::primary_button_compact(
                            "agent-image-preview-copy",
                            "Copy image",
                            cx,
                        )
                        .icon(IconName::Copy)
                        .on_click(move |_, _, cx| {
                            match clipboard_item_for_image_path(&copy_path) {
                                Ok(item) => cx.write_to_clipboard(item),
                                Err(error) => {
                                    eprintln!("failed to copy attached image: {error:#}")
                                }
                            }
                        }),
                    ]
                })
        });
    }
}
