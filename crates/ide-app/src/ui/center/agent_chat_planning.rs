use super::*;

impl CenterArea {
    pub(super) fn render_proposed_plan_card(
        &self,
        agent_id: Uuid,
        row_index: usize,
        plan: &crate::state::agent_chat::ProposedPlan,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let collapsible = plan.should_collapse();
        // Keyed by plan, not just by agent: two plan cards sharing one element
        // id would share one hitbox, so clicking the revised plan's button
        // toggled the original card instead.
        let card_key = proposed_plan_card_key(agent_id, &plan.id);
        let body = if plan.expanded || !plan.should_collapse() {
            plan.display_markdown()
        } else {
            plan.collapsed_preview(14)
        };
        v_flex()
            .relative()
            .w_full()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow(crate::ui::design::shadow())
            .p_4()
            .gap_3()
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t4(cx))
                                    .child("PLAN"),
                            )
                            .child(div().flex_1())
                            .child(
                                Button::new(("agent-chat-proposed-plan-menu", card_key))
                                    .ghost()
                                    .xsmall()
                                    .compact()
                                    .h(crate::ui::design::control_h_xs())
                                    .icon(IconName::Ellipsis)
                                    .tooltip("Plan actions"),
                            ),
                    )
                    .child(
                        div()
                            .w_full()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .line_height(gpui::relative(1.3))
                            .text_color(crate::ui::design::t1(cx))
                            .child(clean_plan_heading(&plan.title)),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .when(!plan.expanded && collapsible, |body| {
                        body.max_h(px(300.)).overflow_hidden()
                    })
                    .child(render_plan_markdown(&body, window, cx)),
            )
            .when(collapsible, |card| {
                let expanded = plan.expanded;
                let plan_id = plan.id.clone();
                card.child(
                    h_flex().w_full().justify_center().child(
                        crate::ui::style::secondary_button_compact(
                            ("agent-chat-proposed-plan-toggle", card_key),
                            if expanded {
                                "Collapse plan"
                            } else {
                                "Expand plan"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.agent_chats.update(cx, |chats, cx| {
                                chats.toggle_proposed_plan_expanded(agent_id, &plan_id, cx);
                            });
                            this.remeasure_agent_chat_list(agent_id);
                            if !expanded {
                                if let Some(state) = this.agent_chat_list_states.get(&agent_id) {
                                    state.scroll_to(gpui::ListOffset {
                                        item_ix: row_index,
                                        offset_in_item: px(0.),
                                    });
                                }
                                this.agent_chat_scrolled_up.insert(agent_id, true);
                            }
                            cx.notify();
                        })),
                    ),
                )
            })
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::PlanCard,
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_proposed_plan_decision_panel(
        &self,
        agent_id: Uuid,
        plan: &crate::state::agent_chat::ProposedPlan,
        input: Entity<InputState>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let has_feedback = !input.read(cx).value().trim().is_empty();
        let approve_input = input.clone();
        let dismiss_input = input.clone();

        v_flex()
            .w_full()
            .gap_2()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::style::focus_text(cx))
                            .child("Implement this plan?"),
                    )
                    .child(
                        div()
                            .max_w(px(360.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(clean_plan_heading(&plan.title)),
                    ),
            )
            .child(
                h_flex()
                    .id(("proposed-plan-approve", agent_id.as_u128() as u64))
                    .relative()
                    .w_full()
                    .min_h(px(32.))
                    .px_2()
                    .py_1()
                    .gap_2p5()
                    .items_center()
                    .rounded(px(crate::ui::style::RADIUS_SM))
                    .bg(crate::ui::design::hover(cx))
                    .cursor_pointer()
                    .child(crate::ui::style::chat_num_badge("1", true, cx))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .text_color(crate::ui::style::focus_text(cx))
                            .child("Yes, implement this plan"),
                    )
                    .child(crate::ui::onboarding::target_marker(
                        crate::ui::onboarding::SpotlightTarget::PlanApprove,
                        cx,
                    ))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        approve_input.update(cx, |input, cx| input.set_value("", window, cx));
                        this.continue_proposed_plan(agent_id, approve_input.clone(), window, cx);
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(
                                gpui_component::Icon::new(IconName::Info)
                                    .size(crate::ui::design::icon_sm()),
                            )
                            .child("No, tell the agent what to do differently"),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .min_h(px(78.))
                            .rounded(px(crate::ui::style::RADIUS))
                            .border_1()
                            .border_color(crate::ui::style::border(cx))
                            .bg(crate::ui::style::surface(cx))
                            .px_2()
                            .pt_2()
                            .pb_1p5()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(34.))
                                    .child(crate::ui::style::composer_text_input(&input)),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(div().flex_1())
                                    .child(
                                        crate::ui::style::dialog_neutral_button(
                                            ("proposed-plan-dismiss", agent_id.as_u128() as u64),
                                            "Dismiss",
                                            cx,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                dismiss_input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx)
                                                });
                                                this.agent_chats.update(cx, |chats, cx| {
                                                    chats.dismiss_proposed_plan(agent_id, cx);
                                                });
                                            }),
                                        ),
                                    )
                                    .child(
                                        crate::ui::style::primary_button_compact(
                                            (
                                                "proposed-plan-submit-feedback",
                                                agent_id.as_u128() as u64,
                                            ),
                                            if has_feedback { "Submit" } else { "Implement" },
                                            cx,
                                        )
                                        .min_w(px(92.))
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.continue_proposed_plan(
                                                    agent_id,
                                                    input.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_pending_approval_panel(
        &self,
        agent_id: Uuid,
        pending: &crate::state::agent_chat::PendingApproval,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let request_id_for_deny = pending.request_id.clone();
        let request_id_for_approve = pending.request_id.clone();
        let (icon, label) = match pending.kind {
            crate::state::agent_chat::PendingApprovalKind::Command => {
                (IconName::SquareTerminal, "COMMAND APPROVAL")
            }
            crate::state::agent_chat::PendingApprovalKind::FileChange => {
                (IconName::File, "FILE APPROVAL")
            }
            crate::state::agent_chat::PendingApprovalKind::Permissions => {
                (IconName::TriangleAlert, "PERMISSION APPROVAL")
            }
        };
        v_flex()
            .w_full()
            .gap_2p5()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        gpui_component::Icon::new(icon)
                            .size(crate::ui::design::icon_md())
                            .text_color(crate::ui::design::amber(cx)),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::amber(cx))
                            .child(label),
                    ),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::style::focus_text(cx))
                    .child(pending.title.clone()),
            )
            .when_some(pending.detail.clone(), |panel, detail| {
                panel.child(
                    div()
                        .w_full()
                        .max_h(px(150.))
                        .overflow_y_scrollbar()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::line(cx))
                        .bg(crate::ui::design::base(cx).opacity(0.7))
                        .px_2p5()
                        .py_2()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_size(crate::ui::design::text_ui())
                        .line_height(gpui::relative(1.45))
                        .text_color(crate::ui::design::t2(cx))
                        .child(detail),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .pt_1()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1())
                    .child(
                        Button::new(("pending-approval-deny", agent_id.as_u128() as u64))
                            .ghost()
                            .small()
                            .h(crate::ui::design::control_h_xs())
                            .label("Deny")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agent_chats.update(cx, |chats, cx| {
                                    chats.resolve_pending_approval(
                                        agent_id,
                                        &request_id_for_deny,
                                        false,
                                        cx,
                                    );
                                });
                            })),
                    )
                    .child(
                        crate::ui::style::primary_button_compact(
                            ("pending-approval-approve", agent_id.as_u128() as u64),
                            "Approve",
                            cx,
                        )
                        .min_w(px(92.))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.agent_chats.update(cx, |chats, cx| {
                                chats.resolve_pending_approval(
                                    agent_id,
                                    &request_id_for_approve,
                                    true,
                                    cx,
                                );
                            });
                        })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_pending_user_input_panel(
        &self,
        agent_id: Uuid,
        pending: &crate::state::agent_chat::PendingUserInput,
        input: Entity<InputState>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(question) = pending.active_question() else {
            return div().into_any_element();
        };
        let progress = pending.progress();
        let selected = pending
            .active_answer()
            .map(|answer| answer.selected_option_labels.clone())
            .unwrap_or_default();
        let is_last = progress.is_last_question;
        let is_multi_select = question.multi_select;
        let custom_draft = !input.read(cx).value().trim().is_empty();
        let can_advance = progress.can_advance || custom_draft;
        let can_submit = progress.is_complete || (is_last && custom_draft);
        let primary_enabled = if is_last { can_submit } else { can_advance };
        let escape_input = input.clone();
        let dismiss_input = input.clone();

        v_flex()
            .relative()
            .w_full()
            .gap_2()
            .capture_action(cx.listener(move |this, _: &Escape, window, cx| {
                cx.stop_propagation();
                escape_input.update(cx, |input, cx| input.set_value("", window, cx));
                this.agent_chats.update(cx, |chats, cx| {
                    chats.dismiss_pending_user_input(agent_id, cx);
                });
            }))
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::style::focus_text(cx))
                            .child(question.question.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(
                                Button::new(("pending-input-prev", agent_id.as_u128() as u64))
                                    .ghost()
                                    .xsmall()
                                    .compact()
                                    .h(px(22.))
                                    .icon(IconName::ChevronLeft)
                                    .disabled(progress.question_index == 0)
                                    .tooltip("Previous question")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.agent_chats.update(cx, |chats, cx| {
                                            chats
                                                .previous_pending_user_input_question(agent_id, cx);
                                        });
                                    })),
                            )
                            .child(format!(
                                "{} of {}",
                                progress.question_index + 1,
                                progress.total_count
                            ))
                            .child(
                                Button::new((
                                    "pending-input-next-small",
                                    agent_id.as_u128() as u64,
                                ))
                                .ghost()
                                .xsmall()
                                .compact()
                                .h(px(22.))
                                .icon(IconName::ChevronRight)
                                .disabled(is_last || !can_advance)
                                .tooltip("Next question")
                                .on_click(cx.listener({
                                    let input = input.clone();
                                    move |this, _, window, cx| {
                                        this.continue_pending_user_input(
                                            agent_id,
                                            input.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                })),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t3(cx))
                    .child(question.header.to_ascii_uppercase()),
            )
            .child(
                // Options keep their full wrapped descriptions, but the list is
                // capped so a question with many long options can never push the
                // composer past the window edge — it scrolls instead.
                v_flex()
                    .id(("pending-input-options", agent_id.as_u128() as u64))
                    .w_full()
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(question.options.iter().enumerate().map(|(index, option)| {
                        let label = option.label.clone();
                        let label_for_click = label.clone();
                        let description = option.description.clone();
                        let option_input = input.clone();
                        let is_selected = selected.iter().any(|selected| selected == &label);
                        h_flex()
                            .id(("pending-input-option", agent_id.as_u128() as usize ^ index))
                            .w_full()
                            .flex_shrink_0()
                            .min_h(px(32.))
                            .px_2()
                            .py_1()
                            .gap_2p5()
                            .items_center()
                            .rounded(px(crate::ui::style::RADIUS_SM))
                            .cursor_pointer()
                            .when(is_selected, |row| row.bg(crate::ui::design::hover(cx)))
                            .when(!is_selected, |row| {
                                row.hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.5)))
                            })
                            .child(crate::ui::style::chat_num_badge(
                                (index + 1).to_string(),
                                is_selected,
                                cx,
                            ))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::style::focus_text(cx))
                                            .child(label),
                                    )
                                    .when(!description.trim().is_empty(), |col| {
                                        col.child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .line_height(gpui::relative(1.35))
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(description.clone()),
                                        )
                                    }),
                            )
                            .on_click(cx.listener(move |this, _, window, cx| {
                                option_input
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.agent_chats.update(cx, |chats, cx| {
                                    chats.select_pending_user_input_option(
                                        agent_id,
                                        &label_for_click,
                                        !is_multi_select,
                                        cx,
                                    );
                                });
                            }))
                            .into_any_element()
                    })),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1p5()
                            .items_center()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(
                                gpui_component::Icon::new(IconName::Info)
                                    .size(crate::ui::design::icon_sm()),
                            )
                            .child("No, tell the agent what to do differently"),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .min_h(px(78.))
                            .rounded(px(crate::ui::style::RADIUS))
                            .border_1()
                            .border_color(crate::ui::style::border(cx))
                            .bg(crate::ui::style::surface(cx))
                            .px_2()
                            .pt_2()
                            .pb_1p5()
                            .gap_1()
                            .child(
                                div()
                                    .flex_1()
                                    .min_h(px(34.))
                                    .child(crate::ui::style::composer_text_input(&input)),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(div().flex_1())
                                    .child(
                                        crate::ui::style::dialog_neutral_button(
                                            ("pending-input-dismiss", agent_id.as_u128() as u64),
                                            "Dismiss",
                                            cx,
                                        )
                                        .tooltip("Dismiss this question and stop the current turn")
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                dismiss_input.update(cx, |input, cx| {
                                                    input.set_value("", window, cx)
                                                });
                                                this.agent_chats.update(cx, |chats, cx| {
                                                    chats.dismiss_pending_user_input(agent_id, cx);
                                                });
                                            }),
                                        ),
                                    )
                                    .child(
                                        crate::ui::style::primary_button_compact(
                                            ("pending-input-continue", agent_id.as_u128() as u64),
                                            if is_last { "Submit" } else { "Continue" },
                                            cx,
                                        )
                                        .min_w(px(92.))
                                        .disabled(!primary_enabled)
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                this.continue_pending_user_input(
                                                    agent_id,
                                                    input.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }),
                                        ),
                                    ),
                            ),
                    ),
            )
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::Questions,
                cx,
            ))
            .into_any_element()
    }
}
