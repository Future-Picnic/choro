//! The structured review card: live checked findings, terminal state,
//! coverage, limitations and freshness. Legacy Markdown reviews keep the
//! original card in `agent_chat_review.rs` and never gain these claims.
use super::agent_chat_review_status::{review_outcome, ReviewOutcome, ReviewTone};
use super::*;

/// How many unreviewed paths to name before summarising the rest.
const UNREVIEWED_NAMED: usize = 6;

fn tone_color(tone: ReviewTone, cx: &App) -> gpui::Hsla {
    match tone {
        ReviewTone::Live => crate::ui::design::teal(cx),
        ReviewTone::Clean => crate::ui::design::sage(cx),
        ReviewTone::Findings => crate::ui::design::t2(cx),
        ReviewTone::Incomplete | ReviewTone::Stale => crate::ui::design::amber(cx),
        ReviewTone::Failed => crate::ui::design::rose(cx),
    }
}

fn state_label(outcome: &ReviewOutcome) -> &'static str {
    if outcome.stale {
        "Outdated"
    } else {
        outcome.state
    }
}

fn state_tag(outcome: &ReviewOutcome, cx: &App) -> gpui::Div {
    let color = tone_color(outcome.tone, cx);
    let label = state_label(outcome);
    div()
        .flex_none()
        .px_1p5()
        .py_0p5()
        .rounded(px(crate::ui::style::RADIUS_SM))
        .bg(color.opacity(0.14))
        .text_color(color)
        .text_size(crate::ui::design::text_label())
        .font_weight(FontWeight::SEMIBOLD)
        .debug_selector(move || format!("review-state-{label}"))
        .child(label)
}

fn note_list(title: &'static str, rows: Vec<String>, cx: &App) -> gpui::Div {
    v_flex()
        .gap_0p5()
        .child(
            div()
                .text_color(crate::ui::design::t2(cx))
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .children(rows.into_iter().map(|row| {
            div()
                .min_w(px(0.))
                .text_color(crate::ui::design::t3(cx))
                .child(row)
        }))
}

/// Everything the card shows that is not derived from the run itself.
pub(super) struct ReviewCardModel {
    pub outcome: ReviewOutcome,
    pub key: u64,
    pub rows: Vec<gpui::AnyElement>,
    pub total: usize,
    pub collapsible: bool,
    pub expanded: bool,
    pub selected: usize,
    pub has_pending: bool,
    /// A Fix freshness check is in flight for this card.
    pub checking: bool,
    pub fix_notice: Option<String>,
    /// This card is the conversation's newest review and nothing holds it.
    pub retry_allowed: bool,
    pub details_expanded: bool,
}

pub(super) struct ReviewCardActions {
    pub retry: Rc<dyn Fn(&mut Window, &mut App)>,
    /// `true` fixes only the selected findings.
    pub fix: Rc<dyn Fn(bool, &mut Window, &mut App)>,
    pub toggle: Rc<dyn Fn(&mut Window, &mut App)>,
    pub details_toggle: Rc<dyn Fn(&mut Window, &mut App)>,
}

pub(super) fn structured_review_card(
    model: ReviewCardModel,
    actions: ReviewCardActions,
    cx: &App,
) -> gpui::AnyElement {
    let ReviewCardModel {
        outcome,
        key,
        rows,
        total,
        collapsible,
        expanded,
        selected,
        has_pending,
        checking,
        fix_notice,
        retry_allowed,
        details_expanded,
    } = model;
    let head_mark = if outcome.tone == ReviewTone::Live {
        crate::ui::logo_spinner::review_spinner(
            14.,
            "agent-review-card-spinner",
            key as usize,
            crate::ui::design::teal(cx),
        )
    } else {
        gpui_component::Icon::new(IconName::Inspector)
            .size(crate::ui::design::icon_sm())
            .text_color(crate::ui::design::t3(cx))
            .into_any_element()
    };
    let summary = outcome.summary.clone();
    let mut card = crate::ui::style::chat_card(cx)
        .debug_selector(|| "structured-review-card".into())
        .child(
            crate::ui::style::chat_card_head(cx)
                .child(head_mark)
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t2(cx))
                        .child("Code review"),
                )
                .child(state_tag(&outcome, cx))
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .debug_selector(move || format!("review-summary-{summary}"))
                        .child(outcome.summary.clone()),
                ),
        );

    let mut details = v_flex()
        .w_full()
        .gap_2()
        .px(crate::ui::design::chat_card_body_pad_x())
        .py(crate::ui::design::chat_card_body_pad_y())
        .text_size(crate::ui::design::text_ui())
        .when_some(outcome.notice.clone(), |col, notice| {
            col.child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t2(cx))
                    .debug_selector(|| "review-notice".into())
                    .child(notice),
            )
        })
        .child(
            div()
                .text_color(crate::ui::design::t3(cx))
                .child(outcome.coverage.clone()),
        );
    let has_details = !outcome.limitations.is_empty() || !outcome.skipped.is_empty()
        || !outcome.unreviewed.is_empty() || !outcome.reviewed.is_empty();
    if !outcome.reviewed.is_empty() {
        let named = if details_expanded { outcome.reviewed.clone() }
            else { outcome.reviewed.iter().take(3).cloned().collect() };
        details = details.child(note_list("Checked files", named, cx).debug_selector(|| "review-checked".into()));
    }
    if !details_expanded && (!outcome.limitations.is_empty() || !outcome.skipped.is_empty()) {
        details = details.child(div().text_color(crate::ui::design::t3(cx))
            .debug_selector(|| "review-gap-summary".into())
            .child(format!("{} coverage notes · {} {} with unavailable changes", outcome.limitations.len(), outcome.skipped.len(), if outcome.skipped.len() == 1 { "file" } else { "files" })));
    }
    if !details_expanded && !outcome.skipped.is_empty() {
        let mut named: Vec<_> = outcome.skipped.iter().take(3).map(|(path, reason)| {
            let name = std::path::Path::new(path).file_name().unwrap_or_default().to_string_lossy();
            format!("{name}: {}", super::agent_chat_review_status::unavailable_reason(reason))
        }).collect();
        if outcome.skipped.len() > 3 { named.push(format!("and {} more", outcome.skipped.len() - 3)); }
        details = details.child(note_list("Unavailable changes", named, cx).debug_selector(|| "review-unavailable".into()));
    }
    if details_expanded && !outcome.limitations.is_empty() {
        details = details.child(
            note_list("Limitations", outcome.limitations.clone(), cx)
                .debug_selector(|| "review-limitations".into()),
        );
    }
    if details_expanded && !outcome.skipped.is_empty() {
        details = details.child(
            note_list(
                "Unavailable changes",
                outcome
                    .skipped
                    .iter()
                    .map(|(path, reason)| format!("{path}: {reason}"))
                    .collect(),
                cx,
            )
            .debug_selector(|| "review-skipped".into()),
        );
    }
    if !outcome.unreviewed.is_empty() {
        let limit = if details_expanded { outcome.unreviewed.len() } else { UNREVIEWED_NAMED };
        let mut named = outcome
            .unreviewed
            .iter()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>();
        if outcome.unreviewed.len() > limit {
            named.push(format!(
                "and {} more",
                outcome.unreviewed.len() - limit
            ));
        }
        details = details.child(
            note_list("Not reviewed", named, cx).debug_selector(|| "review-unreviewed".into()),
        );
    }
    card = card.child(details);

    if !rows.is_empty() {
        card = card.child(
            v_flex()
                .w_full()
                .px(crate::ui::design::chat_card_body_pad_x())
                .pb(crate::ui::design::chat_card_body_pad_y())
                .child(super::agent_chat_review::review_findings_header(cx))
                .children(rows),
        );
    }

    let can_fix = outcome.can_fix && has_pending;
    let retry = retry_allowed
        && (outcome.stale || matches!(outcome.tone, ReviewTone::Incomplete | ReviewTone::Failed));
    if !(can_fix || retry || collapsible || has_details || fix_notice.is_some()) {
        return card.into_any_element();
    }
    let ReviewCardActions { retry: on_retry, fix, toggle, details_toggle } = actions;
    let fix_all = fix.clone();
    card.child(
        v_flex()
            .w_full()
            .border_t_1()
            .border_color(crate::ui::design::line(cx))
            .px(crate::ui::design::chat_card_body_pad_x())
            .py(crate::ui::design::chat_card_head_pad_y())
            .gap_1p5()
            .when_some(fix_notice, |col, notice| {
                col.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::amber(cx))
                        .debug_selector(|| "review-fix-notice".into())
                        .child(notice),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .when(retry, |row| {
                        let label = if outcome.stale {
                            "Review latest changes"
                        } else {
                            "Review again"
                        };
                        let button = if outcome.stale {
                            crate::ui::style::primary_button_compact(
                                ("agent-review-retry", key),
                                label,
                                cx,
                            )
                        } else {
                            crate::ui::style::secondary_button_compact(
                                ("agent-review-retry", key),
                                label,
                            )
                        };
                        row.child(
                            div()
                                .flex_none()
                                .debug_selector(move || format!("review-retry-{label}"))
                                .child(
                                    button
                                        .icon(IconName::Inspector)
                                        .tooltip(
                                            "Start a new review of all this conversation's changes",
                                        )
                                        .on_click(move |_, window, cx| on_retry(window, cx)),
                                ),
                        )
                    })
                    .when(can_fix && checking, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .debug_selector(|| "review-fix-checking".into())
                                .child(crate::ui::style::busy_button_compact(
                                    ("agent-review-fix-checking", key),
                                    "Checking the code is unchanged…",
                                    cx,
                                )),
                        )
                    })
                    .when(can_fix && !checking, |row| {
                        row.child(
                            div()
                                .flex_none()
                                .debug_selector(|| "review-fix".into())
                                .child(
                                    crate::ui::style::primary_button_compact(
                                        ("agent-chat-code-review-fix", key),
                                        if selected > 0 {
                                            format!("Fix selected ({selected})")
                                        } else {
                                            "Fix all".to_string()
                                        },
                                        cx,
                                    )
                                    .icon(IconName::Replace)
                                    .on_click(move |_, window, cx| fix(selected > 0, window, cx)),
                                ),
                        )
                        .when(selected > 0, |row| {
                            row.child(
                                crate::ui::style::secondary_button_compact(
                                    ("agent-chat-code-review-fix-all", key),
                                    "Fix all",
                                )
                                .on_click(move |_, window, cx| fix_all(false, window, cx)),
                            )
                        })
                    })
                    .child(div().flex_1())
                    .when(has_details, |row| {
                        row.child(crate::ui::style::secondary_button_compact(
                            ("review-details-toggle", key),
                            if details_expanded { "Hide review details" } else { "Show review details" })
                            .debug_selector(|| "review-details-toggle".into())
                            .on_click(move |_, window, cx| details_toggle(window, cx)))
                    })
                    .when(collapsible, |row| {
                        row.child(
                            crate::ui::style::secondary_button_compact(
                                ("agent-chat-code-review-toggle", key),
                                if expanded {
                                    "Collapse".to_string()
                                } else {
                                    format!("Show all {total}")
                                },
                            )
                            .on_click(move |_, window, cx| toggle(window, cx)),
                        )
                    }),
            ),
    )
    .into_any_element()
}

impl CenterArea {
    pub(super) fn render_structured_review_card(
        &self,
        agent_id: Uuid,
        review: &crate::state::agent_chat::CodeReview,
        run: &ide_core::code_review::ReviewRun,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (reviewer_active, latest, holding) = {
            let chats = self.agent_chats.read(cx);
            let latest = chats.review_run(agent_id).is_none_or(|r| r.id == run.id);
            let holding = chats.review_blocks_writing(agent_id);
            (holding && latest, latest, holding)
        };
        let outcome = review_outcome(run, reviewer_active);
        let collapsible = review.should_collapse();
        let total = review.findings.len();
        let visible = if review.expanded || !collapsible { total } else { 3 };
        let rows = review
            .findings
            .iter()
            .take(visible)
            .enumerate()
            .map(|(i, finding)| {
                self.render_code_review_finding(agent_id, &review.id, i, finding, window, cx)
            })
            .collect();
        let view = cx.entity().downgrade();
        let retry_view = view.clone();
        let toggle_view = view.clone();
        let details_view = view.clone();
        let details_id = review.id.clone();
        let fix_id = review.id.clone();
        let toggle_id = review.id.clone();
        structured_review_card(
            ReviewCardModel {
                // Only the newest review offers a new one; a whole-scope retry
                // from an older card would be indistinguishable from it.
                retry_allowed: latest && !holding && run.state.terminal(),
                outcome,
                key: code_review_card_key(agent_id, &review.id),
                rows,
                total,
                collapsible,
                expanded: review.expanded,
                details_expanded: self.agent_review_ui.detail_panels.contains(&(agent_id, review.id.clone())),
                selected: review.selected_count(),
                has_pending: review.has_pending_fixes(),
                checking: self.agent_review_ui.fix_checking(agent_id, &review.id),
                fix_notice: self
                    .agent_review_ui
                    .fix_notices
                    .get(&(agent_id, review.id.clone()))
                    .cloned(),
            },
            ReviewCardActions {
                retry: Rc::new(move |window, cx| {
                    let _ = retry_view
                        .update(cx, |this, cx| this.start_agent_review(agent_id, window, cx));
                }),
                fix: Rc::new(move |only_selected, _, cx| {
                    let _ = view.update(cx, |this, cx| {
                        this.request_validated_code_review_fix(
                            agent_id,
                            fix_id.clone(),
                            only_selected,
                            cx,
                        )
                    });
                }),
                toggle: Rc::new(move |_, cx| {
                    let _ = toggle_view.update(cx, |this, cx| {
                        this.agent_chats.update(cx, |chats, cx| {
                            chats.toggle_code_review_expanded(agent_id, &toggle_id, cx);
                        })
                    });
                }),
                details_toggle: Rc::new(move |_, cx| {
                    let _ = details_view.update(cx, |this, cx| {
                        let key = (agent_id, details_id.clone());
                        if !this.agent_review_ui.detail_panels.remove(&key) { this.agent_review_ui.detail_panels.insert(key); }
                        this.remeasure_agent_chat_list(agent_id);
                        cx.notify();
                    });
                }),
            },
            cx,
        )
    }
}

#[cfg(all(test, feature = "ui-layout-tests"))]
mod tests {
    use super::*;
    use ide_core::code_review::{
        ReviewFile, ReviewFileStatus, ReviewFinding, ReviewFreshness, ReviewLocation, ReviewRun,
        ReviewRunState, ReviewSeverity, ReviewSide,
    };
    use std::cell::Cell;

    struct CardFixture {
        run: ReviewRun,
        checking: bool,
        retries: Rc<Cell<usize>>,
        fixes: Rc<Cell<usize>>,
        details_expanded: bool,
    }

    impl Render for CardFixture {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let retries = self.retries.clone();
            let fixes = self.fixes.clone();
            let outcome = review_outcome(&self.run, false);
            let view = cx.entity().downgrade();
            div().w(px(760.)).child(structured_review_card(
                ReviewCardModel {
                    outcome,
                    key: 1,
                    rows: vec![],
                    total: self.run.findings.len(),
                    collapsible: false,
                    expanded: false,
                    selected: 0,
                    has_pending: !self.run.findings.is_empty(),
                    checking: self.checking,
                    fix_notice: None,
                    retry_allowed: true,
                    details_expanded: self.details_expanded,
                },
                ReviewCardActions {
                    retry: Rc::new(move |_, _| retries.set(retries.get() + 1)),
                    fix: Rc::new(move |_, _, _| fixes.set(fixes.get() + 1)),
                    toggle: Rc::new(|_, _| {}),
                    details_toggle: Rc::new(move |_, cx| {
                        let _ = view.update(cx, |this, cx| { this.details_expanded = !this.details_expanded; cx.notify(); });
                    }),
                },
                cx,
            ))
        }
    }

    fn run(state: ReviewRunState, freshness: ReviewFreshness, findings: bool) -> ReviewRun {
        let mut run = ReviewRun::new(
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            "Claude".into(),
            "model".into(),
            "effort".into(),
            1,
        );
        run.state = state;
        run.freshness = freshness;
        let file = |path: &str, status| ReviewFile {
            id: path.into(),
            path: path.into(),
            change_kind: "modified".into(),
            attributed_ranges: vec![],
            before_hash: None,
            after_hash: None,
            diff_pages: 1,
            consumed_pages: Default::default(),
            status,
            skip_reason: None,
        };
        run.files = vec![file("src/a.rs", ReviewFileStatus::Complete)];
        if state != ReviewRunState::Complete {
            run.files.push(file("src/b.rs", ReviewFileStatus::Pending));
            run.limitations.push("Five-minute deadline reached".into());
        }
        if findings {
            run.findings.push(ReviewFinding {
                id: "f1".into(),
                severity: ReviewSeverity::High,
                location: ReviewLocation {
                    file_id: "src/a.rs".into(),
                    path: "src/a.rs".into(),
                    side: ReviewSide::After,
                    start: 3,
                    end: 4,
                },
                title: "Saves can overwrite each other".into(),
                trigger: "Two saves".into(),
                consequence: "Lost edit".into(),
                suggested_fix: "Compare revisions".into(),
                evidence: vec![],
                challenge: "No guard found".into(),
            });
        }
        run
    }

    /// Renders one card in a fresh window (debug bounds persist across frames
    /// within a window, so each case gets its own) and reports what it shows.
    fn render(
        cx: &mut gpui::TestAppContext,
        run: ReviewRun,
        checking: bool,
    ) -> (
        &mut gpui::VisualTestContext,
        Rc<Cell<usize>>,
        Rc<Cell<usize>>,
    ) {
        cx.update(gpui_component::init);
        let retries = Rc::new(Cell::new(0));
        let fixes = Rc::new(Cell::new(0));
        let (r, f) = (retries.clone(), fixes.clone());
        let (_, cx) = cx.add_window_view(move |window, cx| {
            let view = cx.new(|_| CardFixture {
                run,
                checking,
                retries: r,
                fixes: f,
                details_expanded: false,
            });
            gpui_component::Root::new(view, window, cx)
        });
        cx.run_until_parked();
        (cx, retries, fixes)
    }

    #[gpui::test]
    fn partial_card_states_its_gaps_and_offers_a_retry_not_a_clean_result(
        cx: &mut gpui::TestAppContext,
    ) {
        let (cx, retries, _) = render(
            cx,
            run(ReviewRunState::Partial, ReviewFreshness::Current, false),
            false,
        );
        assert!(cx.debug_bounds("review-state-Partial").is_some());
        assert!(cx.debug_bounds("review-summary-No findings confirmed").is_some());
        assert!(cx.debug_bounds("review-summary-No findings").is_none());
        assert!(cx.debug_bounds("review-notice").is_some());
        assert!(cx.debug_bounds("review-limitations").is_none());
        assert!(cx.debug_bounds("review-gap-summary").is_some());
        assert!(cx.debug_bounds("review-unreviewed").is_some());
        assert!(cx.debug_bounds("review-fix").is_none(), "nothing to fix");
        let retry = cx
            .debug_bounds("review-retry-Review again")
            .expect("partial review offers a fresh review");
        cx.simulate_click(retry.center(), gpui::Modifiers::none());
        assert_eq!(retries.get(), 1);
    }

    #[gpui::test]
    fn large_coverage_reports_start_compact_and_reveal_all_details(cx: &mut gpui::TestAppContext) {
        let mut current = run(ReviewRunState::Partial, ReviewFreshness::Current, false);
        current.limitations.extend((0..46).map(|n| format!("src/file{n}.rs has missing evidence")));
        current.limitations.push("Five-minute deadline reached; coverage is incomplete".into());
        let (cx, _, _) = render(cx, current, false);
        assert!(cx.debug_bounds("review-limitations").is_none());
        assert!(cx.debug_bounds("review-gap-summary").is_some());
        let card = cx.debug_bounds("structured-review-card").unwrap();
        assert!(card.size.height < px(500.), "raw coverage notes must not bury actions");
        let toggle = cx.debug_bounds("review-details-toggle").unwrap();
        cx.simulate_click(toggle.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(cx.debug_bounds("review-limitations").is_some());
        // GPUI retains debug bounds across frames. Check the new content and
        // geometry rather than asserting disappearance of old debug bounds.
        let expanded_height = cx.debug_bounds("structured-review-card").unwrap().size.height;
        assert!(expanded_height > card.size.height);
    }

    #[gpui::test]
    fn finished_review_keeps_fixes_and_names_missing_edits_without_expanding_details(cx: &mut gpui::TestAppContext) {
        let mut current = run(ReviewRunState::Partial,ReviewFreshness::Current,true);
        current.finalized_by_reviewer = true;
        current.limitations = vec!["Missing earlier edit contents".into()];
        current.files[1].status = ReviewFileStatus::Skipped;
        current.files[1].path = "crates/ide-app/src/state/agent_chat/review_controller.rs".into();
        current.files[1].skip_reason = Some("Confirmed mutation lacks inspectable before/after contents".into());
        let (cx,_,fixes) = render(cx,current,false);
        assert!(cx.debug_bounds("review-state-Finished").is_some());
        assert!(cx.debug_bounds("review-summary-1 finding").is_some());
        assert!(cx.debug_bounds("review-unavailable").is_some());
        assert!(cx.debug_bounds("review-unreviewed").is_none());
        assert!(cx.debug_bounds("review-limitations").is_none());
        let fix = cx.debug_bounds("review-fix").expect("Available checked findings remain actionable");
        cx.simulate_click(fix.center(),gpui::Modifiers::none());
        assert_eq!(fixes.get(),1);
    }

    #[gpui::test]
    fn outdated_card_replaces_fix_with_review_latest_changes(cx: &mut gpui::TestAppContext) {
        let (cx, retries, fixes) = render(
            cx,
            run(
                ReviewRunState::Complete,
                ReviewFreshness::Outdated("src/a.rs changed".into()),
                true,
            ),
            false,
        );
        assert!(cx.debug_bounds("review-state-Outdated").is_some());
        assert!(cx.debug_bounds("review-fix").is_none(), "stale evidence cannot be fixed");
        let retry = cx
            .debug_bounds("review-retry-Review latest changes")
            .expect("outdated review offers the latest changes");
        cx.simulate_click(retry.center(), gpui::Modifiers::none());
        assert_eq!((retries.get(), fixes.get()), (1, 0));
    }

    #[gpui::test]
    fn current_findings_fix_through_the_gate_and_show_the_check_in_flight(
        cx: &mut gpui::TestAppContext,
    ) {
        let current = run(ReviewRunState::Complete, ReviewFreshness::Current, true);
        let (cx, retries, fixes) = render(cx, current.clone(), false);
        assert!(cx.debug_bounds("review-state-Complete").is_some());
        assert!(cx.debug_bounds("review-retry-Review again").is_none());
        let fix = cx.debug_bounds("review-fix").expect("current findings can be fixed");
        cx.simulate_click(fix.center(), gpui::Modifiers::none());
        assert_eq!((retries.get(), fixes.get()), (0, 1));
    }

    #[gpui::test]
    fn fix_in_flight_shows_a_busy_check_instead_of_a_second_fix(cx: &mut gpui::TestAppContext) {
        let (cx, _, _) = render(
            cx,
            run(ReviewRunState::Complete, ReviewFreshness::Current, true),
            true,
        );
        assert!(cx.debug_bounds("review-fix-checking").is_some());
        assert!(cx.debug_bounds("review-fix").is_none());
    }

    #[gpui::test]
    fn clean_card_reads_clean_without_actions(cx: &mut gpui::TestAppContext) {
        let (cx, _, _) = render(
            cx,
            run(ReviewRunState::Complete, ReviewFreshness::Current, false),
            false,
        );
        assert!(cx.debug_bounds("review-summary-No findings").is_some());
        assert!(cx.debug_bounds("review-retry-Review again").is_none());
        assert!(cx.debug_bounds("review-notice").is_none());
    }
}
