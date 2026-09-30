use super::*;

impl CenterArea {
    pub fn render_selected_doc_assistant_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((project, _)) = self.active_project(cx) else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(gpui_component::Icon::new(IconName::Bot).size_8())
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("Open a project to use the assistant"),
                )
                .into_any_element();
        };
        let Some(doc) = self.docs.read(cx).selected_doc(project) else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(gpui_component::Icon::new(crate::ui::design::docs_icon()).size_8())
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("Open a doc to use the assistant"),
                )
                .into_any_element();
        };
        self.render_doc_assistant_panel(project, &doc, window, cx)
    }

    pub(super) fn doc_label_input(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.doc_label_inputs.get(&project) {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search or create label"));
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.doc_label_inputs.insert(project, input.clone());
        input
    }
}
