//! A choice attached to the composer: its place in the composer's Band row
//! (beside the delegation context), detach and reopen, the one set of Escape
//! rules shared by the composer and the answer's web content, and the
//! measured height a docked answer is fitted to.
use ide_core::visual_answers::VisualSubmissionStatus;

use super::agent_chat_visual_answer::{
    VisualAnswerChat, VisualPanelMode, VisualPlacement, VisualSlot,
};
use super::*;

/// What one Escape press does around an open answer. Running work always
/// wins, so a visual answer can never swallow Stop / Force stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VisualEscape {
    /// The conversation is Running or Cancelling: the normal stop path
    /// (first press graceful, second press force).
    StopWork,
    /// Return an expanded answer to its docked place.
    Dock,
    /// Leave a docked choice: it collapses into the composer band and the
    /// composer is ordinary chat again. Nothing is discarded or sent.
    Leave,
    /// Nothing visual to do; the composer's own Escape handling applies.
    Pass,
}

pub(super) fn visual_escape_step(
    working: bool,
    slot: Option<VisualSlot>,
    takes_composer: bool,
) -> VisualEscape {
    if working {
        VisualEscape::StopWork
    } else if slot == Some(VisualSlot::Expanded) {
        VisualEscape::Dock
    } else if takes_composer {
        VisualEscape::Leave
    } else {
        VisualEscape::Pass
    }
}

/// What the composer area did with one Escape press.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ComposerEscape {
    /// A pending handoff or the context picker took it.
    Swallowed,
    /// Stop, dock or leave (see `VisualEscape`).
    Visual(VisualEscape),
    /// Nothing here wanted it; the focused control's own Escape applies.
    Unhandled,
}

/// How the composer's Band row shows this chat's answer, if at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BandAttachment {
    /// Shown in the composer panel (docked or expanded).
    Open,
    /// Waiting in the band; Open brings it back.
    Collapsed,
}

/// Only an unsent choice the user hasn't detached belongs to the composer.
/// Informational results live in the transcript; sent and detached answers
/// stay there too and reopen from their card.
pub(super) fn band_attachment(entry: &VisualAnswerChat) -> Option<BandAttachment> {
    let answer = entry.current_answer()?;
    let sent = answer
        .interaction
        .submission
        .as_ref()
        .is_some_and(|s| s.status == VisualSubmissionStatus::Sent);
    if VisualPlacement::of(&answer.document) != VisualPlacement::Composer
        || answer.interaction.detached
        || sent
    {
        return None;
    }
    Some(if entry.mode == VisualPanelMode::Collapsed {
        BandAttachment::Collapsed
    } else {
        BandAttachment::Open
    })
}

/// Heights the renderer reports are applied only to the revision that sent them.
pub(super) fn measured_height(entry: &VisualAnswerChat) -> Option<f32> {
    let answer = entry.current_answer()?;
    entry
        .content_height
        .filter(|(id, revision, _)| *id == answer.id && *revision == answer.revision)
        .map(|(.., height)| height)
}

impl CenterArea {
    fn agent_chat_working(&self, agent_id: Uuid, cx: &App) -> bool {
        self.agent_chats
            .read(cx)
            .session(agent_id)
            .is_some_and(|session| {
                matches!(
                    session.status,
                    AgentChatStatus::Running | AgentChatStatus::Cancelling
                )
            })
    }

    /// Decide and apply the visual part of one Escape press. `StopWork` and
    /// `Pass` are left to the caller, which owns the stop path.
    pub(super) fn apply_visual_escape(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> VisualEscape {
        let step = visual_escape_step(
            self.agent_chat_working(agent_id, cx),
            self.visual_answer_slot(agent_id, cx),
            self.visual_answer_takes_composer(agent_id, cx),
        );
        match step {
            VisualEscape::Dock => self.set_visual_panel_expanded(agent_id, false, cx),
            VisualEscape::Leave => self.collapse_visual_answer(agent_id, cx),
            VisualEscape::StopWork | VisualEscape::Pass => {}
        }
        step
    }

    /// The one Escape handler of a conversation's composer area. Exactly one
    /// route delivers each press here, chosen by where focus is:
    /// - the composer text box: gpui-component binds `escape` to the `Escape`
    ///   action only in its `Input` context, captured by the composer frame;
    /// - the answer panel's native controls (outside any `Input`, so the
    ///   action never fires there): the panel's key-down listener;
    /// - the answer's web content (WebKit owns the keyboard): renderer IPC.
    /// Order: a pending handoff swallows it, the context picker closes, then
    /// `visual_escape_step` (running work stops before anything visual).
    pub(super) fn composer_escape(
        &mut self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> ComposerEscape {
        if self.agent_handoff_busy(agent.id) {
            return ComposerEscape::Swallowed;
        }
        if self.dismiss_agent_chat_context_picker(agent, cx) {
            return ComposerEscape::Swallowed;
        }
        match self.apply_visual_escape(agent.id, cx) {
            VisualEscape::Pass => ComposerEscape::Unhandled,
            VisualEscape::StopWork => {
                // First press requests a graceful stop; pressing again while
                // Cancelling follows the same force-stop path as a second
                // click on the stop control.
                self.request_agent_chat_stop(agent.id, cx);
                ComposerEscape::Visual(VisualEscape::StopWork)
            }
            step => ComposerEscape::Visual(step),
        }
    }

    /// Escape from a route whose focus does not stay in the composer text box
    /// (panel controls or web content): afterwards focus returns to the
    /// composer, unless the answer only docked and is still on screen.
    pub(super) fn composer_escape_from_outside_input(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ComposerEscape {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return ComposerEscape::Unhandled;
        };
        let outcome = self.composer_escape(&agent, cx);
        if outcome != ComposerEscape::Visual(VisualEscape::Dock) {
            self.focus_agent_composer(agent_id, window, cx);
        }
        outcome
    }

    pub(super) fn focus_agent_composer(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = self.agent_chat_inputs.get(&agent_id).cloned() {
            web_preview::restore_focus(window);
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    /// X in the composer band: the answer leaves the composer but stays in
    /// the conversation with its selection and note. Composer text is kept;
    /// the answer reopens only from its transcript card, never on its own
    /// (a new revision from the agent is a new question and attaches again).
    pub(super) fn detach_visual_answer(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        let surface = entry.surface;
        let open = entry.mode != VisualPanelMode::Collapsed;
        entry.mode = VisualPanelMode::Collapsed;
        entry.notice = None;
        let current = entry.current;
        if open {
            // Pending selection/scroll is sent before the surface goes away.
            self.web_host
                .update(cx, |host, _| host.flush_visual_answer(surface));
        }
        if let Some(answer_id) = current {
            self.set_visual_attachment(agent_id, answer_id, true, true, cx);
        }
        self.remeasure_agent_chat_list(agent_id);
        cx.notify();
    }

    /// Record a docked answer's content height. Expanded layouts differ and
    /// are ignored; a docked report follows as soon as it docks again.
    pub(super) fn record_visual_content_height(
        &mut self,
        agent_id: Uuid,
        height: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(entry) = self.visual_answers.get_mut(&agent_id) else {
            return;
        };
        if entry.mode == VisualPanelMode::Expanded {
            return;
        }
        let Some((answer_id, revision)) = entry.current_answer().map(|a| (a.id, a.revision)) else {
            return;
        };
        let height = height.ceil();
        if measured_height(entry).is_some_and(|known| (known - height).abs() < 1.0) {
            return;
        }
        entry.content_height = Some((answer_id, revision, height));
        let inline = entry.slot() == Some(VisualSlot::TranscriptCard);
        if inline {
            self.remeasure_agent_chat_list(agent_id);
        }
        cx.notify();
    }

    /// The composer's Band row: the delegation context (Band status,
    /// recipient, activity, recovery) and the attached choice share one
    /// wrapping row inside the composer. The choice keeps its actions
    /// together when the row wraps; only its title truncates. A Preview
    /// notice for the attached choice follows on its own line.
    pub(super) fn render_composer_context_row(
        &mut self,
        agent: &AgentRecord,
        show_visual: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let visual = show_visual
            .then(|| self.render_visual_band_segment(agent.id, cx))
            .flatten();
        let notice = visual
            .is_some()
            .then(|| self.visual_answer_chat(agent.id))
            .flatten()
            .and_then(|entry| {
                let answer_id = entry.current?;
                entry
                    .notice
                    .as_ref()
                    .filter(|(id, _)| *id == answer_id)
                    .map(|(_, notice)| notice.clone())
            })
            .filter(|_| self.visual_answer_preview_blocks(agent.id, cx));
        let row = if ide_core::delegation::enabled() {
            self.render_expert_composer(agent, visual, cx)
        } else if visual.is_none() {
            return None;
        } else {
            h_flex()
                .gap_2()
                .flex_wrap()
                .items_center()
                .children(visual)
                .into_any_element()
        };
        Some(match notice {
            Some(notice) => v_flex()
                .w_full()
                .gap_1()
                .child(row)
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::amber(cx))
                        .child(notice),
                )
                .into_any_element(),
            None => row,
        })
    }

    /// The attached choice in the Band row: what it is, then Open (or Hide
    /// while it is shown) and X to detach it from the composer.
    fn render_visual_band_segment(
        &self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let entry = self.visual_answer_chat(agent_id)?;
        let state = band_attachment(entry)?;
        let answer = entry.current_answer()?;
        let answer_id = answer.id;
        let key = answer_id.as_u128() as u64;
        let title = answer.document.title.clone();
        let open = state == BandAttachment::Open;
        Some(
            h_flex()
                .flex_none()
                .max_w_full()
                .min_w(px(0.))
                .gap_1p5()
                .items_center()
                .child(
                    Icon::new(IconName::Frame)
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::accent(cx)),
                )
                .child(
                    div()
                        .flex_shrink()
                        .min_w(px(0.))
                        .max_w(px(280.))
                        .truncate()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(if open {
                            crate::ui::design::t1(cx)
                        } else {
                            crate::ui::design::t2(cx)
                        })
                        .child(title),
                )
                .child(if open {
                    crate::ui::style::composer_band_action_button(
                        ("visual-band-hide", key),
                        "Hide",
                        cx,
                    )
                    .icon(IconName::ChevronDown)
                    .tooltip("Hide the choice (Esc). It stays here; the composer sends normal messages.")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.collapse_visual_answer(agent_id, cx);
                    }))
                } else {
                    crate::ui::style::composer_band_action_button(
                        ("visual-band-open", key),
                        "Open",
                        cx,
                    )
                    .icon(IconName::ChevronUp)
                    .tooltip("Show this choice above the composer")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_visual_answer(agent_id, answer_id, cx);
                    }))
                })
                .child(
                    crate::ui::style::header_icon_button(
                        ("visual-band-detach", key),
                        IconName::Close,
                        cx,
                    )
                    .tooltip("Detach from the composer. The answer and your selection stay in the conversation.")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.detach_visual_answer(agent_id, cx);
                    })),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::visual_answers::{VisualAnswer, VisualDocument, VisualInteraction, VisualSubmission};

    #[test]
    fn running_work_always_wins_escape_then_dock_then_leave() {
        use VisualEscape::*;
        for slot in [None, Some(VisualSlot::Composer), Some(VisualSlot::Expanded)] {
            assert_eq!(visual_escape_step(true, slot, true), StopWork, "{slot:?}");
        }
        // Idle: expanded docks first; the next press leaves the choice.
        assert_eq!(visual_escape_step(false, Some(VisualSlot::Expanded), true), Dock);
        assert_eq!(visual_escape_step(false, Some(VisualSlot::Composer), true), Leave);
        assert_eq!(visual_escape_step(false, None, false), Pass);
        // An informational result in its transcript card never takes Escape.
        assert_eq!(visual_escape_step(false, Some(VisualSlot::TranscriptCard), false), Pass);
        // Expanded information docks back into its card.
        assert_eq!(visual_escape_step(false, Some(VisualSlot::Expanded), false), Dock);
    }

    fn chat(document: serde_json::Value, mode: VisualPanelMode) -> VisualAnswerChat {
        let document: VisualDocument = serde_json::from_value(document).unwrap();
        let answer = VisualAnswer {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            agent_id: Uuid::new_v4(),
            revision: 1,
            document,
            interaction: VisualInteraction::default(),
            created_at: 0,
            updated_at: 0,
        };
        let mut entry = VisualAnswerChat::default();
        entry.current = Some(answer.id);
        entry.mode = mode;
        entry.answers.insert(answer.id, answer);
        entry
    }

    fn choice() -> serde_json::Value {
        serde_json::json!({
            "title":"Create project button", "summary":"Two directions", "kind":"design_options",
            "openui_lang":"root = Answer([choices])\nchoices = DesignOptions()",
            "options":[{"id":"solid","title":"Solid","description":"","html":"<b>a</b>"},
                       {"id":"tonal","title":"Tonal","description":"","html":"<b>b</b>"}],
            "actions":[{"id":"use","label":"Use this style","kind":"select"}]
        })
    }

    #[test]
    fn the_band_shows_an_attached_choice_open_or_collapsed_but_never_information() {
        assert_eq!(
            band_attachment(&chat(choice(), VisualPanelMode::Docked)),
            Some(BandAttachment::Open)
        );
        assert_eq!(
            band_attachment(&chat(choice(), VisualPanelMode::Expanded)),
            Some(BandAttachment::Open)
        );
        assert_eq!(
            band_attachment(&chat(choice(), VisualPanelMode::Collapsed)),
            Some(BandAttachment::Collapsed)
        );
        let table = serde_json::json!({
            "title":"Projects", "summary":"Sample", "kind":"table",
            "openui_lang":"root = Answer([t])\nt = DataTable(\"p\")",
            "datasets":{"p":{"columns":[{"key":"name","label":"Name","kind":"text"}],"rows":[{"name":"A"}]}},
            "sources":[{"label":"Sample data"}]
        });
        assert_eq!(band_attachment(&chat(table, VisualPanelMode::Docked)), None);
    }

    #[test]
    fn detached_and_sent_choices_leave_the_band_but_keep_their_state() {
        let mut entry = chat(choice(), VisualPanelMode::Collapsed);
        let id = entry.current.unwrap();
        let answer = entry.answers.get_mut(&id).unwrap();
        answer.interaction.selected_option_id = Some("tonal".into());
        answer.interaction.draft = "Rounder".into();
        answer.interaction.detached = true;
        assert_eq!(band_attachment(&entry), None);
        let kept = &entry.current_answer().unwrap().interaction;
        assert_eq!(kept.selected_option_id.as_deref(), Some("tonal"));
        assert_eq!(kept.draft, "Rounder");

        let mut sent = chat(choice(), VisualPanelMode::Collapsed);
        let id = sent.current.unwrap();
        sent.answers.get_mut(&id).unwrap().interaction.submission = Some(VisualSubmission {
            id: Uuid::new_v4(),
            action_id: "use".into(),
            text: "Use this style: Tonal".into(),
            status: VisualSubmissionStatus::Sent,
        });
        assert_eq!(band_attachment(&sent), None);
    }

    #[test]
    fn a_measured_height_applies_only_to_the_revision_that_reported_it() {
        let mut entry = chat(choice(), VisualPanelMode::Docked);
        let id = entry.current.unwrap();
        entry.content_height = Some((id, 1, 212.0));
        assert_eq!(measured_height(&entry), Some(212.0));
        entry.content_height = Some((id, 0, 212.0));
        assert_eq!(measured_height(&entry), None);
        entry.content_height = Some((Uuid::new_v4(), 1, 212.0));
        assert_eq!(measured_height(&entry), None);
    }
}

/// GPUI dispatch proof for the composer's two native Escape routes: the
/// same wiring as the composer (frame `capture_action(Escape)` around a real
/// gpui-component `Input`) and the answer panel (a focusable sibling with a
/// key-down listener). Each press must reach exactly one route, and apply
/// exactly one `visual_escape_step`.
#[cfg(all(test, feature = "ui-layout-tests"))]
mod escape_routes {
    use super::*;

    struct Composer {
        input: Entity<InputState>,
        panel: gpui::FocusHandle,
        working: bool,
        slot: Option<VisualSlot>,
        stops: usize,
        log: Vec<(&'static str, VisualEscape)>,
    }

    impl Composer {
        /// The model of `composer_escape` for one press.
        fn press(&mut self, route: &'static str) {
            let step = visual_escape_step(self.working, self.slot, self.slot.is_some());
            match step {
                VisualEscape::Dock => self.slot = Some(VisualSlot::Composer),
                VisualEscape::Leave => self.slot = None,
                VisualEscape::StopWork => self.stops += 1,
                VisualEscape::Pass => {}
            }
            self.log.push((route, step));
        }
    }

    impl Render for Composer {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            v_flex()
                .size_full()
                .capture_action(cx.listener(|this, _: &Escape, _, cx| {
                    this.press("composer");
                    cx.stop_propagation();
                }))
                .child(
                    div()
                        .id("panel")
                        .h(px(40.))
                        .track_focus(&self.panel)
                        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" && !event.is_held {
                                this.press("panel");
                                cx.stop_propagation();
                            }
                        })),
                )
                .child(Input::new(&self.input))
        }
    }

    fn open(cx: &mut gpui::TestAppContext) -> (Entity<Composer>, &mut gpui::VisualTestContext) {
        cx.update(gpui_component::init);
        let mut view = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx));
            let composer = cx.new(|cx| Composer {
                input,
                panel: cx.focus_handle(),
                working: false,
                slot: Some(VisualSlot::Expanded),
                stops: 0,
                log: Vec::new(),
            });
            view = Some(composer.clone());
            gpui_component::Root::new(composer, window, cx)
        });
        cx.run_until_parked();
        (view.unwrap(), cx)
    }

    fn focus_input(view: &Entity<Composer>, cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| {
            let input = view.read(cx).input.clone();
            input.update(cx, |input, cx| input.focus(window, cx));
        });
        cx.run_until_parked();
    }

    fn focus_panel(view: &Entity<Composer>, cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| window.focus(&view.read(cx).panel));
        cx.run_until_parked();
    }

    fn take_log(
        view: &Entity<Composer>,
        cx: &mut gpui::VisualTestContext,
    ) -> Vec<(&'static str, VisualEscape)> {
        cx.update(|_, cx| view.update(cx, |view, _| std::mem::take(&mut view.log)))
    }

    #[gpui::test]
    fn each_escape_reaches_one_route_and_takes_one_step(cx: &mut gpui::TestAppContext) {
        let (view, cx) = open(cx);
        use VisualEscape::*;

        // Text box focused: only the composer's action route.
        focus_input(&view, cx);
        cx.simulate_keystrokes("escape");
        assert_eq!(take_log(&view, cx), [("composer", Dock)], "expanded only docks");
        cx.simulate_keystrokes("escape");
        assert_eq!(take_log(&view, cx), [("composer", Leave)], "the second press leaves");

        // Panel control focused: the action never fires; only the panel route.
        cx.update(|_, cx| view.update(cx, |view, _| view.slot = Some(VisualSlot::Expanded)));
        focus_panel(&view, cx);
        cx.simulate_keystrokes("escape");
        assert_eq!(take_log(&view, cx), [("panel", Dock)]);

        // Running or Cancelling: one stop per press from either route; the
        // answer is untouched.
        cx.update(|_, cx| view.update(cx, |view, _| view.working = true));
        cx.simulate_keystrokes("escape escape");
        assert_eq!(take_log(&view, cx), [("panel", StopWork), ("panel", StopWork)]);
        focus_input(&view, cx);
        cx.simulate_keystrokes("escape");
        assert_eq!(take_log(&view, cx), [("composer", StopWork)]);
        let (stops, slot) = cx.update(|_, cx| {
            let view = view.read(cx);
            (view.stops, view.slot)
        });
        assert_eq!(stops, 3);
        assert_eq!(slot, Some(VisualSlot::Composer));
    }
}
