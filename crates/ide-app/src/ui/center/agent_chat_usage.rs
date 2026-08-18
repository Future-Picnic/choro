use super::*;
use crate::state::agent_chat::UsageTotals;

impl CenterArea {
    pub(super) fn render_agent_footer_usage(
        &self,
        agent_id: Uuid,
        provider: AgentKind,
        usage: &ConversationUsage,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let expanded = self.agent_chat_usage_expanded.contains(&agent_id);
        let shows_cost = provider == AgentKind::OpenCode;
        let label = if shows_cost {
            format!("Usage ({})", format_usage_cost(usage.totals.cost_usd))
        } else {
            format_token_usage_label(&usage.totals)
        };
        let usage_for_panel = usage.clone();
        let chat_view = cx.entity().clone();

        div()
            .relative()
            .flex_none()
            .child(
                gpui_component::popover::Popover::new((
                    "agent-chat-usage-popover",
                    agent_id.as_u128() as u64,
                ))
                .adaptive_anchor(true)
                .anchor(gpui::Corner::BottomLeft)
                .appearance(false)
                .open(expanded)
                .on_open_change(move |open, _, cx| {
                    chat_view.update(cx, |this, cx| {
                        if *open {
                            this.agent_chat_usage_expanded.insert(agent_id);
                        } else {
                            this.agent_chat_usage_expanded.remove(&agent_id);
                        }
                        cx.notify();
                    });
                })
                .trigger(
                    crate::ui::style::agent_detail_tab_button(
                        ("agent-chat-usage", agent_id.as_u128() as u64),
                        if shows_cost {
                            lucide_icons::Icon::CircleDollarSign
                        } else {
                            lucide_icons::Icon::Gauge
                        },
                        label,
                        false,
                        cx,
                    )
                    .tooltip("Open conversation usage details"),
                )
                .content(move |_, _, cx| render_usage_panel(provider, &usage_for_panel, cx)),
            )
            .into_any_element()
    }
}

fn render_usage_panel(
    provider: AgentKind,
    usage: &ConversationUsage,
    cx: &mut App,
) -> gpui::AnyElement {
    let totals = &usage.totals;
    let latest = usage.latest_turn.as_ref();
    let shows_cost = provider == AgentKind::OpenCode;
    let provider_name = match provider {
        AgentKind::Claude => "Claude Code",
        AgentKind::Codex => "Codex",
        AgentKind::OpenCode => "OpenCode",
    };
    let summary = match provider {
        AgentKind::Codex if totals.cache_read_tokens > 0 => {
            "Reported by Codex. Regular excludes cached input; Input still includes it."
        }
        AgentKind::Claude if totals.cache_read_tokens > 0 => {
            "Reported by Claude Code. Regular excludes cached input."
        }
        AgentKind::OpenCode => "Includes the full OpenCode session, including resumed work.",
        _ => "Token counts reported by the provider for this conversation.",
    };

    v_flex()
        .w(px(340.))
        .max_w(gpui::relative(0.92))
        .p_3()
        .gap_3()
        .rounded(crate::ui::design::r_lg())
        .border_1()
        .border_color(crate::ui::design::line_2(cx))
        .bg(crate::ui::design::focus(cx))
        .shadow(crate::ui::design::shadow())
        .child(
            v_flex()
                .gap_0p5()
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t1(cx))
                        .child("Conversation usage"),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(summary),
                ),
        )
        .child(
            h_flex()
                .w_full()
                .gap_2()
                .child(usage_metric(
                    if !shows_cost && totals.cache_read_tokens > 0 {
                        "Regular tokens"
                    } else {
                        "Total tokens"
                    },
                    format_usage_count(if !shows_cost && totals.cache_read_tokens > 0 {
                        regular_token_count(totals)
                    } else {
                        totals.total_tokens()
                    }),
                    cx,
                ))
                .when(shows_cost, |metrics| {
                    metrics.child(usage_metric(
                        "Estimated cost",
                        format_usage_cost(totals.cost_usd),
                        cx,
                    ))
                })
                .when(!shows_cost && totals.cache_read_tokens > 0, |metrics| {
                    metrics.child(usage_metric(
                        "Cached input",
                        format_usage_count(totals.cache_read_tokens),
                        cx,
                    ))
                }),
        )
        .child(
            v_flex()
                .gap_1p5()
                .child(usage_row("Input", totals.input_tokens, cx))
                .child(usage_row("Output", totals.output_tokens, cx))
                .when(totals.reasoning_tokens > 0, |rows| {
                    rows.child(usage_row("Reasoning", totals.reasoning_tokens, cx))
                })
                .when(totals.cache_read_tokens > 0, |rows| {
                    rows.child(usage_row("Cache read", totals.cache_read_tokens, cx))
                })
                .when(totals.cache_write_tokens > 0, |rows| {
                    rows.child(usage_row("Cache write", totals.cache_write_tokens, cx))
                }),
        )
        .when_some(latest, |panel, latest| {
            panel.child(
                h_flex()
                    .w_full()
                    .pt_2()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .justify_between()
                    .text_size(crate::ui::design::text_label())
                    .child(
                        div()
                            .text_color(crate::ui::design::t3(cx))
                            .child("Latest turn"),
                    )
                    .child(
                        div()
                            .text_color(crate::ui::design::t2(cx))
                            .child(if shows_cost {
                                format!(
                                    "{} · {} tokens",
                                    format_usage_cost(latest.cost_usd),
                                    format_usage_count(latest.total_tokens())
                                )
                            } else {
                                format_token_usage_detail(latest)
                            }),
                    ),
            )
        })
        .when(!usage.models.is_empty(), |panel| {
            panel.child(
                v_flex()
                    .pt_2()
                    .gap_1p5()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(if usage.models.len() == 1 {
                                "Model"
                            } else {
                                "Models"
                            }),
                    )
                    .children(usage.models.iter().map(|model| {
                        h_flex()
                            .w_full()
                            .justify_between()
                            .gap_3()
                            .text_size(crate::ui::design::text_label())
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_color(crate::ui::design::t2(cx))
                                    .child(model.model_id.clone()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(format_usage_count(model.totals.total_tokens())),
                            )
                    })),
            )
        })
        .child(
            div()
                .pt_1()
                .text_size(crate::ui::design::text_label())
                .text_color(crate::ui::design::t3(cx))
                .child(if shows_cost {
                    "Reported by OpenCode. Estimated cost can differ from your provider invoice."
                        .to_string()
                } else {
                    format!("Reported by {provider_name}. No cost estimate is shown.")
                }),
        )
        .into_any_element()
}

fn usage_metric(label: &'static str, value: String, cx: &App) -> gpui::AnyElement {
    v_flex()
        .flex_1()
        .min_w(px(0.))
        .p_2p5()
        .gap_0p5()
        .rounded(crate::ui::design::r_md())
        .bg(crate::ui::design::surface_2(cx))
        .child(
            div()
                .text_size(crate::ui::design::text_label())
                .text_color(crate::ui::design::t3(cx))
                .child(label),
        )
        .child(
            div()
                .text_size(crate::ui::design::text_ui())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(crate::ui::design::t1(cx))
                .child(value),
        )
        .into_any_element()
}

fn usage_row(label: &'static str, value: u64, cx: &App) -> gpui::AnyElement {
    h_flex()
        .w_full()
        .justify_between()
        .text_size(crate::ui::design::text_label())
        .child(div().text_color(crate::ui::design::t3(cx)).child(label))
        .child(
            div()
                .text_color(crate::ui::design::t2(cx))
                .child(format!("{} tokens", format_usage_count(value))),
        )
        .into_any_element()
}

fn format_usage_cost(cost: f64) -> String {
    if !cost.is_finite() || cost <= 0.0 {
        "$0.00".to_string()
    } else if cost < 0.01 {
        "<$0.01".to_string()
    } else {
        format!("${cost:.2}")
    }
}

fn format_usage_count(value: u64) -> String {
    let digits = value.to_string();
    let mut formatted = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(',');
        }
        formatted.push(ch);
    }
    formatted
}

fn format_token_usage_label(totals: &UsageTotals) -> String {
    if totals.cache_read_tokens > 0 {
        format!(
            "Usage ({} regular · {} cached)",
            format_usage_count_compact(regular_token_count(totals)),
            format_usage_count_compact(totals.cache_read_tokens)
        )
    } else {
        format!(
            "Usage ({} tokens)",
            format_usage_count(totals.total_tokens())
        )
    }
}

fn format_usage_count_compact(value: u64) -> String {
    let (scaled, suffix) = if value >= 1_000_000_000 {
        (value as f64 / 1_000_000_000.0, "B")
    } else if value >= 1_000_000 {
        (value as f64 / 1_000_000.0, "M")
    } else if value >= 1_000 {
        (value as f64 / 1_000.0, "K")
    } else {
        return value.to_string();
    };
    let formatted = if scaled >= 100.0 {
        format!("{scaled:.0}")
    } else if scaled >= 10.0 {
        format!("{scaled:.1}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        format!("{scaled:.2}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };
    format!("{formatted}{suffix}")
}

fn format_token_usage_detail(totals: &UsageTotals) -> String {
    if totals.cache_read_tokens > 0 {
        format!(
            "{} regular · {} cached",
            format_usage_count(regular_token_count(totals)),
            format_usage_count(totals.cache_read_tokens)
        )
    } else {
        format!("{} tokens", format_usage_count(totals.total_tokens()))
    }
}

fn regular_token_count(totals: &UsageTotals) -> u64 {
    totals
        .total_tokens()
        .saturating_sub(totals.cache_read_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_usage_values_for_the_compact_control() {
        assert_eq!(format_usage_cost(0.0), "$0.00");
        assert_eq!(format_usage_cost(0.004), "<$0.01");
        assert_eq!(format_usage_cost(1.236), "$1.24");
        assert_eq!(format_usage_count(1_234_567), "1,234,567");
    }

    #[test]
    fn separates_regular_and_cached_token_usage() {
        let totals = UsageTotals {
            reported_total_tokens: 11_694_308,
            cache_read_tokens: 11_211_008,
            ..Default::default()
        };

        assert_eq!(
            format_token_usage_label(&totals),
            "Usage (483K regular · 11.2M cached)"
        );
        assert_eq!(
            format_token_usage_detail(&totals),
            "483,300 regular · 11,211,008 cached"
        );
    }

    #[test]
    fn keeps_token_usage_labels_simple_without_cached_input() {
        let totals = UsageTotals {
            reported_total_tokens: 240_846,
            ..Default::default()
        };

        assert_eq!(format_token_usage_label(&totals), "Usage (240,846 tokens)");
        assert_eq!(format_token_usage_detail(&totals), "240,846 tokens");
    }

    #[test]
    fn compacts_large_usage_counts_without_noisy_zeroes() {
        assert_eq!(format_usage_count_compact(999), "999");
        assert_eq!(format_usage_count_compact(1_000), "1K");
        assert_eq!(format_usage_count_compact(12_040), "12K");
        assert_eq!(format_usage_count_compact(100_000), "100K");
        assert_eq!(format_usage_count_compact(1_250_000), "1.25M");
        assert_eq!(format_usage_count_compact(11_694_308), "11.7M");
        assert_eq!(format_usage_count_compact(2_000_000_000), "2B");
    }
}
