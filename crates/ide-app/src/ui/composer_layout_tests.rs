//! Composer measurements use an in-memory GPUI window, never the running app.
use super::*;
use gpui::{AppContext, Context, EntityInputHandler, Render, Window};

struct ComposerFixture {
    input: Entity<InputState>,
    queued: usize,
}

impl Render for ComposerFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().size_full().child(div().flex_1().min_h(px(0.))).child(
            v_flex()
                .w_full()
                .flex_shrink_0()
                .px(design::agent_chat_gutter_x())
                .pt_3()
                .pb_1()
                .when(self.queued > 0, |area| area.child(
                    v_flex().flex_shrink_0().w_full().min_w(px(0.))
                        .max_w(design::agent_chat_content_max_w()).mx_auto().pb_1p5().gap_0p5()
                        .children((0..self.queued).map(|index| {
                            h_flex().w_full().min_w(px(0.)).items_center().gap_2().px_1p5().py_1()
                                .child(design::indicator::lucide_icon(lucide_icons::Icon::Clock, design::t3(cx), design::icon_sm()))
                                .child(div().flex_1().min_w(px(0.)).truncate().text_size(design::text_ui()).child("Also in the desktop app I see five categories, but I cannot understand where they come from or which contain new email"))
                                .child(h_flex().flex_none().items_center().gap_0p5().invisible()
                                    .child(ghost_button_compact(("steer", index), "Steer").icon(IconName::Redo2))
                                    .child(div().flex_none().size(design::control_h_xs()))
                                    .child(div().flex_none().size(design::control_h_xs())))
                        })),
                ))
                .child(
                    composer_frame(cx)
                        .flex_shrink_0()
                        .max_w(design::agent_chat_content_max_w())
                        .mx_auto()
                        .gap_2()
                        .debug_selector(|| "composer-frame".into())
                        .child(
                            v_flex()
                                .w_full()
                                .min_w(px(0.))
                                .min_h(design::composer_input_min_h())
                                .gap_2()
                                .child(
                                    composer_draft_editor(&self.input, false)
                                        .debug_selector(|| "composer-editor".into()),
                                ),
                        )
                        .child(div().w_full().h(px(28.)).flex_none().child("Send")),
                ),
        )
    }
}

#[gpui::test]
fn composer_keeps_short_draft_visible_when_preview_closes(cx: &mut gpui::TestAppContext) {
    cx.update(gpui_component::init);
    let mut fixture = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 8)
                .default_value("dddddddddd")
        });
        let view = cx.new(|_| ComposerFixture { input, queued: 0 });
        fixture = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    let view = fixture.unwrap();
    for (width, queued) in [
        (1280., 0),
        (1280., 1),
        (1280., 2),
        (620., 2),
        (1280., 2),
        (1280., 5),
        (620., 5),
        (1280., 0),
    ] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.queued = queued;
                view.input.update(cx, |input, cx| {
                    input.set_value(
                        "A long queued message with many words. ".repeat(30),
                        window,
                        cx,
                    );
                });
                cx.notify();
            });
        });
        cx.run_until_parked();
        let long_frame = cx
            .debug_bounds("composer-frame")
            .expect("long draft rendered");
        assert!(
            long_frame.size.height > px(180.) && long_frame.size.height <= px(280.),
            "long draft should grow up to the row limit: {long_frame:?}"
        );
        cx.update(|window, cx| {
            view.read(cx).input.clone().update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.insert("dddddddddd", window, cx);
            });
        });
        cx.simulate_resize(gpui::size(px(width), px(1320.)));
        cx.run_until_parked();
        let frame = cx.debug_bounds("composer-frame").expect("frame rendered");
        let editor = cx.debug_bounds("composer-editor").expect("editor rendered");
        assert!(
            editor.size.width > px(400.),
            "input width collapsed: {editor:?}"
        );
        assert!(
            editor.size.height >= px(36.),
            "input collapsed at {width}px: {editor:?}"
        );
        assert!(
            frame.size.height < px(180.),
            "short draft inflated composer at {width}px: {frame:?}"
        );
        assert!(
            editor.left() >= frame.left()
                && editor.right() <= frame.right()
                && editor.top() >= frame.top()
                && editor.bottom() <= frame.bottom(),
            "editor must stay inside the frame: {editor:?} / {frame:?}"
        );
        cx.update(|window, cx| {
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        cx.simulate_input("a");
        cx.run_until_parked();
        cx.update(|window, cx| {
            view.read(cx).input.clone().update(cx, |input, cx| {
                assert_eq!(input.value().as_ref(), "dddddddddda");
                let text = input.bounds_for_range(0..11, editor, window, cx)
                    .expect("draft has a text layout");
                assert!(text.size.width > px(0.) && text.size.width <= editor.size.width,
                    "draft must have a visible shaped line at {width}px with {queued} queued: {text:?}");
                assert!(text.size.height <= editor.size.height);
            });
        });
    }
}
