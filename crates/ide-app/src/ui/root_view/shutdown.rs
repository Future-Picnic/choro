use super::*;

#[derive(Clone)]
pub(super) enum ShutdownState {
    Idle,
    Saving,
    StoppingProcesses,
    Finishing,
    Failed(SharedString),
    Ready,
}

impl RootView {
    pub(crate) fn handle_close_request(&mut self, cx: &mut Context<Self>) -> bool {
        match self.shutdown_state {
            ShutdownState::Ready => true,
            ShutdownState::Idle => {
                self.begin_shutdown(cx);
                false
            }
            _ => false,
        }
    }

    pub(super) fn begin_shutdown(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.shutdown_state, ShutdownState::Idle) {
            return;
        }
        self.shutdown_state = ShutdownState::Saving;
        cx.notify();

        cx.spawn(async move |this, cx| {
            // Give the dimmed overlay and branded spinner one frame to appear
            // before any synchronous local flush work begins.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(180))
                .await;

            let save_result = this
                .update(cx, |this, cx| {
                    this.workspace
                        .update(cx, |workspace, _| workspace.save_now());
                    this.agents.update(cx, |agents, _| agents.save_now());
                    this.center
                        .update(cx, |center, cx| center.save_for_shutdown(cx))
                })
                .unwrap_or_else(|error| Err(anyhow::anyhow!(error.to_string())));

            if let Err(error) = save_result {
                let message: SharedString = format!("Could not save safely: {error}").into();
                this.update(cx, |this, cx| {
                    this.shutdown_state = ShutdownState::Failed(message);
                    cx.notify();
                })
                .ok();
                return;
            }

            this.update(cx, |this, cx| {
                this.shutdown_state = ShutdownState::StoppingProcesses;
                cx.notify();
            })
            .ok();
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;

            this.update(cx, |this, cx| {
                this.agent_chats
                    .update(cx, |chats, cx| chats.shutdown_all(cx));
                this.terminals
                    .update(cx, |terminals, cx| terminals.shutdown_all(cx));
                this.shutdown_state = ShutdownState::Finishing;
                cx.notify();
            })
            .ok();
            let finish_delay = if std::env::var_os("CHORO_DEBUG_SHUTDOWN").is_some()
                || std::env::var_os("MYIDE_DEBUG_SHUTDOWN").is_some()
            {
                std::time::Duration::from_secs(5)
            } else {
                std::time::Duration::from_millis(160)
            };
            cx.background_executor().timer(finish_delay).await;

            this.update(cx, |this, cx| {
                this.shutdown_state = ShutdownState::Ready;
                crate::notifications::set_dock_badge(None);
                cx.notify();
                cx.quit();
            })
            .ok();
        })
        .detach();
    }

    fn render_shutdown_step(
        &self,
        label: &'static str,
        complete: bool,
        active: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let color = if complete {
            crate::ui::design::sage(cx)
        } else if active {
            crate::ui::design::accent(cx)
        } else {
            crate::ui::design::t3(cx).opacity(0.62)
        };
        h_flex()
            .w_full()
            .gap_2()
            .items_center()
            .child(if complete {
                Icon::new(IconName::Check)
                    .size(crate::ui::design::icon_md())
                    .text_color(color)
                    .into_any_element()
            } else if active {
                logo_spinner(14., "shutdown-step", label.len(), color)
            } else {
                Icon::new(IconName::Dash)
                    .size(crate::ui::design::icon_md())
                    .text_color(color)
                    .into_any_element()
            })
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .font_weight(if active {
                        gpui::FontWeight::MEDIUM
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(color)
                    .child(label),
            )
            .into_any_element()
    }

    pub(super) fn render_shutdown_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if matches!(
            self.shutdown_state,
            ShutdownState::Idle | ShutdownState::Ready
        ) {
            return None;
        }

        let failed = match &self.shutdown_state {
            ShutdownState::Failed(message) => Some(message.clone()),
            _ => None,
        };
        let save_complete = matches!(
            self.shutdown_state,
            ShutdownState::StoppingProcesses | ShutdownState::Finishing
        );
        let processes_complete = matches!(self.shutdown_state, ShutdownState::Finishing);

        let mut card = v_flex()
            .w(px(360.))
            .gap_4()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.62))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_5()
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(logo_spinner(
                        28.,
                        "shutdown-hero",
                        0,
                        crate::ui::design::accent(cx),
                    ))
                    .child(
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(if failed.is_some() {
                                        "Couldn’t close safely"
                                    } else {
                                        "Saving and closing…"
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Keeping your local work consistent."),
                            ),
                    ),
            );

        if let Some(message) = failed {
            card = card
                .child(
                    div()
                        .rounded(crate::ui::design::r_sm())
                        .bg(crate::ui::design::rose(cx).opacity(0.1))
                        .p_3()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(message),
                )
                .child(
                    h_flex()
                        .w_full()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("shutdown-back")
                                .label("Back to app")
                                .custom(crate::ui::style::dialog_neutral_variant(cx))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shutdown_state = ShutdownState::Idle;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("shutdown-force-quit")
                                .label("Quit anyway")
                                .danger()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.shutdown_state = ShutdownState::Ready;
                                    cx.quit();
                                })),
                        ),
                );
        } else {
            card = card.child(
                v_flex()
                    .gap_2()
                    .child(self.render_shutdown_step(
                        "Save workspace and editors",
                        save_complete,
                        matches!(self.shutdown_state, ShutdownState::Saving),
                        cx,
                    ))
                    .child(self.render_shutdown_step(
                        "Stop agents and terminals",
                        processes_complete,
                        matches!(self.shutdown_state, ShutdownState::StoppingProcesses),
                        cx,
                    ))
                    .child(self.render_shutdown_step(
                        "Close application",
                        false,
                        matches!(self.shutdown_state, ShutdownState::Finishing),
                        cx,
                    )),
            );
        }

        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(crate::ui::design::base(cx).opacity(0.86))
                .child(card)
                .into_any_element(),
        )
    }
}
