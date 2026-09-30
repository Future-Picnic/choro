use super::*;

impl SettingsView {
    pub(super) fn render_appearance_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_theme = self
            .workspace
            .read(cx)
            .theme_name
            .clone()
            .unwrap_or_else(|| crate::theme::SIGNATURE_THEME.to_string());
        let conversation_layout = self.workspace.read(cx).conversation_layout;
        v_flex()
                            .w_full()
                            .gap_3()
                            .child(
                                v_flex()
                                    .w_full()
                                    .gap_3()
                                    .p_4()
                                    .rounded(crate::ui::design::r_lg())
                                    .border_1()
                                    .border_color(crate::ui::design::line_2(cx))
                                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Theme"),
                                    )
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap_2()
                                            .flex_wrap()
                                            .children(
                                                crate::theme::available_themes(cx)
                                                    .into_iter()
                                                    .enumerate()
                                                    .map(|(index, name)| {
                                                        let selected = name == current_theme;
                                                        let label = name.clone();
                                                        crate::ui::style::settings_option_button(
                                                            ("settings-theme-option", index),
                                                            label,
                                                            selected,
                                                        )
                                                            .on_click(cx.listener(
                                                                move |this, _, _, cx| {
                                                                    this.select_theme(name.clone(), cx);
                                                                },
                                                            ))
                                                    }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child("Custom themes can be dropped into the themes folder next to the config file."),
                                    ),
                            )
                            .child(
                                v_flex()
                                    .w_full()
                                    .gap_3()
                                    .p_4()
                                    .rounded(crate::ui::design::r_lg())
                                    .border_1()
                                    .border_color(crate::ui::design::line_2(cx))
                                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Conversation layout"),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child("Choose where new agent work appears while you chat."),
                                    )
                                    .child(
                                        crate::ui::style::segmented_container_quiet(cx)
                                            .max_w(px(420.))
                                            .child(
                                                crate::ui::style::segment(
                                                    "settings-conversation-layout-classic",
                                                    IconName::ArrowUp,
                                                    "Classic · newest at bottom",
                                                    conversation_layout == ConversationLayout::Classic,
                                                    cx,
                                                )
                                                .flex_1()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.select_conversation_layout(
                                                        ConversationLayout::Classic,
                                                        cx,
                                                    );
                                                })),
                                            )
                                            .child(
                                                crate::ui::style::segment(
                                                    "settings-conversation-layout-top-down",
                                                    IconName::ArrowDown,
                                                    "Top-down · newest first",
                                                    conversation_layout == ConversationLayout::TopDown,
                                                    cx,
                                                )
                                                .flex_1()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.select_conversation_layout(
                                                        ConversationLayout::TopDown,
                                                        cx,
                                                    );
                                                })),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_label())
                                            .text_color(crate::ui::design::t4(cx))
                                            .child("Top-down places the composer first, then grows the latest conversation downward."),
                                    ),
                            )
                            .into_any_element()
    }
}
