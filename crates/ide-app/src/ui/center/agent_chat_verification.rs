use super::agent_chat_runtime::{
    is_follow_up_verification, verification_lifecycle, VerificationLifecycle,
};
use super::*;

impl CenterArea {
    pub(super) fn render_verification_decision_panel(
        &self,
        agent_id: Uuid,
        is_reverification: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let key = agent_id.as_u128() as u64;
        let verify_label = if is_reverification {
            "Re-verify"
        } else {
            "Verify now"
        };
        let title = if is_reverification {
            "Re-verify the fixes?"
        } else {
            "Verify this work?"
        };
        let message = if is_reverification {
            "This runs another agent pass on the clear requirements that were just fixed. It can take additional time and tokens."
        } else {
            "Verification checks the finished work against its written requirements. It can take additional time and tokens."
        };

        crate::ui::style::chat_card(cx)
            .child(
                crate::ui::style::chat_card_head(cx)
                    .child(crate::ui::design::indicator::lucide_icon(
                        lucide_icons::Icon::BadgeCheck,
                        crate::ui::design::accent(cx),
                        crate::ui::design::icon_sm(),
                    ))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t2(cx))
                            .child(title),
                    )
                    .child(div().flex_1()),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .py(crate::ui::design::chat_card_body_pad_y())
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(message),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .flex_wrap()
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    ("agent-chat-verification-never", key),
                                    "Never verify",
                                    cx,
                                )
                                .tooltip(
                                    "Turn verification off; you can change this later in Settings",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.verification_prompt_pending.clear();
                                        this.workspace.update(cx, |workspace, cx| {
                                            workspace.set_verification_mode(
                                                ide_core::config::VerificationMode::Off,
                                                cx,
                                            );
                                        });
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(div().flex_1())
                            .child(
                                crate::ui::style::dialog_neutral_button(
                                    ("agent-chat-verification-not-now", key),
                                    "Not now",
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.verification_prompt_pending.remove(&agent_id);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                crate::ui::style::secondary_button_compact(
                                    ("agent-chat-verification-always", key),
                                    "Always verify",
                                )
                                .tooltip("Verify this work and future eligible work automatically")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.verification_prompt_pending.remove(&agent_id);
                                        this.workspace.update(cx, |workspace, cx| {
                                            workspace.set_verification_mode(
                                                ide_core::config::VerificationMode::Automatic,
                                                cx,
                                            );
                                        });
                                        this.request_agent_verification(agent_id, cx);
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                crate::ui::style::primary_button_compact(
                                    ("agent-chat-verification-confirm", key),
                                    verify_label,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.verification_prompt_pending.remove(&agent_id);
                                        this.request_agent_verification(agent_id, cx);
                                        cx.notify();
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_verification_card(
        &self,
        agent_id: Uuid,
        verification: &crate::state::agent_chat::Verification,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let hide_unclear = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .and_then(|session| {
                session
                    .timeline
                    .iter()
                    .enumerate()
                    .rfind(|(_, item)| {
                        matches!(
                            item,
                            AgentChatTimelineItem::Verification(candidate)
                                if candidate.id == verification.id
                        )
                    })
                    .map(|(index, _)| is_follow_up_verification(&session.timeline, index))
            })
            .unwrap_or(false);
        let display_items = verification
            .display_items()
            .into_iter()
            .map(|(_, item)| item)
            .filter(|item| !hide_unclear || item.status != VerificationStatus::Unclear)
            .collect::<Vec<_>>();
        let total = display_items.len();
        let met = display_items
            .iter()
            .filter(|item| item.status == VerificationStatus::Met)
            .count();
        let all_displayed_met = if hide_unclear {
            display_items
                .iter()
                .all(|item| item.status == VerificationStatus::Met)
        } else {
            verification.all_met()
        };

        let summary = if verification.is_unparsed() {
            String::new()
        } else if all_displayed_met {
            if total == 0 {
                "No clear gaps".to_string()
            } else {
                format!("All {total} met")
            }
        } else {
            format!("{met} of {total} met")
        };

        let mut card = crate::ui::style::chat_card(cx).child(
            crate::ui::style::chat_card_head(cx)
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::BadgeCheck,
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_sm(),
                ))
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(crate::ui::design::t2(cx))
                        .child("Verified"),
                )
                .child(div().flex_1())
                .when(!summary.is_empty(), |head| {
                    head.child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(summary),
                    )
                }),
        );

        if verification.is_unparsed() {
            return card
                .child(
                    div()
                        .w_full()
                        .px(crate::ui::design::chat_card_body_pad_x())
                        .py(crate::ui::design::chat_card_body_pad_y())
                        .child(render_plan_markdown(
                            &verification.display_markdown(),
                            window,
                            cx,
                        )),
                )
                .into_any_element();
        }

        if all_displayed_met {
            return card
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .px(crate::ui::design::chat_card_body_pad_x())
                        .py(crate::ui::design::chat_card_body_pad_y())
                        .child(
                            gpui_component::Icon::new(IconName::CircleCheck)
                                .size(crate::ui::design::icon())
                                .text_color(crate::ui::design::sage(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t2(cx))
                                .child(if total == 0 {
                                    "No clear unmet requirements remain.".to_string()
                                } else if total == 1 {
                                    "The 1 stated requirement is met.".to_string()
                                } else {
                                    format!("All {total} requirements met.")
                                }),
                        ),
                )
                .into_any_element();
        }

        let collapsible = total > 5;
        let show_all = verification.expanded || !collapsible;
        let visible = if show_all { total } else { 5 };

        card = card.child(
            v_flex()
                .w_full()
                .gap_3()
                .px(crate::ui::design::chat_card_body_pad_x())
                .py(crate::ui::design::chat_card_body_pad_y())
                .children(
                    display_items
                        .iter()
                        .take(visible)
                        .map(|item| self.render_verification_item(item, window, cx))
                        .collect::<Vec<_>>(),
                ),
        );

        let card_key = verification_card_key(agent_id, &verification.id);
        let can_request_fix = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .is_some_and(|session| {
                verification_lifecycle(&session.timeline) == VerificationLifecycle::NeedsFix
                    && session.timeline.iter().rev().find_map(|item| match item {
                        AgentChatTimelineItem::Verification(latest) => Some(latest.id.as_str()),
                        _ => None,
                    }) == Some(verification.id.as_str())
            });
        let pending = if can_request_fix {
            verification.items_to_fix().len()
        } else {
            0
        };
        let expanded = verification.expanded;
        let fix_id = verification.id.clone();
        let toggle_id = verification.id.clone();
        card.when(pending > 0 || collapsible, |card| {
            card.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .py(crate::ui::design::chat_card_head_pad_y())
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .when(pending > 0, |row| {
                        row.child(
                            crate::ui::style::primary_button_compact(
                                ("agent-chat-verification-fix", card_key),
                                format!("Ask to fix ({pending})"),
                                cx,
                            )
                            .icon(IconName::Replace)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.request_agent_verification_fix(
                                        agent_id,
                                        fix_id.clone(),
                                        cx,
                                    );
                                },
                            )),
                        )
                    })
                    .child(div().flex_1())
                    .when(collapsible, |row| {
                        row.child(
                            crate::ui::style::secondary_button_compact(
                                ("agent-chat-verification-toggle", card_key),
                                if expanded {
                                    "Collapse".to_string()
                                } else {
                                    format!("Show all {total}")
                                },
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.agent_chats.update(cx, |chats, cx| {
                                        chats
                                            .toggle_verification_expanded(agent_id, &toggle_id, cx);
                                    });
                                },
                            )),
                        )
                    }),
            )
        })
        .into_any_element()
    }

    fn render_verification_item(
        &self,
        item: &crate::state::agent_chat::VerificationItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (icon, color) = verification_status_style(item.status, cx);
        let fixed = item.fix_requested;
        h_flex()
            .w_full()
            .items_start()
            .gap_2()
            .child(
                div().flex_none().mt(px(2.)).child(
                    gpui_component::Icon::new(icon)
                        .size(crate::ui::design::icon())
                        .text_color(if fixed {
                            crate::ui::design::t3(cx)
                        } else {
                            color
                        }),
                ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(if fixed {
                                        crate::ui::design::t3(cx)
                                    } else {
                                        crate::ui::design::t2(cx)
                                    })
                                    .child(item.title.clone()),
                            )
                            .when(fixed, |row| {
                                row.child(
                                    div()
                                        .flex_none()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(px(crate::ui::style::RADIUS_SM))
                                        .bg(crate::ui::design::sage(cx).opacity(0.14))
                                        .text_color(crate::ui::design::sage(cx))
                                        .text_size(crate::ui::design::text_label())
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child("Fix sent"),
                                )
                            })
                            .child(
                                div()
                                    .flex_none()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded(px(crate::ui::style::RADIUS_SM))
                                    .bg(color.opacity(0.14))
                                    .text_color(color)
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(item.status.label()),
                            ),
                    )
                    .when(!item.detail.trim().is_empty(), |col| {
                        col.child(div().w_full().child(render_plan_markdown(
                            &item.detail,
                            window,
                            cx,
                        )))
                    }),
            )
            .into_any_element()
    }
}

/// Status → icon + accent colour for verification rows.
fn verification_status_style(status: VerificationStatus, cx: &App) -> (IconName, gpui::Hsla) {
    match status {
        VerificationStatus::Met => (IconName::CircleCheck, crate::ui::design::sage(cx)),
        VerificationStatus::Unclear => (IconName::TriangleAlert, crate::ui::design::amber(cx)),
        VerificationStatus::Missed => (IconName::CircleX, crate::ui::design::rose(cx)),
    }
}
