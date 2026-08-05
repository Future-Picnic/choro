use super::*;

impl CenterArea {
    pub(super) fn render_code_review_card(
        &self,
        agent_id: Uuid,
        review: &crate::state::agent_chat::CodeReview,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let collapsible = review.should_collapse();
        let show_all = review.expanded || !collapsible;
        let total = review.findings.len();
        let visible = if show_all { total } else { 3 };

        let summary = if review.is_clean() {
            "Looks clean".to_string()
        } else if total == 1 {
            "1 finding".to_string()
        } else {
            format!("{total} findings")
        };

        let mut card = crate::ui::style::chat_card(cx).child(
            crate::ui::style::chat_card_head(cx)
                .child(
                    gpui_component::Icon::new(IconName::Inspector)
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::t3(cx)),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::NORMAL)
                        .text_color(crate::ui::design::t2(cx))
                        .child("Code review"),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(summary),
                ),
        );

        if review.is_clean() {
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
                                .child("No real issues found in the changed files."),
                        ),
                )
                .into_any_element();
        }

        let review_id_for_rows = review.id.clone();
        card = card.child(
            v_flex()
                .w_full()
                .px(crate::ui::design::chat_card_body_pad_x())
                .pb(crate::ui::design::chat_card_body_pad_y())
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .py(crate::ui::design::chat_card_body_pad_y())
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(div().flex_none().w(crate::ui::design::icon()))
                        .child(
                            div()
                                .flex_none()
                                .w(gpui::relative(REVIEW_ISSUE_COL))
                                .child("Issue"),
                        )
                        .child(div().flex_1().min_w(px(0.)).child("What happens"))
                        .child(div().flex_1().min_w(px(0.)).child("Suggested fix")),
                )
                .children(
                    review
                        .findings
                        .iter()
                        .take(visible)
                        .enumerate()
                        .map(|(i, finding)| {
                            self.render_code_review_finding(
                                agent_id,
                                &review_id_for_rows,
                                i,
                                finding,
                                window,
                                cx,
                            )
                        })
                        .collect::<Vec<_>>(),
                ),
        );

        let card_key = code_review_card_key(agent_id, &review.id);
        let selected = review.selected_count();
        let has_pending = review.has_pending_fixes();
        let expanded = review.expanded;
        let fix_id = review.id.clone();
        let fix_all_id = review.id.clone();
        let toggle_id = review.id.clone();
        card.when(has_pending || collapsible, |card| {
            card.child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .px(crate::ui::design::chat_card_body_pad_x())
                    .py(crate::ui::design::chat_card_head_pad_y())
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .when(has_pending, |row| {
                        row.child(
                            crate::ui::style::primary_button_compact(
                                ("agent-chat-code-review-fix", card_key),
                                if selected > 0 {
                                    format!("Fix selected ({selected})")
                                } else {
                                    "Fix all".to_string()
                                },
                                cx,
                            )
                            .icon(IconName::Replace)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.request_agent_code_review_fix(
                                        agent_id,
                                        fix_id.clone(),
                                        selected > 0,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .when(selected > 0, |row| {
                            row.child(
                                crate::ui::style::secondary_button_compact(
                                    ("agent-chat-code-review-fix-all", card_key),
                                    "Fix all",
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.request_agent_code_review_fix(
                                            agent_id,
                                            fix_all_id.clone(),
                                            false,
                                            cx,
                                        );
                                    },
                                )),
                            )
                        })
                    })
                    .child(div().flex_1())
                    .when(collapsible, |row| {
                        row.child(
                            crate::ui::style::secondary_button_compact(
                                ("agent-chat-code-review-toggle", card_key),
                                if expanded {
                                    "Collapse".to_string()
                                } else {
                                    format!("Show all {total}")
                                },
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.agent_chats.update(cx, |chats, cx| {
                                        chats.toggle_code_review_expanded(agent_id, &toggle_id, cx);
                                    });
                                },
                            )),
                        )
                    }),
            )
        })
        .into_any_element()
    }

    pub(super) fn render_code_review_finding(
        &self,
        agent_id: Uuid,
        review_id: &str,
        index: usize,
        finding: &crate::state::agent_chat::CodeReviewFinding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let color = code_review_severity_color(finding.severity, cx);
        let fixed = finding.fix_requested;
        let review_id = review_id.to_string();
        h_flex()
            .w_full()
            .items_start()
            .gap_2()
            .py_2()
            .border_t_1()
            .border_color(crate::ui::design::line(cx))
            .child(if fixed {
                div()
                    .flex_none()
                    .mt(px(2.))
                    .child(
                        gpui_component::Icon::new(IconName::CircleCheck)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::sage(cx)),
                    )
                    .into_any_element()
            } else {
                crate::ui::style::checkbox(
                    (
                        "agent-chat-code-review-finding",
                        code_review_finding_key(agent_id, &review_id, index),
                    ),
                    finding.selected,
                    cx,
                )
                .mt(px(2.))
                .on_click(cx.listener({
                    let review_id = review_id.clone();
                    move |this, _, _, cx| {
                        this.agent_chats.update(cx, |chats, cx| {
                            chats.toggle_code_review_finding_selected(
                                agent_id, &review_id, index, cx,
                            );
                        });
                    }
                }))
                .into_any_element()
            })
            .child(
                v_flex()
                    .flex_none()
                    .w(gpui::relative(REVIEW_ISSUE_COL))
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_1p5()
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
                                    .child(finding.severity.label()),
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
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_body())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(if fixed {
                                crate::ui::design::t3(cx)
                            } else {
                                crate::ui::design::t2(cx)
                            })
                            .child(finding.title.clone()),
                    )
                    .when_some(finding.location.clone(), |col, location| {
                        col.child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .font_family(crate::ui::design::FONT_MONO)
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(location),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .when(!finding.impact.trim().is_empty(), |cell| {
                        cell.child(render_plan_markdown(&finding.impact, window, cx))
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .when_some(finding.fix.clone(), |cell, fix| {
                        cell.child(render_plan_markdown(&fix, window, cx))
                    }),
            )
            .into_any_element()
    }
}

/// Fraction of the row given to the issue column (severity, title, location);
/// the consequence and fix columns split the remainder evenly. Shared by the
/// header row and finding rows so the columns stay aligned.
const REVIEW_ISSUE_COL: f32 = 0.3;
