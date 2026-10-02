use super::*;
use crate::state::subscription_usage::{
    AllowanceWindow, ProviderAllowance, SubscriptionUsageState,
};
use crate::ui::design;
use ide_core::AgentKind;

impl RootView {
    pub(super) fn subscription_usage_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        usage_control(self.subscription_usage.clone(), cx)
    }
}

fn usage_control(state: Entity<SubscriptionUsageState>, cx: &App) -> impl IntoElement {
    let reading = state.read(cx);
    let warning = reading
        .providers
        .iter()
        .any(|row| row.warning(chrono::Utc::now().timestamp()));
    let open = reading.open;
    let toggle_state = state.clone();
    gpui_component::popover::Popover::new("subscription-usage-popover")
        .adaptive_anchor(true)
        .anchor(gpui::Corner::BottomLeft)
        .appearance(false)
        .open(open)
        .on_open_change(move |open, _, cx| {
            toggle_state.update(cx, |state, cx| state.set_open(*open, cx));
        })
        .trigger(
            style::sidebar_footer_glyph_button(
                "sidebar-subscription-usage",
                lucide_icons::Icon::Gauge,
                cx,
            )
            .debug_selector(|| "subscription-usage-trigger".into())
            .relative()
            .tooltip(if warning {
                "Subscription usage — an allowance is running low"
            } else {
                "Subscription usage"
            })
            .when(warning, |button| {
                button.child(
                    div()
                        .absolute()
                        .top_1()
                        .right_1()
                        .size(px(5.))
                        .rounded_full()
                        .bg(design::amber(cx)),
                )
            }),
        )
        .content(move |_, window, cx| render_subscription_usage(&state, window, cx))
}

fn render_subscription_usage(
    state: &Entity<SubscriptionUsageState>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let reading = state.read(cx);
    let refreshing = reading.providers.iter().any(|row| row.refreshing);
    let refresh_state = state.clone();
    let close_state = state.clone();
    let now = chrono::Utc::now().timestamp();
    v_flex()
        .debug_selector(|| "subscription-usage-panel".into())
        .w(px(320.))
        .max_w(gpui::relative(0.92))
        .max_h(window.bounds().size.height * 0.8)
        .rounded(design::r_lg())
        .border_1()
        .border_color(design::line_2(cx))
        .bg(design::focus(cx))
        .shadow(design::shadow())
        .child(
            h_flex()
                .w_full()
                .flex_none()
                .px_3()
                .py_2()
                .gap_1()
                .child(
                    div()
                        .flex_1()
                        .text_size(design::text_head())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(design::t1(cx))
                        .child("Subscription usage"),
                )
                .child(
                    style::refresh_icon_button("subscription-usage-refresh", cx)
                        .disabled(refreshing)
                        .tooltip(if refreshing {
                            "Checking account usage…"
                        } else {
                            "Refresh account usage"
                        })
                        .on_click(move |_, _, cx| {
                            refresh_state.update(cx, |state, cx| state.refresh(true, cx));
                        }),
                )
                .child(
                    style::header_icon_button("subscription-usage-close", IconName::Close, cx)
                        .tooltip("Close subscription usage")
                        .on_click(move |_, _, cx| {
                            close_state.update(cx, |state, cx| state.set_open(false, cx));
                        }),
                ),
        )
        .child(
            v_flex()
                .id("subscription-usage-providers")
                .min_h(px(0.))
                .overflow_y_scroll()
                .px_3()
                .when(reading.providers.is_empty(), |body| {
                    body.child(
                        v_flex()
                            .py_4()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(design::text_body())
                                    .text_color(design::t2(cx))
                                    .child("Connect an agent to see its subscription usage."),
                            )
                            .child(
                                style::ghost_button_compact(
                                    "subscription-connect-agent",
                                    "Agent settings",
                                )
                                .on_click(|_, window, cx| {
                                    window.dispatch_action(Box::new(OpenSettings), cx)
                                }),
                            ),
                    )
                })
                .children(
                    reading
                        .providers
                        .iter()
                        .map(|row| provider_section(row, now, cx)),
                ),
        )
        .child(
            div()
                .flex_none()
                .px_3()
                .py_2()
                .border_t_1()
                .border_color(design::line(cx))
                .text_size(design::text_label())
                .text_color(design::t3(cx))
                .child("Account allowances include usage outside Choro."),
        )
        .into_any_element()
}

#[cfg(all(test, feature = "ui-layout-tests"))]
mod layout_tests {
    use super::*;

    struct Fixture {
        usage: Entity<SubscriptionUsageState>,
    }

    impl Render for Fixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(
                div()
                    .absolute()
                    .left_2()
                    .bottom_2()
                    .child(usage_control(self.usage.clone(), cx)),
            )
        }
    }

    #[gpui::test]
    fn subscription_popover_opens_above_footer_fits_small_windows_and_dismisses(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(gpui_component::init);
        let mut usage = None;
        let (_, cx) = cx.add_window_view(|window, cx| {
            let now = chrono::Utc::now().timestamp();
            let state = cx.new(|_| {
                SubscriptionUsageState::in_memory(vec![(
                    AgentKind::Claude,
                    crate::state::subscription_usage::AllowanceSnapshot {
                        status: "ready".into(),
                        plan: Some("max".into()),
                        windows: (0..12)
                            .map(|index| AllowanceWindow {
                                label: format!("Weekly model {index}"),
                                used_percent: 84.,
                                resets_at: Some(now + 86400),
                            })
                            .collect(),
                        ..Default::default()
                    },
                )])
            });
            let view = cx.new(|cx| {
                cx.observe(&state, |_: &mut Fixture, _, cx| cx.notify())
                    .detach();
                Fixture {
                    usage: state.clone(),
                }
            });
            usage = Some(state);
            gpui_component::Root::new(view, window, cx)
        });
        let usage = usage.unwrap();
        cx.simulate_resize(gpui::size(px(360.), px(350.)));
        cx.run_until_parked();
        let trigger = cx.debug_bounds("subscription-usage-trigger").unwrap();
        cx.simulate_click(trigger.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(usage.read_with(cx, |state, _| state.open));
        let panel = cx.debug_bounds("subscription-usage-panel").unwrap();
        assert!(
            panel.bottom() <= trigger.top(),
            "footer control must open upward"
        );
        assert!(panel.top() >= px(0.) && panel.left() >= px(0.));
        assert!(panel.right() <= px(360.) && panel.size.height <= px(280.));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!usage.read_with(cx, |state, _| state.open));
        cx.simulate_click(trigger.center(), Modifiers::default());
        cx.run_until_parked();
        cx.simulate_click(gpui::point(px(350.), px(340.)), Modifiers::default());
        cx.run_until_parked();
        assert!(!usage.read_with(cx, |state, _| state.open));
    }
}

fn provider_section(row: &ProviderAllowance, now: i64, cx: &App) -> AnyElement {
    let name = match row.provider {
        AgentKind::Claude => "Claude",
        AgentKind::Codex => "Codex",
        AgentKind::Gemini => "Gemini",
        AgentKind::OpenCode => "OpenCode",
    };
    let account_url = match row.provider {
        AgentKind::Claude => Some("https://claude.ai/settings/usage"),
        AgentKind::Codex => Some("https://chatgpt.com/codex/settings/usage"),
        _ => None,
    };
    v_flex()
        .py_3()
        .gap_2p5()
        .border_t_1()
        .border_color(design::line(cx))
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(crate::ui::center::provider_brand_icon(row.provider).size(design::icon()))
                .child(
                    div()
                        .flex_1()
                        .text_size(design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(design::t1(cx))
                        .child(name),
                )
                .when_some(row.snapshot.plan.as_ref(), |header, plan| {
                    header.child(
                        div()
                            .text_size(design::text_label())
                            .text_color(design::t3(cx))
                            .child(plan.replace('_', " ")),
                    )
                }),
        )
        .when(row.snapshot.windows.is_empty(), |section| {
            section.child(
                div()
                    .text_size(design::text_ui())
                    .text_color(design::t3(cx))
                    .child(row.message()),
            )
        })
        .children(
            row.snapshot
                .windows
                .iter()
                .map(|window| allowance_row(window, now, cx)),
        )
        .when_some(
            row.snapshot.reset_credits.filter(|count| *count > 0),
            |section, count| {
                section.child(
                    div()
                        .text_size(design::text_label())
                        .text_color(design::t2(cx))
                        .child(format!(
                            "{count} full {} available",
                            if count == 1 { "reset" } else { "resets" }
                        )),
                )
            },
        )
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .flex_1()
                        .text_size(design::text_label())
                        .text_color(if row.refresh_failed {
                            design::amber(cx)
                        } else {
                            design::t3(cx)
                        })
                        .child(if row.refreshing {
                            "Checking…".into()
                        } else if row.refresh_failed {
                            format!(
                                "Refresh failed · last reading {}",
                                age_label(row.updated_at, now)
                            )
                        } else if row.updated_at.is_some() {
                            format!("Updated {}", age_label(row.updated_at, now))
                        } else {
                            String::new()
                        }),
                )
                .when_some(account_url, |footer, url| {
                    footer.child(
                        style::header_icon_button(
                            gpui::SharedString::from(format!("subscription-account-{name}")),
                            IconName::ExternalLink,
                            cx,
                        )
                        .tooltip(format!("Open {name} account usage"))
                        .on_click(move |_, _, cx| cx.open_url(url)),
                    )
                }),
        )
        .into_any_element()
}

fn allowance_row(window: &AllowanceWindow, now: i64, cx: &App) -> AnyElement {
    let expired = window.expired(now);
    let remaining = window.remaining_percent();
    let color = if remaining <= 5. {
        design::rose(cx)
    } else if remaining <= 20. {
        design::amber(cx)
    } else {
        design::sage(cx)
    };
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .justify_between()
                .text_size(design::text_ui())
                .child(
                    div()
                        .min_w(px(0.))
                        .text_color(design::t2(cx))
                        .child(window.label.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(if expired { design::t3(cx) } else { color })
                        .child(if expired {
                            "Awaiting refresh".into()
                        } else {
                            format!("{remaining:.0}% remaining")
                        }),
                ),
        )
        .when(!expired, |row| {
            row.child(
                div()
                    .w_full()
                    .h(px(4.))
                    .rounded_full()
                    .bg(design::surface_2(cx))
                    .child(
                        div()
                            .h_full()
                            .w(gpui::relative((remaining / 100.) as f32))
                            .rounded_full()
                            .bg(color),
                    ),
            )
        })
        .child(
            div()
                .text_size(design::text_label())
                .text_color(design::t3(cx))
                .child(reset_label(window.resets_at, now)),
        )
        .into_any_element()
}

fn age_label(updated_at: Option<i64>, now: i64) -> String {
    let age = now.saturating_sub(updated_at.unwrap_or(now)).max(0);
    if age < 60 {
        "just now".into()
    } else if age < 3600 {
        format!("{}m ago", age / 60)
    } else {
        format!("{}h ago", age / 3600)
    }
}

fn reset_label(reset: Option<i64>, now: i64) -> String {
    let Some(reset) = reset else {
        return "Reset time unavailable".into();
    };
    if reset <= now {
        return "Reset time passed · checking next allowance".into();
    }
    let minutes = (reset - now + 59) / 60;
    if minutes >= 1440 {
        format!("Resets in {}d {}h", minutes / 1440, (minutes % 1440) / 60)
    } else if minutes >= 60 {
        format!("Resets in {}h {}m", minutes / 60, minutes % 60)
    } else {
        format!("Resets in {minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_labels_do_not_claim_a_new_allowance_after_the_reset_time_passes() {
        assert_eq!(reset_label(Some(1061), 1000), "Resets in 2m");
        assert_eq!(reset_label(Some(4600), 1000), "Resets in 1h 0m");
        assert_eq!(
            reset_label(Some(1000), 1000),
            "Reset time passed · checking next allowance"
        );
        assert_eq!(reset_label(None, 1000), "Reset time unavailable");
    }
}
