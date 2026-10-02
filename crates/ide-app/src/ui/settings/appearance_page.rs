use super::*;

impl SettingsView {
    /// Theme only. Sidebar surface, divider lines, and conversation layout are
    /// fixed design decisions (flat, soft, classic), not preferences.
    pub(super) fn render_appearance_page(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let current_theme = self
            .workspace
            .read(cx)
            .theme_name
            .clone()
            .unwrap_or_else(|| crate::theme::SIGNATURE_THEME.to_string());
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
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child("Each preview is painted in that theme's own colors — sidebar, canvas, card, and the accent and status tokens."),
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
                                                        super::theme_preview::theme_card(
                                                            ("settings-theme-option", index),
                                                            name.clone().into(),
                                                            selected,
                                                            cx,
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
                            .into_any_element()
    }
}
