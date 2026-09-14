//! Real GPUI measurement in a test window; no app state, database, or OS UI.
use super::*;

struct BandListFixture;

impl Render for BandListFixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut list =
            style::expert_settings_list_frame(cx).debug_selector(|| "band-list-frame".into());
        for (index, entry) in catalog::catalog().experts.iter().enumerate() {
            let mut profile = entry.profile();
            if index == 0 {
                profile.name = "Accessibility and interaction design specialist".into();
                profile.description = "Improve keyboard, screen-reader, visual, and interaction accessibility while preserving the existing design system and reviewing every important user journey.".into();
            }
            let actions = h_flex()
                .flex_none()
                .gap_1()
                .when(index == 0, |actions| {
                    actions.debug_selector(|| "band-row-actions".into())
                })
                .child(style::dialog_neutral_button(
                    ("layout-enabled", index),
                    "Enabled",
                    cx,
                ))
                .child(style::dialog_neutral_button(
                    ("layout-edit", index),
                    "Edit",
                    cx,
                ))
                .child(
                    style::settings_inline_icon_button(("layout-more", index), IconName::Ellipsis)
                        .dropdown_menu(|menu, _, _| menu),
                );
            list = list.child(expert_row_frame(index, &profile, cx).child(actions));
        }
        // Match the Settings sidebar, bounded page and fixed header/toolbar
        // chain so percentages are measured in the same containing blocks.
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
                            .mx_auto()
                            .h_full()
                            .min_h(px(0.))
                            .max_w(px(960.))
                            .gap_3()
                            .px_6()
                            .py_5()
                            .child(div().w_full().h(px(52.)).flex_none().child("Band"))
                            .child(
                                v_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .gap_3()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .w_full()
                                            .h(px(28.))
                                            .flex_none()
                                            .child("Search bandmates"),
                                    )
                                    .child(
                                        style::expert_settings_scroll_body(
                                            "band-layout-scroll",
                                            window,
                                            cx,
                                        )
                                        .debug_selector(|| "band-list-viewport".into())
                                        .child(list),
                                    ),
                            ),
                    ),
            )
    }
}

#[gpui::test]
fn band_list_keeps_edit_actions_inside_viewport(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let (_, cx) = cx.add_window_view(|_, _| BandListFixture);
    for width in [912., 560., 420.] {
        cx.simulate_resize(gpui::size(px(width + 236. + 48.), px(640.)));
        cx.run_until_parked();
        let viewport = cx
            .debug_bounds("band-list-viewport")
            .expect("viewport rendered");
        assert_eq!(
            viewport.size.width,
            px(width),
            "resize did not render the requested content width"
        );
        let list = cx.debug_bounds("band-list-frame").expect("list rendered");
        let actions = cx
            .debug_bounds("band-row-actions")
            .expect("actions rendered");
        assert!(
            list.size.height > viewport.size.height,
            "long list must scroll, not shrink its rows"
        );
        assert!(
            list.right() <= viewport.right(),
            "list exceeds viewport at {width}px: {list:?} / {viewport:?}"
        );
        let content = cx
            .debug_bounds("band-row-content-0")
            .expect("row text rendered");
        let model = cx.debug_bounds("band-row-model-0").expect("model rendered");
        assert!(
            content.size.width > px(64.) && model.size.width > px(64.),
            "row text collapsed at {width}px"
        );
        assert!(
            content.right() <= actions.left(),
            "row text crowds actions at {width}px"
        );
        assert!(
            actions.right() <= viewport.right(),
            "actions clipped at {width}px: {actions:?} / {viewport:?}"
        );
        assert!(
            actions.left() >= viewport.left(),
            "actions leave viewport at {width}px"
        );
        assert!(
            actions.size.width > px(80.),
            "actions collapsed at {width}px"
        );
        assert!(
            actions.top() >= viewport.top() && actions.bottom() <= viewport.bottom(),
            "actions not reachable at {width}px"
        );
    }
}
