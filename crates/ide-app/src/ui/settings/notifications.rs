use super::*;

impl SettingsView {
    pub(super) fn render_notifications_section(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let preferences = self.workspace.read(cx).notifications;
        let permission = crate::notifications::permission();
        let setting_group =
            |title: &'static str, description: &'static str, control: gpui::AnyElement| {
                h_flex()
                    .w_full()
                    .min_h(px(72.))
                    .gap_5()
                    .items_center()
                    .justify_between()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(title),
                            )
                            .child(
                                div()
                                    .max_w(px(460.))
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(description),
                            ),
                    )
                    .child(div().w(px(360.)).flex_none().child(control))
            };

        v_flex()
            .w_full()
            .gap_4()
            .child(
                v_flex()
                    .w_full()
                    .px_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(setting_group(
                        "Questions and approvals",
                        "Notify when another conversation needs an answer, approval, or plan decision. The open conversation is always suppressed.",
                        crate::ui::style::segmented_container_quiet(cx)
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-attention-on",
                                    IconName::Check,
                                    "On",
                                    preferences.questions_and_approvals,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.questions_and_approvals = true;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-attention-off",
                                    IconName::Close,
                                    "Off",
                                    !preferences.questions_and_approvals,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.questions_and_approvals = false;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .into_any_element(),
                    ))
                    .child(
                        div()
                            .w_full()
                            .h(px(1.))
                            .bg(crate::ui::design::line(cx).opacity(0.45)),
                    )
                    .child(setting_group(
                        "Turn completion",
                        "Completion is less urgent and stays silent. Background-only avoids banners while you are already working in Choro.",
                        crate::ui::style::segmented_container_quiet(cx)
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-completion-never",
                                    IconName::Close,
                                    "Never",
                                    preferences.completion == CompletionNotifications::Never,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.completion = CompletionNotifications::Never;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-completion-background",
                                    IconName::CircleCheck,
                                    "Background",
                                    preferences.completion == CompletionNotifications::Background,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.completion = CompletionNotifications::Background;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-completion-always",
                                    IconName::Check,
                                    "Always",
                                    preferences.completion == CompletionNotifications::Always,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.completion = CompletionNotifications::Always;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .into_any_element(),
                    ))
                    .child(
                        div()
                            .w_full()
                            .h(px(1.))
                            .bg(crate::ui::design::line(cx).opacity(0.45)),
                    )
                    .child(setting_group(
                        "Notification sound",
                        "Play Choro companion sounds for new questions, approvals, and completed work. macOS banners use the system sound for questions and approvals.",
                        crate::ui::style::segmented_container_quiet(cx)
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-sound-on",
                                    IconName::Check,
                                    "On",
                                    preferences.sound,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.sound = true;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-notifications-sound-off",
                                    IconName::Close,
                                    "Off",
                                    !preferences.sound,
                                    cx,
                                )
                                .flex_1()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.workspace.update(cx, |workspace, cx| {
                                        let mut settings = workspace.notifications;
                                        settings.sound = false;
                                        workspace.set_notification_settings(settings, cx);
                                        workspace.save_now();
                                    });
                                })),
                            )
                            .into_any_element(),
                    )),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .items_center()
                    .justify_between()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        v_flex()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("macOS permission"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(permission.label()),
                            ),
                    )
                    .when(
                        matches!(
                            permission,
                            crate::notifications::NotificationPermission::Denied
                                | crate::notifications::NotificationPermission::Provisional
                        ),
                        |row| {
                            row.child(
                                crate::ui::style::dialog_neutral_button(
                                    "settings-open-system-notifications",
                                    "Open System Settings",
                                    cx,
                                )
                                .on_click(|_, _, _| {
                                    crate::notifications::open_system_notification_settings();
                                }),
                            )
                        },
                    ),
            )
            .into_any_element()
    }
}
