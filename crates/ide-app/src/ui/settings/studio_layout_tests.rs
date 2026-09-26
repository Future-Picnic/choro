//! Headless GPUI measurements of the shared Studio row and form treatments.
use super::*;

struct StudioFixture {
    preferences: Entity<InputState>,
}
impl Render for StudioFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list =
            style::expert_settings_list_frame(cx).debug_selector(|| "studio-list".into());
        for index in 0usize..12 {
            list = list.child(style::expert_settings_list_row(("test-studio-skill", index), cx)
                .child(skill_copy("Accessibility and inclusive interaction design guidelines".into(), "Use readable contrast, preserve the existing project components, and check keyboard navigation throughout every form and dialog.".into(), "Claude", cx)
                    .when(index == 0, |copy| copy.debug_selector(|| "studio-copy".into())))
                .child(h_flex().flex_none().gap_1()
                    .when(index == 0, |actions| actions.debug_selector(|| "studio-actions".into()))
                    .child(style::ghost_button_compact(("test-edit", index), "Edit"))
                    .child(style::ghost_button_compact(("test-detach", index), "Detach"))));
        }
        h_flex()
            .size_full()
            .child(div().w(px(236.)).h_full().flex_none())
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_hidden()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.))
                            .h_full()
                            .max_w(px(960.))
                            .mx_auto()
                            .px_6()
                            .py_5()
                            .gap_3()
                            .child(div().flex_none().h(px(52.)).child("Studio"))
                            .child(
                                style::expert_settings_scroll_body(
                                    "studio-test-scroll",
                                    window,
                                    cx,
                                )
                                .debug_selector(|| "studio-viewport".into())
                                .gap_5()
                                .child(heading(
                                    "Additional skills",
                                    "Copies are editable here; originals stay unchanged.",
                                    cx,
                                ))
                                .child(list)
                                .child(
                                    style::expert_settings_multiline_input(&self.preferences)
                                        .h(px(120.)),
                                )
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .justify_end()
                                        .child(style::dialog_neutral_button(
                                            "test-revert",
                                            "Revert",
                                            cx,
                                        ))
                                        .child(style::primary_button_compact(
                                            "test-save",
                                            "Save preferences",
                                            cx,
                                        )),
                                ),
                            ),
                    ),
            )
    }
}

#[gpui::test]
fn studio_skills_keep_actions_visible_with_long_content(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|window, cx| StudioFixture {
        preferences: cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .rows(4)
                .default_value("Keep layouts compact.")
        }),
    });
    for width in [912., 560., 420.] {
        cx.simulate_resize(gpui::size(px(width + 236. + 48.), px(640.)));
        cx.run_until_parked();
        let viewport = cx.debug_bounds("studio-viewport").unwrap();
        let list = cx.debug_bounds("studio-list").unwrap();
        let copy = cx.debug_bounds("studio-copy").unwrap();
        let actions = cx.debug_bounds("studio-actions").unwrap();
        assert!(
            list.size.height > viewport.size.height,
            "Long list must scroll"
        );
        assert!(
            list.right() <= viewport.right(),
            "List overflows at {width}"
        );
        assert!(copy.size.width > px(100.), "Copy collapsed at {width}");
        assert!(
            copy.right() <= actions.left(),
            "Text overlaps actions at {width}"
        );
        assert!(
            actions.right() <= viewport.right(),
            "Actions clipped at {width}"
        );
    }
}
