//! Where a Visual answer appears, and whether its conversation is on screen.
//! Pure rules (no GPUI) so placement and screen containment are tested directly.
use gpui::{Bounds, Pixels};
use ide_core::visual_answers::VisualDocument;

use super::agent_chat_visual_answer::VisualPanelMode;

/// Informational results belong to the conversation; only an explicit choice
/// (`VisualDocument::requires_user_response`) is placed beside the composer.
/// Optional follow-up actions, search and sort never make it a question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VisualPlacement {
    Conversation,
    Composer,
}

impl VisualPlacement {
    pub fn of(document: &VisualDocument) -> Self {
        if document.requires_user_response() {
            Self::Composer
        } else {
            Self::Conversation
        }
    }
}

/// The one region that hosts an open answer's content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VisualSlot {
    /// Inside the answer's own transcript card.
    TranscriptCard,
    /// The panel at the top of the composer card.
    Composer,
    /// The same panel filling the conversation column.
    Expanded,
}

pub(super) fn visual_slot(placement: VisualPlacement, mode: VisualPanelMode) -> Option<VisualSlot> {
    match (mode, placement) {
        (VisualPanelMode::Collapsed, _) => None,
        (VisualPanelMode::Expanded, _) => Some(VisualSlot::Expanded),
        (VisualPanelMode::Docked, VisualPlacement::Conversation) => Some(VisualSlot::TranscriptCard),
        (VisualPanelMode::Docked, VisualPlacement::Composer) => Some(VisualSlot::Composer),
    }
}

/// What the Agents screen renders in place of the selected conversation.
/// Selecting an agent is not enough: the New agent composer, a non-chat
/// runtime, or a narrow-window delegation panel replaces its transcript.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct AgentsScreen {
    pub agents_mode: bool,
    pub new_agent_composer: bool,
    pub selected_chat: bool,
    pub panel_replaces_body: bool,
}

impl AgentsScreen {
    pub fn shows_selected_conversation(self) -> bool {
        self.agents_mode && !self.new_agent_composer && self.selected_chat && !self.panel_replaces_body
    }
}

/// Everything in the transcript around an inline result's content: list
/// padding, the row's bottom gap and the card's padding, border, header,
/// gaps and caption row (with slack for a wrapped caption).
pub(super) const INLINE_CARD_CHROME: f32 = 160.0;
/// Below this the content is too cramped to read; Expand is offered instead.
pub(super) const MIN_INLINE_CONTENT: f32 = 120.0;

/// Content height for an inline result: its preferred height, reduced to
/// what the measured transcript can show whole. `None` when it cannot fit
/// at all (short window or tall composer), so the card offers Expand rather
/// than a surface no scrolling could reveal.
pub(super) fn inline_content_height(preferred: f32, transcript_height: Option<f32>) -> Option<f32> {
    let Some(transcript) = transcript_height else {
        return Some(preferred);
    };
    let fits = (transcript - INLINE_CARD_CHROME).min(preferred);
    // Short content (measured below the readable minimum) fits whole.
    (fits >= MIN_INLINE_CONTENT.min(preferred)).then_some(fits.floor())
}

/// A native view cannot be clipped by GPUI, so an inline surface is shown
/// only while its whole rectangle lies inside the visible list region.
/// Anything partly outside would float over the header or composer.
pub(super) fn fully_visible(bounds: Bounds<Pixels>, mask: Bounds<Pixels>) -> bool {
    const TOLERANCE: f32 = 0.5;
    let (b, m) = (bounds, mask);
    b.size.width > gpui::px(0.)
        && b.size.height > gpui::px(0.)
        && f32::from(b.left()) >= f32::from(m.left()) - TOLERANCE
        && f32::from(b.top()) >= f32::from(m.top()) - TOLERANCE
        && f32::from(b.right()) <= f32::from(m.right()) + TOLERANCE
        && f32::from(b.bottom()) <= f32::from(m.bottom()) + TOLERANCE
}

/// Real GPUI layout: a transcript list between a header and a composer. Rows
/// see the list's own clip as their content mask, so only rows wholly inside
/// the conversation may show a native surface.
#[cfg(all(test, feature = "ui-layout-tests"))]
mod layout_tests {
    use std::{cell::RefCell, rc::Rc};

    use gpui::{
        canvas, div, list, px, IntoElement, ListAlignment, ListOffset, ListState, ParentElement,
        Render, Styled,
    };

    use super::fully_visible;

    type Seen = Rc<RefCell<Vec<(usize, f32, bool)>>>;

    /// Header, transcript list and composer, like the agent chat column.
    struct Conversation {
        state: ListState,
        seen: Seen,
    }

    impl Render for Conversation {
        fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            let rows = self.seen.clone();
            div()
                .w(px(600.))
                .h(px(HEADER + LIST + 80.))
                .flex()
                .flex_col()
                .child(div().h(px(HEADER)).flex_none())
                .child(
                    list(self.state.clone(), move |ix, _, _| {
                        let rows = rows.clone();
                        div()
                            .h(px(ROW))
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        let shown = fully_visible(bounds, window.content_mask().bounds);
                                        rows.borrow_mut().push((ix, f32::from(bounds.top()), shown));
                                    },
                                    |_, _, _, _| {},
                                )
                                .size_full(),
                            )
                            .into_any_element()
                    })
                    .h(px(LIST))
                    .flex_none(),
                )
                .child(div().h(px(80.)).flex_none())
        }
    }

    const HEADER: f32 = 40.;
    const LIST: f32 = 300.;
    const ROW: f32 = 120.;

    #[gpui::test]
    fn scrolled_rows_partly_under_the_header_or_composer_are_not_shown(cx: &mut gpui::TestAppContext) {
        let seen: Seen = Default::default();
        let state = ListState::new(6, ListAlignment::Top, px(0.));
        // Row 0 is scrolled half under the header.
        state.scroll_to(ListOffset { item_ix: 0, offset_in_item: px(60.) });
        let view_seen = seen.clone();
        let (_, cx) = cx.add_window_view(move |_, _| Conversation { state, seen: view_seen });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        let seen = seen.borrow();
        assert!(!seen.is_empty(), "the list laid out its rows");
        for &(ix, top, shown) in seen.iter() {
            let inside = top >= HEADER - 0.5 && top + ROW <= HEADER + LIST + 0.5;
            assert_eq!(shown, inside, "row {ix} at {top}");
        }
        assert!(seen.iter().any(|&(ix, _, shown)| ix == 0 && !shown), "half-hidden row 0 stays native-hidden");
        assert!(seen.iter().any(|&(ix, _, shown)| ix == 1 && shown), "a fully visible row shows");
        assert!(
            seen.iter().all(|&(_, top, shown)| !shown || top + ROW <= HEADER + LIST + 0.5),
            "nothing visible reaches into the composer"
        );
    }

    use gpui::{
        point, AnyView, AppContext, Entity, ScrollDelta, ScrollWheelEvent, StyleRefinement,
    };

    use super::super::web_preview::InlinePlacement;
    use super::{inline_content_height, INLINE_CARD_CHROME};

    /// The host's transcript proof, plus what each row placement decided.
    #[derive(Default)]
    struct Proof {
        placement: InlinePlacement,
        transcript_height: Option<f32>,
        /// (placed, hidden right after the decision), per laid-out result row.
        decisions: Vec<(bool, bool)>,
    }
    type SharedProof = Rc<RefCell<Proof>>;

    const PREFERRED: f32 = 200.;
    const RESULT_ROW: usize = 1;
    /// A short message above the result, so the 320 px result row starts
    /// wholly inside a 400 px transcript.
    const OTHER_ROW: f32 = 40.;
    /// Card chrome above the content inside the row.
    const ABOVE_CONTENT: f32 = 60.;

    /// Wired like `AgentChatTranscript`: begin at render, place or reject from
    /// the result row's content canvas, record height and finish from a probe.
    struct Transcript {
        state: ListState,
        proof: SharedProof,
    }

    impl Render for Transcript {
        fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            self.proof.borrow_mut().placement.begin();
            let content = inline_content_height(PREFERRED, self.proof.borrow().transcript_height);
            let rows = self.proof.clone();
            let probe = self.proof.clone();
            let finish = self.proof.clone();
            div()
                .relative()
                .size_full()
                .child(
                    list(self.state.clone(), move |ix, _, _| {
                        if ix != RESULT_ROW {
                            return div().h(px(OTHER_ROW)).into_any_element();
                        }
                        let Some(content) = content else {
                            // "There isn't room…" with Expand: no surface reserved.
                            return div().h(px(64.)).into_any_element();
                        };
                        let rows = rows.clone();
                        div()
                            .relative()
                            .h(px(content + INLINE_CARD_CHROME - 40.))
                            .child(
                                canvas(
                                    move |bounds, window, _| {
                                        let mut proof = rows.borrow_mut();
                                        let placed = fully_visible(bounds, window.content_mask().bounds);
                                        if placed {
                                            proof.placement.place();
                                        } else {
                                            proof.placement.reject();
                                        }
                                        let hidden = proof.placement.hidden;
                                        proof.decisions.push((placed, hidden));
                                    },
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .top(px(ABOVE_CONTENT))
                                .left(px(12.))
                                .right(px(12.))
                                .h(px(content)),
                            )
                            .into_any_element()
                    })
                    .size_full()
                    .py_5(),
                )
                .child(
                    canvas(
                        move |bounds, _, _| {
                            probe.borrow_mut().transcript_height = Some(f32::from(bounds.size.height));
                        },
                        move |_, _, _, _| finish.borrow_mut().placement.finish(),
                    )
                    .absolute()
                    .inset_0(),
                )
        }
    }

    /// Header, cached transcript and a composer of a given height. Unlike
    /// `CenterArea`, it has no frame-end hook: only the transcript's own
    /// probe can end its placement proof.
    struct Owner {
        transcript: Entity<Transcript>,
        composer: f32,
    }

    impl Render for Owner {
        fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            div()
                .w(px(600.))
                .h(px(HEADER + 400. + 80.))
                .flex()
                .flex_col()
                .child(div().h(px(HEADER)).flex_none())
                .child(AnyView::from(self.transcript.clone()).cached(
                    StyleRefinement::default().flex_1().w_full().min_h(px(0.)),
                ))
                .child(div().h(px(self.composer)).flex_none())
        }
    }

    fn open(
        cx: &mut gpui::TestAppContext,
        composer: f32,
    ) -> (SharedProof, Entity<Transcript>, &mut gpui::VisualTestContext) {
        let proof: SharedProof = Default::default();
        // Plenty of later messages, so the result can scroll wholly away.
        // GPUI bounds scrolling by measured rows; overdraw measures them all.
        let state = ListState::new(30, ListAlignment::Top, px(4000.));
        let view_proof = proof.clone();
        let (owner, cx) = cx.add_window_view(move |_, cx| Owner {
            transcript: cx.new(|_| Transcript { state, proof: view_proof }),
            composer,
        });
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        // The first layout measured the list; re-render the transcript alone
        // (as `record_transcript_height` does) so the card is sized to fit.
        let transcript = owner.read_with(cx, |owner, _| owner.transcript.clone());
        transcript.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        (proof, transcript, cx)
    }

    /// A real wheel event over the list: GPUI's list handler scrolls and
    /// notifies only the view that owns the list (the transcript).
    fn scroll(cx: &mut gpui::VisualTestContext, dy: f32) {
        let position = point(px(300.), px(HEADER + 200.));
        cx.simulate_mouse_move(position, None, gpui::Modifiers::none());
        cx.simulate_event(ScrollWheelEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
            ..Default::default()
        });
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_shown_result_scrolled_partly_or_wholly_out_hides_without_an_owner_frame_end(
        cx: &mut gpui::TestAppContext,
    ) {
        let (proof, transcript, cx) = open(cx, 80.);
        let scroll_top = |cx: &mut gpui::VisualTestContext| {
            transcript.read_with(cx, |t, _| t.state.logical_scroll_top())
        };
        assert!(!proof.borrow().placement.hidden, "fully visible: shown");
        assert_eq!(proof.borrow().decisions.last(), Some(&(true, false)));

        // A small scroll: the content (160 px down) slides 20 px under the header.
        proof.borrow_mut().decisions.clear();
        scroll(cx, -(ABOVE_CONTENT + OTHER_ROW + 80.));
        let top = scroll_top(cx);
        assert!(top.item_ix > 0 || top.offset_in_item > px(0.), "the list scrolled: {top:?}");
        assert!(
            proof.borrow().decisions.iter().any(|&(placed, hidden)| !placed && hidden),
            "the rejected placement hid at once: {:?}",
            proof.borrow().decisions
        );
        assert!(proof.borrow().placement.hidden);

        // Far enough that the row is no longer laid out at all.
        proof.borrow_mut().decisions.clear();
        scroll(cx, -2000.);
        assert!(proof.borrow().decisions.is_empty(), "the row left the list");
        assert!(proof.borrow().placement.hidden, "a layout without the row clears the proof");

        // Back to the top: shown again.
        scroll(cx, 4000.);
        assert!(!proof.borrow().placement.hidden);
        assert_eq!(proof.borrow().decisions.last(), Some(&(true, false)));
    }

    #[gpui::test]
    fn a_tall_composer_shrinks_the_result_or_offers_expand(cx: &mut gpui::TestAppContext) {
        // Moderate draft: 400 + 80 − 160 = 320 px of transcript; the content
        // shrinks below its preferred height and still shows whole.
        let (proof, transcript, cx) = open(cx, 160.);
        let height = proof.borrow().transcript_height.expect("measured");
        assert!((height - 320.).abs() < 1., "transcript {height}");
        assert_eq!(inline_content_height(PREFERRED, Some(height)), Some(160.));
        transcript.update(cx, |t, cx| {
            t.state.scroll_to(ListOffset { item_ix: RESULT_ROW, offset_in_item: px(0.) });
            cx.notify();
        });
        cx.run_until_parked();
        let proof = proof.borrow();
        assert_eq!(proof.decisions.last(), Some(&(true, false)), "placed whole: {:?}", proof.decisions);
        assert!(!proof.placement.hidden);
    }

    #[gpui::test]
    fn no_room_for_the_result_reserves_no_surface(cx: &mut gpui::TestAppContext) {
        // A very tall draft or decision: 400 + 80 − 300 = 180 px of transcript.
        let (proof, _, _) = open(cx, 300.);
        let height = proof.borrow().transcript_height.expect("measured");
        assert_eq!(inline_content_height(PREFERRED, Some(height)), None);
        let proof = proof.borrow();
        assert!(
            proof.decisions.last().is_none_or(|&(placed, _)| !placed),
            "nothing is placed once the fallback is shown"
        );
        assert!(proof.placement.hidden, "no native surface without room");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, px, size};

    fn document(value: serde_json::Value) -> VisualDocument {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn read_only_results_go_to_the_conversation_and_choices_to_the_composer() {
        let table = document(serde_json::json!({
            "title":"Projects", "summary":"Sample data", "kind":"table",
            "openui_lang":"root = Answer([t])\nt = DataTable(\"projects\")",
            "actions":[{"id":"week","label":"Show the last seven days","kind":"follow_up"}]
        }));
        assert_eq!(
            VisualPlacement::of(&table),
            VisualPlacement::Conversation,
            "an optional follow-up is not a question"
        );
        let choices = document(serde_json::json!({
            "title":"Directions", "summary":"Two styles", "kind":"design_options",
            "openui_lang":"root = Answer([c])\nc = DesignOptions()",
            "options":[{"id":"a","title":"A","description":"","html":"<b>a</b>"},
                       {"id":"b","title":"B","description":"","html":"<b>b</b>"}],
            "actions":[{"id":"use","label":"Use this style","kind":"select"}]
        }));
        assert_eq!(VisualPlacement::of(&choices), VisualPlacement::Composer);
    }

    #[test]
    fn docked_information_stays_in_its_card_and_expand_uses_the_full_column() {
        use VisualPanelMode::*;
        use VisualPlacement::*;
        assert_eq!(visual_slot(Conversation, Docked), Some(VisualSlot::TranscriptCard));
        assert_eq!(visual_slot(Composer, Docked), Some(VisualSlot::Composer));
        assert_eq!(visual_slot(Conversation, Expanded), Some(VisualSlot::Expanded));
        assert_eq!(visual_slot(Composer, Expanded), Some(VisualSlot::Expanded));
        assert_eq!(visual_slot(Conversation, Collapsed), None);
        assert_eq!(visual_slot(Composer, Collapsed), None);
    }

    #[test]
    fn a_selected_agent_is_not_proof_its_conversation_is_on_screen() {
        let chat = AgentsScreen {
            agents_mode: true,
            selected_chat: true,
            ..Default::default()
        };
        assert!(chat.shows_selected_conversation());
        for hidden in [
            AgentsScreen { new_agent_composer: true, ..chat },
            AgentsScreen { agents_mode: false, ..chat },
            AgentsScreen { selected_chat: false, ..chat },
            AgentsScreen { panel_replaces_body: true, ..chat },
        ] {
            assert!(!hidden.shows_selected_conversation(), "{hidden:?}");
        }
    }

    #[test]
    fn inline_results_shrink_to_the_transcript_or_offer_expand() {
        assert_eq!(inline_content_height(420.0, None), Some(420.0), "first frame");
        assert_eq!(inline_content_height(420.0, Some(900.0)), Some(420.0));
        // A short window or a tall draft leaves 300 px of transcript.
        assert_eq!(inline_content_height(315.0, Some(300.0)), Some(140.0));
        assert!(inline_content_height(315.0, Some(300.0)).unwrap() + INLINE_CARD_CHROME <= 300.0);
        // Too little room to read anything: Expand, not a hidden surface.
        assert_eq!(inline_content_height(315.0, Some(250.0)), None);
        assert_eq!(inline_content_height(315.0, Some(40.0)), None);
        // A short measured result needs only its own height.
        assert_eq!(inline_content_height(96.0, Some(260.0)), Some(96.0));
        assert_eq!(inline_content_height(96.0, Some(250.0)), None);
    }

    #[test]
    fn partly_scrolled_rows_never_show_their_native_surface() {
        let list = Bounds::new(point(px(0.), px(100.)), size(px(600.), px(500.)));
        let row = |top: f32| Bounds::new(point(px(20.), px(top)), size(px(500.), px(300.)));
        assert!(fully_visible(row(150.), list));
        assert!(fully_visible(row(300.), list), "touching the bottom edge");
        assert!(!fully_visible(row(320.), list), "would cover the composer");
        assert!(!fully_visible(row(80.), list), "would cover the header");
        assert!(!fully_visible(row(900.), list));
        let wide = Bounds::new(point(px(-10.), px(150.)), size(px(700.), px(300.)));
        assert!(!fully_visible(wide, list), "would cover the sidebar");
        assert!(!fully_visible(Bounds::new(point(px(20.), px(150.)), size(px(0.), px(0.))), list));
    }
}
