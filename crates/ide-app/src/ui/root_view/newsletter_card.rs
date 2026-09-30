use super::*;

use crate::ui::design;
use gpui::{Animation, AnimationExt};
use std::time::Duration;

/// A product-update note, not a second appearance of the onboarding artwork.
/// The front sheet arrives once; after signup the same space resolves into a
/// subscribed note. All content stays visible when Reduce Motion is enabled.
fn newsletter_artwork(complete: bool, cx: &App) -> impl IntoElement {
    #[cfg(target_os = "macos")]
    let reduce_motion =
        objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion();
    #[cfg(not(target_os = "macos"))]
    let reduce_motion = false;

    let accent = design::accent(cx);
    let sage = design::sage(cx);
    let ink = design::t1(cx);
    let muted = design::t3(cx);
    let front =
        v_flex()
            .relative()
            .w(px(310.))
            .h(px(128.))
            .gap_3()
            .p_4()
            .rounded(design::r_md())
            .border_1()
            .border_color(design::line_2(cx))
            .bg(design::surface(cx))
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(design::indicator::lucide_icon(
                        if complete {
                            lucide_icons::Icon::Check
                        } else {
                            lucide_icons::Icon::Mail
                        },
                        if complete { sage } else { accent },
                        design::icon_md(),
                    ))
                    .child(
                        div()
                            .text_size(design::text_head())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(ink)
                            .child("Choro notes"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(design::text_label())
                            .text_color(muted)
                            .child(if complete { "SUBSCRIBED" } else { "YOUR INBOX" }),
                    ),
            )
            .child(div().h(px(1.)).w_full().bg(design::line_2(cx)))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(design::text_body())
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(ink)
                            .child(if complete {
                                "You’ll hear from us when there’s news."
                            } else {
                                "What’s new in Choro"
                            }),
                    )
                    .child(div().text_size(design::text_ui()).text_color(muted).child(
                        if complete {
                            "No account needed. Unsubscribe anytime."
                        } else {
                            "A short note when something useful ships."
                        },
                    )),
            );

    let front: AnyElement = if reduce_motion {
        front.into_any_element()
    } else {
        front
            .with_animation(
                ("newsletter-note-arrival", u32::from(complete)),
                Animation::new(Duration::from_millis(if complete { 320 } else { 440 }))
                    .with_easing(gpui::ease_out_quint()),
                move |note, progress| {
                    note.top(px((1.0 - progress) * 14.0))
                        .opacity(0.55 + progress * 0.45)
                },
            )
            .into_any_element()
    };

    div().w_full().flex().justify_center().child(
        div()
            .relative()
            .w(px(342.))
            .max_w(gpui::relative(1.))
            .h(px(152.))
            .child(
                div()
                    .absolute()
                    .top(px(5.))
                    .right(px(0.))
                    .w(px(286.))
                    .h(px(124.))
                    .rounded(design::r_md())
                    .border_1()
                    .border_color(design::accent_line(cx))
                    .bg(design::accent_soft(cx)),
            )
            .child(div().absolute().left(px(0.)).top(px(18.)).child(front)),
    )
}

impl RootView {
    pub(super) fn open_newsletter_card(&mut self, cx: &mut Context<Self>) {
        if self.newsletter_subscribed || self.onboarding.is_some() {
            return;
        }
        self.newsletter_visible = true;
        self.newsletter_status = NewsletterStatus::Idle;
        self.newsletter_error = None;
        cx.notify();
    }

    fn dismiss_newsletter_card(&mut self, cx: &mut Context<Self>) {
        self.newsletter_visible = false;
        self.newsletter_error = None;
        crate::newsletter::dismiss();
        cx.notify();
    }

    fn submit_newsletter(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.newsletter_status,
            NewsletterStatus::Sending | NewsletterStatus::Subscribed
        ) {
            return;
        }
        let email = self.newsletter_email.read(cx).value().trim().to_string();
        if email.is_empty() {
            self.newsletter_status = NewsletterStatus::Error;
            self.newsletter_error = Some("Enter your email address.".into());
            cx.notify();
            return;
        }
        self.newsletter_status = NewsletterStatus::Sending;
        self.newsletter_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { crate::newsletter::subscribe(&email) })
                .await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        crate::newsletter::mark_subscribed();
                        this.newsletter_subscribed = true;
                        this.newsletter_status = NewsletterStatus::Subscribed;
                    }
                    Err(error) => {
                        this.newsletter_status = NewsletterStatus::Error;
                        this.newsletter_error = Some(error);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn render_newsletter_card(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.newsletter_visible
            || self.onboarding.is_some()
            || !matches!(self.shutdown_state, ShutdownState::Idle)
        {
            return None;
        }
        let complete = self.newsletter_status == NewsletterStatus::Subscribed;
        let mut body = v_flex()
            .gap_5()
            .p_8()
            .child(newsletter_artwork(complete, cx))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        div()
                            .text_size(design::text_display())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .line_height(gpui::relative(1.18))
                            .text_color(design::t1(cx))
                            .child(if complete {
                                "You’re on the list."
                            } else {
                                "Stay close to what’s next."
                            }),
                    )
                    .child(
                        div()
                            .text_size(design::text_body())
                            .line_height(gpui::relative(1.55))
                            .text_color(design::t2(cx))
                            .child(if complete {
                                "Thanks for joining us. We’ll send occasional Choro updates, and you can unsubscribe from any email."
                            } else {
                                "Get occasional Choro feature updates and release notes by email. No account or extra setup—just what’s new when it’s ready."
                            }),
                    ),
            );

        if !complete {
            body = body
                .child(
                    v_flex()
                        .gap_3()
                        .child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(design::text_label())
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(design::t2(cx))
                                        .child("YOUR EMAIL"),
                                )
                                .child(Input::new(&self.newsletter_email).w_full()),
                        )
                        .when_some(self.newsletter_error.as_ref(), |form, error| {
                            form.child(
                                div()
                                    .text_size(design::text_ui())
                                    .text_color(design::rose(cx))
                                    .child(error.clone()),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(design::text_ui())
                        .line_height(gpui::relative(1.45))
                        .text_color(design::t3(cx))
                        .child("Send me updates opts you into these emails. Kit receives your address, not your projects or chats. Unsubscribe anytime."),
                );
        }

        let footer = h_flex()
            .w_full()
            .items_center()
            .gap_2()
            .bg(design::base(cx))
            .px_8()
            .py_4()
            .child(
                style::ghost_button_compact("newsletter-privacy", "Privacy policy")
                    .text_color(design::t3(cx))
                    .on_click(|_, _, _| {
                        crate::ui::git::git_panel::open_url("https://choro.dev/privacy.html");
                    }),
            )
            .child(div().flex_1())
            .child(
                style::dialog_neutral_button(
                    "newsletter-not-now",
                    if complete { "Done" } else { "Not now" },
                    cx,
                )
                .h(px(36.))
                .px_5()
                .on_click(cx.listener(|this, _, _, cx| this.dismiss_newsletter_card(cx))),
            );
        let footer = if complete {
            footer
        } else {
            footer.child(
                style::primary_button_compact(
                    "newsletter-submit",
                    if self.newsletter_status == NewsletterStatus::Sending {
                        "Sending…"
                    } else {
                        "Send me updates"
                    },
                    cx,
                )
                .h(px(36.))
                .px_5()
                .disabled(self.newsletter_status == NewsletterStatus::Sending)
                .on_click(cx.listener(|this, _, _, cx| this.submit_newsletter(cx))),
            )
        };

        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(design::base(cx).opacity(0.92))
                .child(
                    v_flex()
                        .id("newsletter-welcome")
                        .w(px(640.))
                        .max_w(gpui::relative(0.94))
                        .max_h(gpui::relative(0.94))
                        .occlude()
                        .overflow_y_scroll()
                        .rounded(design::r_lg())
                        .border_1()
                        .border_color(design::line_2(cx))
                        .bg(design::focus(cx))
                        .shadow(design::shadow())
                        .child(body)
                        .child(footer),
                )
                .into_any_element(),
        )
    }
}
