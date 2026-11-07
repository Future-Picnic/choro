use super::*;

impl SettingsView {
    /// Whether Merge PR dialogs start with "Return to <base> and pull latest"
    /// selected. Each merge can still change it.
    pub(super) fn render_merge_preferences_card(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let enabled = self.workspace.read(cx).return_to_base_after_merge;
        let segment = |id: &'static str, icon: IconName, label: &'static str, value: bool| {
            crate::ui::style::segment(id, icon, label, enabled == value, cx)
                .flex_1()
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.set_return_to_base_after_merge(value, cx);
                    });
                    cx.notify();
                }))
        };

        h_flex()
            .w_full()
            .gap_5()
            .items_center()
            .justify_between()
            .p_4()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::surface(cx).opacity(0.55))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Return to the base branch after merging"),
                    )
                    .child(
                        div()
                            .max_w(px(460.))
                            .text_size(crate::ui::design::text_body())
                            .text_color(crate::ui::design::t3(cx))
                            .child(
                                "Pre-select “Return to main and pull latest” (or whichever branch the PR merges into) when you merge a pull request. You can still change it for each merge.",
                            ),
                    ),
            )
            .child(
                crate::ui::style::segmented_container_quiet(cx)
                    .w(px(220.))
                    .flex_none()
                    .child(segment(
                        "settings-merge-return-on",
                        IconName::Check,
                        "Selected",
                        true,
                    ))
                    .child(segment(
                        "settings-merge-return-off",
                        IconName::Close,
                        "Off",
                        false,
                    )),
            )
            .into_any_element()
    }
}
