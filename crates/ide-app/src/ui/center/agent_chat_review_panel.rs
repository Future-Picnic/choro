//! The composer's place while an independent reviewer holds a conversation.
//!
//! The draft editor, attachments and pasted blocks stay in their per-agent
//! maps untouched; only the frame is swapped for this panel. When the reviewer
//! process stops (for any reason) the composer renders again and, if focus was
//! still in the panel, the draft editor takes it back.
use super::agent_chat_review_status::{review_progress, ReviewProgress};
use super::*;
use gpui::Focusable as _;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ReviewUiState {
    panel_focus: HashMap<Uuid, gpui::FocusHandle>,
    /// Conversations whose last render showed the review panel.
    held: HashSet<Uuid>,
    pub errors: HashMap<Uuid, String>,
    fix_checks: HashSet<(Uuid, String)>,
    pub fix_notices: HashMap<(Uuid, String), String>,
    pub detail_panels: HashSet<(Uuid, String)>,
}

impl ReviewUiState {
    pub fn panel_focus(&mut self, agent_id: Uuid, cx: &mut App) -> gpui::FocusHandle {
        self.panel_focus
            .entry(agent_id)
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    /// Keep keyboard focus inside the composer area: a focused draft hands
    /// focus to the panel so Escape and Tab still reach Cancel review.
    pub fn hold_focus(
        &mut self,
        agent_id: Uuid,
        input_focused: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if input_focused {
            let handle = self.panel_focus(agent_id, cx);
            window.focus(&handle);
        }
    }

    /// Record whether this render shows the panel. Returns true exactly once,
    /// on the first render after the reviewer stopped, when the draft editor
    /// should take focus back because the user had not moved it elsewhere.
    pub fn sync_hold(&mut self, agent_id: Uuid, holding: bool, window: &Window) -> bool {
        if holding {
            self.held.insert(agent_id);
            return false;
        }
        self.held.remove(&agent_id)
            && self
                .panel_focus
                .get(&agent_id)
                .is_some_and(|handle| handle.is_focused(window))
    }

    pub fn fix_checking(&self, agent_id: Uuid, review_id: &str) -> bool {
        self.fix_checks.contains(&(agent_id, review_id.to_string()))
    }
}

/// What the held composer still contains, so the panel can say it is safe.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct HeldDraft {
    pub text: bool,
    pub attachments: usize,
}

impl HeldDraft {
    fn message(self) -> Option<String> {
        let held = match (self.text, self.attachments) {
            (false, 0) => return None,
            (true, 0) => return Some("Your draft returns when the review ends.".into()),
            (false, 1) => return Some("Your attachment returns when the review ends.".into()),
            (false, n) => format!("Your {n} attachments"),
            (true, 1) => "Your draft and attachment".to_string(),
            (true, n) => format!("Your draft and {n} attachments"),
        };
        Some(format!("{held} return when the review ends."))
    }
}

/// Outcome of the freshness check that must pass before findings are fixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum FixPermission {
    Allowed,
    Blocked(String),
}

pub(super) async fn fix_permission(
    receiver: async_channel::Receiver<anyhow::Result<bool>>,
) -> FixPermission {
    match receiver.recv().await {
        Ok(Ok(true)) => FixPermission::Allowed,
        Ok(Ok(false)) => FixPermission::Blocked(
            "These findings may no longer match the code. Review the latest changes before fixing."
                .into(),
        ),
        Ok(Err(error)) => FixPermission::Blocked(format!(
            "Couldn't confirm this review matches the current code: {error:#}"
        )),
        Err(_) => FixPermission::Blocked(
            "Couldn't confirm this review matches the current code. Try again.".into(),
        ),
    }
}

/// The panel element. `on_cancel` runs from the button and from Escape.
pub(super) fn review_panel(
    agent_id: Uuid,
    progress: &ReviewProgress,
    draft: HeldDraft,
    focus: &gpui::FocusHandle,
    on_cancel: Rc<dyn Fn(&mut Window, &mut App)>,
    cx: &App,
) -> gpui::AnyElement {
    let key = agent_id.as_u128() as u64;
    let teal = crate::ui::design::teal(cx);
    let escape_cancel = on_cancel.clone();
    let cancelling = progress.cancelling;
    style::review_panel_frame(cx)
        .id(("agent-review-panel", key))
        .debug_selector(|| "review-panel".into())
        .track_focus(focus)
        .on_key_down(move |event: &gpui::KeyDownEvent, window, cx| {
            if event.keystroke.key == "escape" && !cancelling {
                cx.stop_propagation();
                escape_cancel(window, cx);
            }
        })
        .child(
            h_flex()
                .w_full()
                .min_w(px(0.))
                .items_center()
                .gap_3()
                .child(crate::ui::logo_spinner::review_spinner(
                    18.,
                    "agent-review-panel-spinner",
                    key as usize,
                    teal,
                ))
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(
                            h_flex()
                                .min_w(px(0.))
                                .items_baseline()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child(progress.title),
                                )
                                .child(
                                    div()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(progress.scope.clone()),
                                ),
                        )
                        .child(
                            h_flex()
                                .min_w(px(0.))
                                .items_center()
                                .gap_3()
                                .text_size(crate::ui::design::text_ui())
                                .child(
                                    div()
                                        .flex_none()
                                        .debug_selector(|| "review-panel-detail".into())
                                        .text_color(teal)
                                        .child(progress.detail.clone()),
                                )
                                .when_some(progress.group.clone(), |row, group| {
                                    row.child(
                                        div()
                                            .min_w(px(0.))
                                            .truncate()
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(format!("Now in {group}")),
                                    )
                                })
                                .when(progress.findings > 0, |row| {
                                    row.child(
                                        div()
                                            .flex_none()
                                            .text_color(crate::ui::design::t2(cx))
                                            .child(if progress.findings == 1 {
                                                "1 finding so far".to_string()
                                            } else {
                                                format!("{} findings so far", progress.findings)
                                            }),
                                    )
                                }),
                        )
                        .when_some(progress.open_changes.clone(), |column, path| {
                            column.child(div().min_w(px(0.)).truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .debug_selector(|| "review-open-changes".into())
                                .child(format!("Open changes: {path}")))
                        }),
                )
                .child(
                    style::dialog_neutral_button(
                        ("agent-review-cancel", key),
                        if cancelling { "Cancelling…" } else { "Cancel review" },
                        cx,
                    )
                    .flex_none()
                    .disabled(cancelling)
                    .tooltip(if cancelling {
                        "Waiting for the reviewer to stop"
                    } else {
                        "Stop the reviewer (Esc). Your draft returns when it stops"
                    })
                    .on_click(move |_, window, cx| on_cancel(window, cx)),
                ),
        )
        .when_some(draft.message(), |frame, message| {
            frame.child(
                h_flex()
                    .pl(px(30.))
                    .items_center()
                    .gap_1p5()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child(crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::NotebookPen,
                        crate::ui::design::t3(cx),
                        crate::ui::design::icon_sm(),
                    ))
                    .child(message),
            )
        })
        .into_any_element()
}

/// The composer's replacement for one render, or `None` when the composer
/// should render. Returning focus happens here, on the first render after the
/// reviewer stops, so every exit path (complete, cancelled, failed, timed
/// out, interrupted) restores the same way.
#[allow(clippy::too_many_arguments)]
pub(super) fn review_hold(
    ui: &mut ReviewUiState,
    chats: &Entity<AgentChatState>,
    agent_id: Uuid,
    draft: HeldDraft,
    input: &Entity<InputState>,
    on_cancel: Rc<dyn Fn(&mut Window, &mut App)>,
    window: &mut Window,
    cx: &mut App,
) -> Option<gpui::AnyElement> {
    let (holding, run) = {
        let chats = chats.read(cx);
        (
            chats.review_blocks_writing(agent_id),
            chats.review_run(agent_id).cloned(),
        )
    };
    if ui.sync_hold(agent_id, holding, window) {
        input.update(cx, |input, cx| input.focus(window, cx));
    }
    if !holding {
        return None;
    }
    let run = run?;
    let focus = ui.panel_focus(agent_id, cx);
    Some(review_panel(
        agent_id,
        &review_progress(&run),
        draft,
        &focus,
        on_cancel,
        cx,
    ))
}

impl CenterArea {
    /// Start an independent review. The backend reserves synchronously and
    /// captures off the UI thread; an error leaves the composer untouched.
    pub(super) fn start_agent_review(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input_focused = self
            .agent_chat_inputs
            .get(&agent_id)
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window));
        if self.request_agent_code_review(agent_id, cx) {
            self.agent_review_ui
                .hold_focus(agent_id, input_focused, window, cx);
        }
    }

    pub(super) fn cancel_agent_review(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.agent_chats
            .update(cx, |chats, cx| chats.cancel_review(agent_id, cx));
    }

    /// Fix only after the backend confirms the review still matches the code.
    /// Legacy Markdown reviews carry no snapshot and are allowed through.
    pub(super) fn request_validated_code_review_fix(
        &mut self,
        agent_id: Uuid,
        review_id: String,
        only_selected: bool,
        cx: &mut Context<Self>,
    ) {
        let key = (agent_id, review_id.clone());
        if self.agent_chats.read(cx).review_blocks_writing(agent_id) {
            self.agent_review_ui.fix_notices.insert(
                key,
                "A review is running in this conversation. Fix after it ends.".into(),
            );
            cx.notify();
            return;
        }
        if !self.agent_review_ui.fix_checks.insert(key.clone()) {
            return;
        }
        self.agent_review_ui.fix_notices.remove(&key);
        let receiver = self.agent_chats.update(cx, |chats, cx| {
            chats.validate_review_for_fix(agent_id, review_id.clone(), cx)
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let permission = fix_permission(receiver).await;
            let _ = this.update(cx, |this, cx| {
                this.agent_review_ui.fix_checks.remove(&key);
                match permission {
                    FixPermission::Allowed => {
                        this.request_agent_code_review_fix(agent_id, review_id, only_selected, cx)
                    }
                    FixPermission::Blocked(message) => {
                        this.agent_review_ui.fix_notices.insert(key, message);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The panel for `agent_id`, or `None` when no reviewer process holds it.
    pub(super) fn render_agent_review_hold(
        &mut self,
        agent_id: Uuid,
        draft: HeldDraft,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let view = cx.entity().downgrade();
        let on_cancel: Rc<dyn Fn(&mut Window, &mut App)> = Rc::new(move |_, cx| {
            let _ = view.update(cx, |this, cx| this.cancel_agent_review(agent_id, cx));
        });
        review_hold(
            &mut self.agent_review_ui,
            &self.agent_chats,
            agent_id,
            draft,
            input,
            on_cancel,
            window,
            cx,
        )
    }

    /// Start errors stay beside the composer, which keeps the draft.
    pub(super) fn render_agent_review_error(
        &self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let error = self.agent_review_ui.errors.get(&agent_id)?.clone();
        Some(
            h_flex()
                .w_full()
                .max_w(crate::ui::design::agent_chat_content_max_w())
                .mx_auto()
                .mb_2()
                .px_3()
                .py_2()
                .gap_2()
                .items_start()
                .rounded(crate::ui::design::r_sm())
                .bg(crate::ui::design::rose_soft(cx))
                .debug_selector(|| "review-start-error".into())
                .child(
                    div()
                        .mt(px(2.))
                        .child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::AlertCircle,
                            crate::ui::design::rose(cx),
                            crate::ui::design::icon_sm(),
                        )),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .text_size(crate::ui::design::text_ui())
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child("Review didn't start"),
                        )
                        .child(
                            div()
                                .text_color(crate::ui::design::t2(cx))
                                .child(error),
                        ),
                )
                .child(
                    style::header_icon_button(
                        ("agent-review-error-dismiss", agent_id.as_u128() as u64),
                        IconName::Close,
                        cx,
                    )
                    .tooltip("Dismiss")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agent_review_ui.errors.remove(&agent_id);
                        cx.notify();
                    })),
                )
                .into_any_element(),
        )
    }
}

/// Why Review can't start yet, in the user's terms; `None` when it can.
/// The backend's guards stay authoritative; this only avoids a dead click.
pub(super) fn review_unavailable_reason(
    chats: &AgentChatState,
    session: &AgentChatSession,
    agent_id: Uuid,
    has_open_decision: bool,
    has_active_delegation: bool,
) -> Option<&'static str> {
    if matches!(
        session.status,
        AgentChatStatus::Running | AgentChatStatus::Cancelling
    ) {
        return Some("Review is available when the agent finishes");
    }
    if !session.queued_turns.is_empty() {
        return Some("Send or clear queued messages before Review");
    }
    if has_open_decision {
        return Some("Answer the open question or approval before Review");
    }
    if has_active_delegation {
        return Some("Wait for Bandmate tasks to finish before Review");
    }
    if chats.review_restoring(agent_id) {
        return Some("Restoring saved reviews. Try again in a moment");
    }
    if !chats.safe_for_delegation(agent_id) {
        return Some("Review is available when this conversation is idle");
    }
    None
}

#[cfg(all(test, feature = "ui-layout-tests"))]
#[path = "agent_chat_review_panel_tests.rs"]
mod tests;

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn held_draft_names_what_returns_without_inventing_content() {
        assert_eq!(HeldDraft::default().message(), None);
        assert_eq!(
            HeldDraft { text: true, attachments: 0 }.message().unwrap(),
            "Your draft returns when the review ends."
        );
        assert_eq!(
            HeldDraft { text: false, attachments: 1 }.message().unwrap(),
            "Your attachment returns when the review ends."
        );
        assert_eq!(
            HeldDraft { text: false, attachments: 3 }.message().unwrap(),
            "Your 3 attachments return when the review ends."
        );
        assert_eq!(
            HeldDraft { text: true, attachments: 2 }.message().unwrap(),
            "Your draft and 2 attachments return when the review ends."
        );
    }

    #[test]
    fn fix_is_dispatched_only_after_a_positive_freshness_check() {
        let check = |value: Option<anyhow::Result<bool>>| {
            let (tx, rx) = async_channel::bounded(1);
            if let Some(value) = value {
                tx.try_send(value).unwrap();
            }
            drop(tx);
            smol::block_on(fix_permission(rx))
        };
        assert_eq!(check(Some(Ok(true))), FixPermission::Allowed);
        assert!(matches!(check(Some(Ok(false))), FixPermission::Blocked(m) if m.contains("Review the latest changes")));
        assert!(matches!(check(Some(Err(anyhow::anyhow!("store offline")))), FixPermission::Blocked(m) if m.contains("store offline")));
        assert!(matches!(check(None), FixPermission::Blocked(_)));
    }
}
