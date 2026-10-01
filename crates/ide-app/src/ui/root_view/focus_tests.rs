//! Real action routing in a test window; no live workspace or native UI control.
use super::*;

struct SidebarFocusFixture {
    root_focus: FocusHandle,
    input_focus: FocusHandle,
    input_mounted: bool,
    show_right: bool,
}

impl Render for SidebarFocusFixture {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            v_flex()
                .size_full()
                .track_focus(&self.root_focus)
                .on_action(cx.listener(|this, _: &ToggleRightPanel, _, cx| {
                    this.show_right = !this.show_right;
                    cx.notify();
                }))
                .when(!self.show_right, |view| {
                    view.child(
                        style::git_sidebar_open_button(Some(3), cx)
                            .debug_selector(|| "git-opener".into()),
                    )
                })
                .when(self.show_right && self.input_mounted, |view| {
                    view.child(
                        div()
                            .track_focus(&self.input_focus)
                            .child("Focused control"),
                    )
                }),
        )
    }
}

fn fixture(
    cx: &mut gpui::TestAppContext,
) -> (Entity<SidebarFocusFixture>, &mut gpui::VisualTestContext) {
    cx.update(|cx| {
        gpui_component::init(cx);
        cx.bind_keys([gpui::KeyBinding::new("cmd-shift-b", ToggleRightPanel, None)]);
    });
    let mut fixture = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let view = cx.new(|cx| {
            let root_focus = cx.focus_handle();
            let input_focus = cx.focus_handle();
            restore_workspace_focus_on_loss(root_focus.clone(), window, cx);
            input_focus.focus(window);
            SidebarFocusFixture {
                root_focus,
                input_focus,
                input_mounted: true,
                show_right: true,
            }
        });
        fixture = Some(view.clone());
        gpui_component::Root::new(view, window, cx)
    });
    cx.run_until_parked();
    (fixture.unwrap(), cx)
}

#[gpui::test]
fn git_button_reopens_sidebar_after_its_focused_control_is_hidden(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    cx.simulate_keystrokes("cmd-shift-b");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.show_right));
    let button = cx.debug_bounds("git-opener").unwrap();
    cx.simulate_click(button.center(), Modifiers::default());
    cx.run_until_parked();
    assert!(
        view.read_with(cx, |view, _| view.show_right),
        "Git must reopen without navigating away"
    );
}

#[gpui::test]
fn sidebar_shortcut_works_after_the_focused_control_is_replaced(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    // Keep the handle alive, like a cached composer or panel input, but stop
    // mounting it. GPUI still has a focus ID with no action path in this frame.
    view.update(cx, |view, cx| {
        view.input_mounted = false;
        cx.notify();
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("cmd-shift-b");
    cx.run_until_parked();
    assert!(
        !view.read_with(cx, |view, _| view.show_right),
        "Shortcut must reach the workspace handler"
    );
    cx.simulate_keystrokes("cmd-shift-b");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.show_right));
}

#[gpui::test]
fn workspace_fallback_preserves_focus_in_a_mounted_control(cx: &mut gpui::TestAppContext) {
    let (view, cx) = fixture(cx);
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    cx.update(|window, cx| {
        assert!(view.read(cx).input_focus.is_focused(window));
        assert!(!view.read(cx).root_focus.is_focused(window));
    });
}
