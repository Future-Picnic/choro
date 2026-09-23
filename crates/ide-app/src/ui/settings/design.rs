use super::*;

impl SettingsView {
    pub(super) fn render_design_section(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        v_flex().w_full().gap_4()
            .child(v_flex().gap_2().p_4().rounded(crate::ui::design::r_lg())
                .bg(crate::ui::design::surface(cx))
                .child(div().text_size(crate::ui::design::text_body()).font_weight(FontWeight::SEMIBOLD).child("Studio"))
                .child(div().text_size(crate::ui::design::text_ui()).text_color(crate::ui::design::t3(cx))
                    .child("Your default design workspace. Create and edit screens in Choro, prototype interactions, and implement saved designs. No design account or browser connection is needed.")))
            .into_any_element()
    }

}
