//! Native chrome around a Visual answer: the composer panel (docked or
//! expanded), the collapsed composer strip and the transcript card. Only the
//! answer content itself is web; header, actions and composer stay native.
use ide_core::visual_answers::{VisualActionKind, VisualAnswer, VisualSubmissionStatus};

use super::agent_chat_visual_answer::{kind_label, VisualAnswerChat, VisualSlot};
use super::agent_chat_visual_answer_attach::{measured_height, ComposerEscape};
use super::agent_chat_visual_answer_placement::inline_content_height;
use super::*;

/// Docked content is at most min(420px, 45% of the workspace height).
const DOCKED_MAX_HEIGHT: f32 = 420.0;
const DOCKED_MIN_HEIGHT: f32 = 140.0;
const EXPANDED_MIN_HEIGHT: f32 = 220.0;
/// A fitted panel is never shorter than this, so it still reads as a panel.
const FITTED_MIN_HEIGHT: f32 = 72.0;
/// The content rectangle's 1px border on each side.
const CONTENT_BORDER: f32 = 2.0;

fn docked_content_height(viewport_height: f32) -> f32 {
    (viewport_height * 0.45).clamp(DOCKED_MIN_HEIGHT, DOCKED_MAX_HEIGHT)
}

/// Fit the docked rectangle to what the renderer measured (no blank space
/// under short content), never beyond the workspace bound; taller content
/// scrolls inside with a visible scrollbar. Unmeasured, use the bound.
fn fitted_content_height(measured: Option<f32>, bound: f32) -> f32 {
    measured
        .map(|height| (height + CONTENT_BORDER).clamp(FITTED_MIN_HEIGHT.min(bound), bound))
        .unwrap_or(bound)
}

fn element_key(answer: &VisualAnswer) -> u64 {
    answer.id.as_u128() as u64
}

fn answer_icon(answer: &VisualAnswer) -> IconName {
    use ide_core::visual_answers::VisualKind;
    match answer.document.kind {
        VisualKind::DesignOptions => IconName::Frame,
        VisualKind::Table => IconName::LayoutDashboard,
        _ => IconName::ChartPie,
    }
}

/// One quiet line under the content describing what Send will do.
fn footer_caption(entry: &VisualAnswerChat, answer: &VisualAnswer) -> String {
    if let Some(submission) = &answer.interaction.submission {
        return match submission.status {
            VisualSubmissionStatus::Sent => "Sent. This revision is kept as a record; you can still filter and inspect it.".into(),
            VisualSubmissionStatus::Sending if entry.sending_current() => "Sending…".into(),
            VisualSubmissionStatus::Sending => "Choro couldn’t confirm this message was sent. Check the conversation before sending it again.".into(),
        };
    }
    let selected = answer
        .interaction
        .selected_option_id
        .as_ref()
        .and_then(|id| answer.document.options.iter().find(|o| &o.id == id));
    match (answer.document.options.is_empty(), selected) {
        (false, Some(option)) => format!(
            "{} selected. Nothing is sent until you choose an action.",
            option.title
        ),
        (false, None) => "Select a direction, or describe a change below.".into(),
        // A result, not a question: the composer below is ordinary chat.
        (true, _) => "Searching and sorting stay on this screen. Nothing is sent.".into(),
    }
}

/// What the reserved content rectangle shows beneath (or instead of) WebKit.
fn content_placeholder(hosted: bool, ready: bool, inline: bool) -> (bool, &'static str) {
    match (hosted, ready, inline) {
        // Another surface (for example a teammate's Preview in the side
        // panel) holds the shared view; it is never displaced.
        (false, ..) => (false, "This visual answer appears here when the side panel’s preview closes."),
        // Loaded, but the row is partly scrolled out, so WebKit stays hidden
        // rather than floating over the header or composer.
        (true, true, true) => (false, "Scroll to show the whole result."),
        (true, ..) => (true, "Loading visual answer…"),
    }
}

impl CenterArea {
    /// The answer panel at the top of the composer card, above the native
    /// text box and footer. Expanded uses the same panel at full height.
    /// An informational result docked in its transcript card is not here.
    pub(super) fn render_visual_answer_panel(
        &mut self,
        agent: &AgentRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let slot = self.visual_answer_slot(agent.id, cx)?;
        if slot == VisualSlot::TranscriptCard {
            return None;
        }
        let agent_id = agent.id;
        let key = element_key(self.visual_answer_chat(agent_id)?.current_answer()?);
        let expanded = slot == VisualSlot::Expanded;
        let body = self.render_visual_answer_body(agent, slot, window, cx)?;
        Some(
            body.id(("visual-answer-panel", key))
                .pb_2p5()
                .mb_0p5()
                .border_b_1()
                .border_color(crate::ui::design::line(cx).opacity(0.6))
                .when(expanded, |panel| panel.flex_1().min_h(px(0.)))
                // Focus on the panel's own controls (no `Input` context, so
                // the composer's Escape action never fires): this is that
                // press's only route into the composer's Escape handler.
                .on_key_down(cx.listener(move |this, event: &gpui::KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape"
                        && !event.is_held
                        && this.composer_escape_from_outside_input(agent_id, window, cx)
                            != ComposerEscape::Unhandled
                    {
                        cx.stop_propagation();
                    }
                }))
                .into_any_element(),
        )
    }

    /// Header, reserved content rectangle and action row of the open answer,
    /// shared by the composer panel and the transcript card.
    fn render_visual_answer_body(
        &self,
        agent: &AgentRecord,
        slot: VisualSlot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::Div> {
        let entry = self.visual_answer_displayed(agent.id, cx)?;
        let answer = entry.current_answer()?.clone();
        let expanded = slot == VisualSlot::Expanded;
        let inline = slot == VisualSlot::TranscriptCard;
        let surface = entry.surface;
        let host_state = self.web_host.read(cx);
        let hosted = host_state.hosts_visual_answer(surface);
        let ready = host_state.visual_answer_ready(surface);
        let (spinner, placeholder) = content_placeholder(hosted, ready, inline);
        let sending = entry.sending_current();
        let uncertain = entry.uncertain_submission();
        let caption = footer_caption(entry, &answer);
        // A send error first; otherwise a note or state that couldn't be saved.
        let error = entry.error.clone().or_else(|| entry.note_error.clone());
        let invalid = !entry.render_errors.is_empty();
        let key = element_key(&answer);
        let agent_id = agent.id;
        let host = self.web_host.clone();
        let preferred_height = fitted_content_height(
            measured_height(entry),
            docked_content_height(f32::from(window.viewport_size().height)),
        );
        // Inline, the result is shown only while wholly inside the transcript,
        // so it is sized to fit it; if nothing readable fits, offer Expand.
        let content_height = if inline {
            inline_content_height(preferred_height, entry.transcript_height)
        } else {
            Some(preferred_height)
        };
        if inline && content_height.is_none() {
            return Some(self.render_visual_answer_too_small(agent_id, &answer, cx));
        }
        let content_height = content_height.unwrap_or(preferred_height);

        let header = h_flex()
            .w_full()
            .gap_2p5()
            .items_center()
            .child(
                div()
                    .flex_none()
                    .size(px(28.))
                    .rounded(crate::ui::design::r_sm())
                    .bg(crate::ui::design::accent_soft(cx))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        Icon::new(answer_icon(&answer))
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::accent(cx)),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(crate::ui::style::focus_text(cx))
                            .child(answer.document.title.clone()),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(answer.document.summary.clone()),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .bg(crate::ui::design::surface_2(cx))
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(if answer.revision > 1 {
                        format!("Revision {}", answer.revision)
                    } else {
                        kind_label(&answer).to_string()
                    }),
            )
            .child(
                crate::ui::style::header_icon_button(
                    ("visual-answer-expand", key),
                    if expanded {
                        IconName::Minimize
                    } else {
                        IconName::Maximize
                    },
                    cx,
                )
                .tooltip(if expanded { "Dock (Esc)" } else { "Expand" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_visual_panel_expanded(agent_id, !expanded, cx);
                })),
            )
            // A choice is hidden or detached from the composer band; only a
            // result in its transcript card collapses from its own header.
            .when(inline, |header| {
                header.child(
                    crate::ui::style::header_icon_button(
                        ("visual-answer-collapse", key),
                        IconName::ChevronDown,
                        cx,
                    )
                    .tooltip("Collapse visual answer")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.collapse_visual_answer(agent_id, cx);
                    })),
                )
            });

        let content = div()
            .relative()
            .w_full()
            .when(expanded, |content| content.flex_1().min_h(px(EXPANDED_MIN_HEIGHT)))
            .when(!expanded, |content| content.h(px(content_height)))
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::base(cx))
            .overflow_hidden()
            .child(
                v_flex()
                    .size_full()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .px_4()
                    .text_color(crate::ui::design::t3(cx))
                    .when(spinner, |state| {
                        state.child(logo_spinner(16., "visual-answer", key as usize, crate::ui::design::t3(cx)))
                    })
                    .child(div().text_size(crate::ui::design::text_ui()).child(placeholder)),
            )
            .child(
                // Reserve the rectangle for the shared native surface. The
                // composer keeps working while the content loads. Inline, the
                // host shows it only while this whole rectangle is visible.
                canvas(
                    move |bounds, window, cx| {
                        host.update(cx, |host, _| host.place_visual_answer(surface, bounds, window));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top(px(1.))
                .left(px(1.))
                .right(px(1.))
                .bottom(px(1.)),
            );

        let footer =
            self.render_visual_answer_actions(agent_id, &answer, sending, uncertain, invalid, cx);
        Some(
            v_flex()
                .w_full()
                .min_w(px(0.))
                .gap_2p5()
                .child(header)
                .child(content)
                .child(
                    v_flex()
                        .w_full()
                        .gap_1p5()
                        .child(
                            h_flex()
                                .w_full()
                                .gap_2()
                                .items_center()
                                .flex_wrap()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(160.))
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(if uncertain {
                                            crate::ui::design::amber(cx)
                                        } else {
                                            crate::ui::design::t3(cx)
                                        })
                                        .child(caption),
                                )
                                .child(footer),
                        )
                        .when_some(error, |col, error| {
                            col.child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(error),
                            )
                        })
                        .when_some(
                            answer
                                .interaction
                                .submission_error
                                .clone()
                                .filter(|_| answer.interaction.submission.is_none()),
                            |col, error| {
                                col.child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::rose(cx))
                                        .child(error),
                                )
                            },
                        ),
                ),
        )
    }

    /// The transcript is too short (small window or a tall composer) to show
    /// this result whole. No native surface is reserved; Expand gives it the
    /// whole conversation column.
    fn render_visual_answer_too_small(
        &self,
        agent_id: Uuid,
        answer: &VisualAnswer,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let key = element_key(answer);
        v_flex()
            .w_full()
            .min_w(px(0.))
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .gap_2p5()
                    .items_center()
                    .child(
                        Icon::new(answer_icon(answer))
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::accent(cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(answer.document.title.clone()),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("There isn’t room to show this result here."),
                            ),
                    )
                    .child(
                        crate::ui::style::secondary_button_compact(
                            ("visual-answer-expand-to-fit", key),
                            "Expand",
                        )
                        .icon(IconName::Maximize)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_visual_panel_expanded(agent_id, true, cx);
                        })),
                    )
                    .child(
                        crate::ui::style::header_icon_button(
                            ("visual-answer-collapse", key),
                            IconName::ChevronDown,
                            cx,
                        )
                        .tooltip("Collapse visual answer")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.collapse_visual_answer(agent_id, cx);
                        })),
                    ),
            )
    }

    fn render_visual_answer_actions(
        &self,
        agent_id: Uuid,
        answer: &VisualAnswer,
        sending: bool,
        uncertain: bool,
        invalid: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let key = element_key(answer);
        let row = h_flex().flex_none().gap_2().items_center();
        if uncertain {
            return row
                .child(
                    crate::ui::style::secondary_button_compact(
                        ("visual-answer-confirm-sent", key),
                        "It was sent",
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.confirm_visual_submission_sent(agent_id, cx)
                    })),
                )
                .child(
                    crate::ui::style::primary_button_compact(
                        ("visual-answer-resend", key),
                        "Send again",
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.resend_visual_submission(agent_id, window, cx)
                    })),
                )
                .into_any_element();
        }
        if answer.interaction.submission.is_some() {
            return row.into_any_element();
        }
        if sending {
            return row
                .child(crate::ui::style::busy_button_compact(
                    ("visual-answer-sending", key),
                    "Sending…",
                    cx,
                ))
                .into_any_element();
        }
        let has_selection = answer.interaction.selected_option_id.is_some();
        let mut row = row;
        for (index, action) in answer.document.actions.iter().enumerate() {
            let action_id = action.id.clone();
            let id = ("visual-answer-action", key.wrapping_add(index as u64));
            row = match action.kind {
                // Refinements are sent from the composer with Enter.
                VisualActionKind::Refine => row,
                VisualActionKind::FollowUp => row.child(
                    crate::ui::style::secondary_button_compact(id, action.label.clone())
                        .disabled(invalid)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.submit_visual_action(agent_id, action_id.clone(), window, cx);
                        })),
                ),
                VisualActionKind::Select => row.child(
                    crate::ui::style::primary_button_compact(id, "Use this style", cx)
                        .disabled(!has_selection || invalid)
                        .tooltip(if has_selection {
                            "Send your selection and any note below"
                        } else {
                            "Select a direction first"
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.submit_visual_action(agent_id, action_id.clone(), window, cx);
                        })),
                ),
            };
        }
        row.into_any_element()
    }

    /// The transcript's record of a publication: title, kind and a way back.
    /// An open informational result is shown live inside this card.
    pub(super) fn render_visual_answer_card(
        &self,
        agent: &AgentRecord,
        answer_id: Uuid,
        stored_title: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let entry = self.visual_answer_chat(agent_id);
        let current = entry.is_some_and(|entry| entry.current == Some(answer_id));
        let slot = current
            .then(|| self.visual_answer_slot(agent_id, cx))
            .flatten();
        if slot == Some(VisualSlot::TranscriptCard) {
            if let Some(body) = self.render_visual_answer_body(agent, VisualSlot::TranscriptCard, window, cx) {
                return body
                    .max_w(crate::ui::design::agent_chat_content_max_w())
                    .p_3()
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::surface(cx))
                    .into_any_element();
            }
        }
        let answer = entry.and_then(|entry| entry.answers.get(&answer_id));
        let showing = slot.map(|slot| match slot {
            VisualSlot::Expanded => "Expanded",
            _ => "Shown in the composer",
        });
        let title = answer
            .map(|a| a.document.title.clone())
            .unwrap_or_else(|| stored_title.to_string());
        let detail = match answer {
            Some(answer) => {
                let mut detail = kind_label(answer).to_string();
                if answer.revision > 1 {
                    detail.push_str(&format!(", revision {}", answer.revision));
                }
                match answer.interaction.submission.as_ref().map(|s| s.status) {
                    Some(VisualSubmissionStatus::Sent) => detail.push_str(". Sent"),
                    Some(VisualSubmissionStatus::Sending) => detail.push_str(". Sending"),
                    None => {}
                }
                detail
            }
            None => "Visual answer".to_string(),
        };
        let notice = entry
            .and_then(|entry| entry.notice.as_ref())
            .filter(|(id, _)| *id == answer_id && self.visual_answer_preview_blocks(agent_id, cx))
            .map(|(_, notice)| notice.clone());
        let key = answer_id.as_u128() as u64;
        v_flex()
            .w_full()
            .max_w(crate::ui::design::agent_chat_content_max_w())
            .gap_1p5()
            .px_3()
            .py_2p5()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::surface(cx))
            .child(
                h_flex()
                    .w_full()
                    .gap_2p5()
                    .items_center()
                    .child(
                        Icon::new(answer.map(answer_icon).unwrap_or(IconName::ChartPie))
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::accent(cx)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(detail),
                            ),
                    )
                    .child(if let Some(showing) = showing {
                        div()
                            .flex_none()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(showing)
                            .into_any_element()
                    } else {
                        crate::ui::style::secondary_button_compact(
                            ("visual-answer-card-open", key),
                            "Open visual answer",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_visual_answer(agent_id, answer_id, cx)
                        }))
                        .into_any_element()
                    }),
            )
            .when_some(notice, |card, notice| {
                card.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::amber(cx))
                        .child(notice),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{docked_content_height, fitted_content_height};

    #[test]
    fn docked_content_is_bounded_by_the_workspace() {
        assert_eq!(docked_content_height(1200.0), 420.0);
        assert_eq!(docked_content_height(700.0), 315.0);
        assert_eq!(docked_content_height(200.0), 140.0);
    }

    #[test]
    fn docked_content_fits_what_the_renderer_measured() {
        // Four compact buttons: no blank space under the options.
        assert_eq!(fitted_content_height(Some(250.0), 420.0), 252.0);
        // A full-page comparison is capped; it scrolls inside.
        assert_eq!(fitted_content_height(Some(1400.0), 420.0), 420.0);
        // Never a sliver, never beyond the bound, unmeasured uses the bound.
        assert_eq!(fitted_content_height(Some(10.0), 420.0), 72.0);
        assert_eq!(fitted_content_height(Some(10.0), 60.0), 60.0);
        assert_eq!(fitted_content_height(None, 315.0), 315.0);
    }
}
