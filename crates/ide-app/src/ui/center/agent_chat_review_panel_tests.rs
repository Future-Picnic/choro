//! In-memory GPUI windows only; no reviewer process or user store is touched.
use super::*;
use ide_core::code_review::{
    ReviewFile, ReviewFileStatus, ReviewFreshness, ReviewRun, ReviewRunState, ReviewStage,
};

struct Fixture {
    chats: Entity<AgentChatState>,
    ui: ReviewUiState,
    composers: Vec<(Uuid, Entity<InputState>, Vec<PathBuf>)>,
    /// Per slot, whether the latest frame showed the review panel. GPUI's
    /// debug-bounds map keeps entries from earlier frames, so absence is
    /// checked here rather than through `debug_bounds`.
    held: Vec<bool>,
}

impl Render for Fixture {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut column = v_flex().size_full().gap_2();
        self.held.clear();
        for (index, (agent, input, attachments)) in self.composers.clone().into_iter().enumerate() {
            let chats = self.chats.clone();
            let on_cancel: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| {
                chats.update(cx, |chats, cx| chats.cancel_review(agent, cx));
            });
            let draft = HeldDraft {
                text: !input.read(cx).value().trim().is_empty(),
                attachments: attachments.len(),
            };
            column = match review_hold(
                &mut self.ui,
                &self.chats,
                agent,
                draft,
                &input,
                on_cancel,
                window,
                cx,
            ) {
                Some(panel) => {
                    self.held.push(true);
                    column.child(div().w(px(720.)).child(panel))
                }
                None => {
                    self.held.push(false);
                    column.child(
                        style::composer_frame(cx)
                            .w(px(720.))
                            .debug_selector(move || format!("composer-{index}"))
                            .child(style::composer_draft_editor(&input, false)),
                    )
                }
            };
        }
        column
    }
}

fn run(parent: Uuid, state: ReviewRunState) -> ReviewRun {
    let mut run = ReviewRun::new(
        Uuid::new_v4(),
        parent,
        "Claude".into(),
        "model".into(),
        "effort".into(),
        1,
    );
    run.state = state;
    run.stage = ReviewStage::Reviewing;
    run.freshness = ReviewFreshness::Current;
    run.current_group = Some(String::new());
    run.files = ["a.rs", "b.rs", "c.rs"]
        .into_iter()
        .enumerate()
        .map(|(i, path)| ReviewFile {
            id: path.into(),
            path: path.into(),
            change_kind: "modified".into(),
            attributed_ranges: vec![],
            before_hash: None,
            after_hash: None,
            diff_pages: 1,
            consumed_pages: Default::default(),
            status: if i == 0 {
                ReviewFileStatus::Complete
            } else if i == 1 {
                ReviewFileStatus::Reviewing
            } else {
                ReviewFileStatus::Pending
            },
            skip_reason: None,
        })
        .collect();
    run
}

struct Handles {
    view: Entity<Fixture>,
    a: Uuid,
    b: Uuid,
    input_a: Entity<InputState>,
    input_b: Entity<InputState>,
}

fn open(cx: &mut gpui::TestAppContext) -> (Handles, &mut gpui::VisualTestContext) {
    cx.update(gpui_component::init);
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let mut handles = None;
    let (_, cx) = cx.add_window_view(|window, cx| {
        let chats = cx.new(|_| AgentChatState::new());
        let input_a = cx.new(|cx| InputState::new(window, cx).default_value("Keep this draft"));
        let input_b = cx.new(|cx| InputState::new(window, cx));
        let view = cx.new(|cx| {
            cx.subscribe(&chats, |_: &mut Fixture, _, _: &AgentChatEvent, cx| cx.notify())
                .detach();
            Fixture {
                chats,
                ui: ReviewUiState::default(),
                composers: vec![
                    (a, input_a.clone(), vec![PathBuf::from("/draft/screenshot.png")]),
                    (b, input_b.clone(), vec![]),
                ],
                held: vec![],
            }
        });
        handles = Some(Handles {
            view: view.clone(),
            a,
            b,
            input_a,
            input_b,
        });
        gpui_component::Root::new(view, window, cx)
    });
    cx.run_until_parked();
    (handles.unwrap(), cx)
}

/// Mirrors `CenterArea::start_agent_review` after `start_review` succeeds.
fn start(h: &Handles, cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| {
        let input_focused = h.input_a.read(cx).focus_handle(cx).is_focused(window);
        h.view.update(cx, |view, cx| {
            view.chats.update(cx, |chats, cx| {
                chats.set_review_fixture(run(h.a, ReviewRunState::Running), true, true, cx)
            });
            view.ui.hold_focus(h.a, input_focused, window, cx);
        });
    });
    cx.run_until_parked();
}

fn report(h: &Handles, state: ReviewRunState, active: bool, cx: &mut gpui::VisualTestContext) {
    cx.update(|_, cx| {
        h.view.update(cx, |view, cx| {
            let mut next = view.chats.read(cx).review_run(h.a).unwrap().clone();
            next.state = state;
            next.revision += 1;
            view.chats
                .update(cx, |chats, cx| chats.set_review_fixture(next, active, true, cx));
        });
    });
    cx.run_until_parked();
}

fn draft_of(input: &Entity<InputState>, cx: &mut gpui::VisualTestContext) -> String {
    cx.update(|_, cx| input.read(cx).value().to_string())
}

fn held(h: &Handles, slot: usize, cx: &mut gpui::VisualTestContext) -> bool {
    cx.update(|_, cx| h.view.read(cx).held[slot])
}

fn focused(input: &Entity<InputState>, cx: &mut gpui::VisualTestContext) -> bool {
    cx.update(|window, cx| input.read(cx).focus_handle(cx).is_focused(window))
}

#[gpui::test]
fn review_holds_the_composer_and_restores_draft_attachments_and_focus_on_every_exit(
    cx: &mut gpui::TestAppContext,
) {
    let (h, cx) = open(cx);
    cx.update(|window, cx| h.input_a.update(cx, |input, cx| input.focus(window, cx)));
    // A restored draft opens with the caret at the start; move to the end
    // the way a user would before continuing to type.
    cx.simulate_keystrokes("cmd-down");
    cx.simulate_input(", with typed text");
    let draft = draft_of(&h.input_a, cx);
    assert_eq!(draft, "Keep this draft, with typed text");

    for exit in [
        ReviewRunState::Complete,
        ReviewRunState::Cancelled,
        ReviewRunState::Failed,
        ReviewRunState::Partial,
        ReviewRunState::Interrupted,
    ] {
        assert!(focused(&h.input_a, cx), "{exit:?}: draft focused before Review");
        start(&h, cx);
        assert!(held(&h, 0, cx), "{exit:?}: the panel replaces the composer");
        assert!(!held(&h, 1, cx), "another conversation keeps its composer");
        assert!(!focused(&h.input_a, cx), "{exit:?}: focus moves into the panel");
        let panel = cx.debug_bounds("review-panel").expect("panel laid out");
        let open_changes = cx.debug_bounds("review-open-changes").expect("opened review changes are visible");
        assert!(open_changes.size.width <= panel.size.width);
        assert!(
            panel.size.height >= crate::ui::design::composer_frame_h(),
            "the panel keeps the composer's footprint: {panel:?}"
        );
        // Typing while held never reaches the parent's draft.
        cx.simulate_input("lost?");
        assert_eq!(draft_of(&h.input_a, cx), draft, "{exit:?}");

        if exit == ReviewRunState::Cancelled {
            cx.simulate_keystrokes("escape");
            cx.run_until_parked();
            cx.update(|_, cx| {
                let chats = h.view.read(cx).chats.read(cx);
                assert_eq!(
                    chats.review_run(h.a).unwrap().state,
                    ReviewRunState::Cancelling
                );
                assert!(chats.review_blocks_writing(h.a));
            });
            assert!(
                held(&h, 0, cx),
                "cancelling keeps the panel until the reviewer stops"
            );
        }

        // A terminal report is not enough while the reviewer process lives.
        report(&h, exit, true, cx);
        assert!(held(&h, 0, cx), "{exit:?}");
        assert_eq!(draft_of(&h.input_a, cx), draft, "{exit:?}");

        report(&h, exit, false, cx);
        assert!(!held(&h, 0, cx), "{exit:?}: the composer returns");
        assert_eq!(draft_of(&h.input_a, cx), draft, "{exit:?}");
        assert!(focused(&h.input_a, cx), "{exit:?}: focus returns to the draft");
        cx.update(|_, cx| {
            assert_eq!(
                h.view.read(cx).composers[0].2,
                [PathBuf::from("/draft/screenshot.png")],
                "{exit:?}: attachments survive"
            );
        });
        // The restored draft is live again: typing reaches it.
        cx.simulate_keystrokes("cmd-down");
        cx.simulate_input("!");
        assert_eq!(draft_of(&h.input_a, cx), format!("{draft}!"), "{exit:?}");
        cx.update(|window, cx| {
            h.input_a
                .update(cx, |input, cx| input.set_value(draft.clone(), window, cx));
            h.input_a.update(cx, |input, cx| input.focus(window, cx));
        });
    }
}

#[gpui::test]
fn other_conversations_stay_writable_and_keep_focus_when_a_review_ends(
    cx: &mut gpui::TestAppContext,
) {
    let (h, cx) = open(cx);
    cx.update(|window, cx| h.input_a.update(cx, |input, cx| input.focus(window, cx)));
    start(&h, cx);
    cx.update(|window, cx| h.input_b.update(cx, |input, cx| input.focus(window, cx)));
    cx.simulate_input("Still writing here");
    assert_eq!(draft_of(&h.input_b, cx), "Still writing here");
    assert!(held(&h, 0, cx) && !held(&h, 1, cx));
    cx.update(|_, cx| assert!(!h.view.read(cx).chats.read(cx).review_blocks_writing(h.b)));

    report(&h, ReviewRunState::Complete, false, cx);
    assert!(!held(&h, 0, cx));
    assert!(
        focused(&h.input_b, cx) && !focused(&h.input_a, cx),
        "the ended review must not steal focus from where the user moved it"
    );
    assert_eq!(draft_of(&h.input_a, cx), "Keep this draft");
}

#[gpui::test]
fn fix_waits_for_a_positive_freshness_check(cx: &mut gpui::TestAppContext) {
    use gpui::AppContext as _;
    let parent = Uuid::new_v4();
    let legacy = Uuid::new_v4();
    let chats = cx.new(|_| AgentChatState::new());
    let held = run(parent, ReviewRunState::Complete);
    let review_id = held.id.to_string();
    let receivers = chats.update(cx, |chats, cx| {
        chats.set_review_fixture(held, true, true, cx);
        (
            chats.validate_review_for_fix(parent, review_id, cx),
            chats.validate_review_for_fix(legacy, "legacy".into(), cx),
        )
    });
    cx.run_until_parked();
    assert!(
        matches!(smol::block_on(fix_permission(receivers.0)), FixPermission::Blocked(_)),
        "a reviewer that still holds the conversation blocks fixing"
    );
    assert_eq!(
        smol::block_on(fix_permission(receivers.1)),
        FixPermission::Allowed,
        "legacy Markdown reviews carry no snapshot to validate"
    );
}
