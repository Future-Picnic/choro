use super::*;

impl CenterArea {
    pub(super) fn render_review_checklist_card(
        &self,
        agent_id: Uuid,
        checklist: &ReviewChecklist,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let key = checklist_key(agent_id, &checklist.id);
        // Saved cards used to default to fully expanded. Start with a compact
        // preview even for those records; showing all is a session-local choice.
        let expansion_key = (agent_id, checklist.id.clone());
        let expanded = self
            .agent_chat_expanded_check_cards
            .contains(&expansion_key);
        let total = checklist.items.len();
        let visible_count = if expanded {
            total
        } else {
            total.min(CHAT_CARD_PREVIEW_LIMIT)
        };
        let complete = checklist.is_complete();
        let accent = if complete {
            crate::ui::design::sage(cx)
        } else {
            crate::ui::design::t3(cx)
        };
        let summary = match checklist.status {
            ReviewChecklistStatus::Pending => "Preparing checks…".to_string(),
            ReviewChecklistStatus::Failed => "Couldn’t prepare checks".to_string(),
            ReviewChecklistStatus::Ready if checklist.items.is_empty() => {
                "No manual checks suggested".to_string()
            }
            ReviewChecklistStatus::Ready if complete => {
                format!("All {} checked", checklist.items.len())
            }
            ReviewChecklistStatus::Ready => format!(
                "{} of {} checked",
                checklist.completed_count(),
                checklist.items.len()
            ),
        };

        let mut card = crate::ui::style::chat_card(cx).child(
            crate::ui::style::chat_card_head(cx)
                .child(crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::ListChecks,
                    accent,
                    crate::ui::design::icon_sm(),
                ))
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t2(cx))
                        .child("What to check"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(if complete {
                            crate::ui::design::sage(cx)
                        } else {
                            crate::ui::design::t3(cx)
                        })
                        .child(summary),
                ),
        );

        if checklist.status == ReviewChecklistStatus::Failed {
            let source_turn_id = checklist.source_turn_id.clone();
            return card
                .child(
                    h_flex()
                        .w_full()
                        .px(crate::ui::design::chat_card_body_pad_x())
                        .py(crate::ui::design::chat_card_body_pad_y())
                        .child(
                            crate::ui::style::secondary_button_compact(
                                ("review-checklist-retry", key),
                                "Retry",
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.retry_agent_review_checklist(
                                        agent_id,
                                        source_turn_id.clone(),
                                        cx,
                                    );
                                },
                            )),
                        ),
                )
                .into_any_element();
        }

        if checklist.status != ReviewChecklistStatus::Ready || checklist.items.is_empty() {
            return card.into_any_element();
        }

        let checklist_id = checklist.id.clone();
        let preview_items = checklist.items.iter().take(visible_count);
        card = card.child(
            v_flex()
                .w_full()
                .px(crate::ui::design::chat_card_body_pad_x())
                .py(crate::ui::design::chat_card_body_pad_y())
                .gap_0p5()
                .children(preview_items.enumerate().map(|(index, item)| {
                    let item_id = item.id.clone();
                    let checklist_id = checklist_id.clone();
                    let checked = item.checked;
                    let row_key = key.wrapping_add(index as u64 + 1);
                    let flow = item.flow.as_ref().and_then(|flow| {
                        let previous = index
                            .checked_sub(1)
                            .and_then(|previous| checklist.items.get(previous)?.flow.as_deref());
                        (previous != Some(flow.as_str())).then(|| flow.clone())
                    });
                    let action = item.action_label().to_string();
                    let expected = item.expected.clone();
                    v_flex()
                        .w_full()
                        .when_some(flow, |group, flow| {
                            group.child(
                                div()
                                    .px_3()
                                    .pt_3()
                                    .pb_1()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t2(cx))
                                    .child(flow),
                            )
                        })
                        .child(
                            crate::ui::style::review_checklist_row(cx)
                                .id(("review-checklist-row", row_key))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.agent_chats.update(cx, |chats, cx| {
                                        chats.toggle_review_checklist_item(
                                            agent_id,
                                            &checklist_id,
                                            &item_id,
                                            cx,
                                        );
                                    });
                                    cx.notify();
                                }))
                                .child(crate::ui::style::checkbox(
                                    ("review-checklist-checkbox", row_key),
                                    checked,
                                    cx,
                                ))
                                .child(
                                    div()
                                        .id(("review-checklist-action", row_key))
                                        .min_w(px(0.))
                                        .flex_1()
                                        .text_ellipsis()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(if checked {
                                            crate::ui::design::t3(cx)
                                        } else {
                                            crate::ui::design::t1(cx)
                                        })
                                        .child(action.clone())
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(action.clone()).build(window, cx)
                                        }),
                                )
                                .when_some(expected, |row, expected| {
                                    row.child(
                                        div()
                                            .id(("review-checklist-info", row_key))
                                            .flex_shrink_0()
                                            .child(
                                                Icon::new(IconName::Info)
                                                    .size(crate::ui::design::icon_sm())
                                                    .text_color(crate::ui::design::t3(cx)),
                                            )
                                            .tooltip(move |window, cx| {
                                                Tooltip::new(expected.clone()).build(window, cx)
                                            }),
                                    )
                                }),
                        )
                })),
        );
        card.when(total > CHAT_CARD_PREVIEW_LIMIT, |card| {
            card.child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .py(crate::ui::design::chat_card_head_pad_y())
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .child(
                        crate::ui::style::secondary_button_compact(
                            ("review-checklist-toggle", key),
                            if expanded {
                                "Show less".to_string()
                            } else {
                                format!("Show all {total} checks")
                            },
                        )
                        .icon(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.agent_chat_expanded_check_cards.remove(&expansion_key) {
                                this.agent_chat_expanded_check_cards
                                    .insert(expansion_key.clone());
                            }
                            this.remeasure_agent_chat_list(agent_id);
                            cx.notify();
                        })),
                    ),
            )
        })
        .into_any_element()
    }
}

fn checklist_key(agent_id: Uuid, checklist_id: &str) -> u64 {
    checklist_id
        .bytes()
        .fold(agent_id.as_u128() as u64, |hash, byte| {
            hash.wrapping_mul(1099511628211).wrapping_add(byte as u64)
        })
}
