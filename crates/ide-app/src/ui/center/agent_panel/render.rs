use super::*;

impl Render for AgentShipDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let included = self.included_files();
        let included_count = included.len();
        let staged_outside = self.staged_outside_scope();
        let pr_base_branch_ready = !self.open_pr || !self.pr_base_branch.trim().is_empty();
        let max_dialog_height =
            (f32::from(window.viewport_size().height) - 150.0).clamp(220.0, 720.0);
        let can_prepare = !self.busy
            && !included.is_empty()
            && staged_outside.is_empty()
            && (self.branch.is_some() || self.create_branch)
            && pr_base_branch_ready;
        let can_ship = can_prepare && self.prepared;
        let current_branch = self
            .branch
            .clone()
            .unwrap_or_else(|| "Detached HEAD".into());
        let action = self.current_action();
        let primary_label = if self.busy {
            Self::busy_label(action, self.auto_ship, self.prepared)
        } else if self.auto_ship || self.prepared {
            Self::action_label(action)
        } else {
            "Generate content"
        };
        let primary_disabled = if self.auto_ship {
            !can_prepare
        } else if self.prepared {
            !can_ship
        } else {
            !can_prepare
        };

        v_flex()
            .relative()
            .w_full()
            .max_h(px(max_dialog_height))
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::ShipDialog,
                cx,
            ))
            .child(
                v_flex()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.))
                    .gap_5()
                    .overflow_y_scrollbar()
            // ── Branch + Changes to commit ───────────────────────────────
            .child(
                h_flex()
                    .w_full()
                    .mb_5()
                    .gap_5()
                    .items_start()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_2()
                            .child(Self::ship_label("Branch", cx))
                            .child(
                                crate::ui::style::segmented_container_quiet(cx)
                                    .w_full()
                                    .child(Self::ship_segment(
                                        "ship-branch-current",
                                        "Current",
                                        !self.create_branch,
                                        self.busy,
                                        |this, _, cx| {
                                            this.create_branch = false;
                                            this.reset_prepared();
                                            this.error = None;
                                            cx.notify();
                                        },
                                        cx,
                                    ))
                                    .child(Self::ship_segment(
                                        "ship-branch-new",
                                        "New branch",
                                        self.create_branch,
                                        self.busy,
                                        |this, _, cx| {
                                            this.create_branch = true;
                                            this.reset_prepared();
                                            this.error = None;
                                            crate::ui::onboarding::emit_for_project(
                                                this.project_id,
                                                crate::ui::onboarding::OnboardingEvent::ShipNewBranch,
                                                cx,
                                            );
                                            cx.notify();
                                        },
                                        cx,
                                    )),
                            )
                            .child(if self.create_branch {
                                h_flex()
                                    .w_full()
                                    .h(crate::ui::design::subhead_h())
                                    .items_center()
                                    .rounded(px(crate::ui::style::RADIUS))
                                    .border_1()
                                    .border_color(crate::ui::design::line(cx))
                                    .bg(crate::ui::style::surface(cx))
                                    .px_3()
                                    .child(
                                        Input::new(&self.branch_name)
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .w_full()
                                            .min_w(px(0.)),
                                    )
                                    .into_any_element()
                            } else {
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_1p5()
                                    .h(crate::ui::design::subhead_h())
                                    .px_2p5()
                                    .rounded(px(crate::ui::style::RADIUS))
                                    .border_1()
                                    .border_color(crate::ui::design::line(cx))
                                    .bg(crate::ui::style::surface(cx))
                                    .child(branch_icon(crate::ui::design::t3(cx)))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t1(cx))
                                            .truncate()
                                            .child(SharedString::from(current_branch.clone())),
                                    )
                                    .into_any_element()
                            }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_2()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_1()
                                    .child(Self::ship_label("Changes to commit", cx))
                                    .child(
                                        div()
                                            .id("ship-changes-info")
                                            .child(
                                                gpui_component::Icon::new(IconName::Info)
                                                    .size(crate::ui::design::icon_sm())
                                                    .text_color(crate::ui::design::t3(cx)),
                                            )
                                            .tooltip(|window, cx| {
                                                Tooltip::new("“This chat” commits only files this agent edited. “All changes” commits every change in the repo.")
                                                    .build(window, cx)
                                            }),
                                    ),
                            )
                            .child(
                                crate::ui::style::segmented_container_quiet(cx)
                                    .w_full()
                                    .child(Self::ship_segment(
                                        "ship-scope-conversation",
                                        "This chat",
                                        self.scope == AgentShipScope::Conversation,
                                        self.busy,
                                        |this, _, cx| {
                                            this.scope = AgentShipScope::Conversation;
                                            this.reset_prepared();
                                            this.files_collapsed = false;
                                            this.error = None;
                                            cx.notify();
                                        },
                                        cx,
                                    ))
                                    .child(Self::ship_segment(
                                        "ship-scope-all",
                                        "All changes",
                                        self.scope == AgentShipScope::All,
                                        self.busy,
                                        |this, _, cx| {
                                            this.scope = AgentShipScope::All;
                                            this.reset_prepared();
                                            this.files_collapsed = false;
                                            this.error = None;
                                            crate::ui::onboarding::emit_for_project(
                                                this.project_id,
                                                crate::ui::onboarding::OnboardingEvent::ShipAllChanges,
                                                cx,
                                            );
                                            cx.notify();
                                        },
                                        cx,
                                    )),
                            ),
                    ),
            )
            // ── Files (per-file checkboxes + status letters) ─────────────
            .child(
                v_flex()
                    .w_full()
                    .mb_5()
                    .gap_2()
                    .child(
                        h_flex()
                            .id("ship-files-toggle")
                            .items_center()
                            .gap_1()
                            .cursor_pointer()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.files_collapsed = !this.files_collapsed;
                                cx.notify();
                            }))
                            .child(
                                Icon::new(if self.files_collapsed {
                                    IconName::ChevronRight
                                } else {
                                    IconName::ChevronDown
                                })
                                .size(crate::ui::design::icon_sm())
                                .text_color(crate::ui::design::t3(cx)),
                            )
                            .child(Self::ship_label("Files", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(format!("{included_count}/{}", self.scope_files().len())),
                            ),
                    )
                    .when(!self.files_collapsed, |section| section.child({
                        let files = self.scope_files().to_vec();
                        if files.is_empty() {
                            div()
                                .w_full()
                                .rounded(px(crate::ui::style::RADIUS))
                                .border_1()
                                .border_color(crate::ui::design::line(cx))
                                .bg(crate::ui::style::surface(cx))
                                .px_3()
                                .py_2()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No changes in scope")
                                .into_any_element()
                        } else {
                            v_flex()
                                .w_full()
                                .rounded(px(crate::ui::style::RADIUS))
                                .border_1()
                                .border_color(crate::ui::design::line(cx))
                                .bg(crate::ui::style::surface(cx))
                                .p_1p5()
                                .gap_0p5()
                                .children(
                                    files
                                        .iter()
                                        .enumerate()
                                        .map(|(i, p)| self.render_ship_file(i, p, cx)),
                                )
                                .into_any_element()
                        }
                    })),
            )
            // ── Commit message ───────────────────────────────────────────
            .child(
                v_flex()
                    .w_full()
                    .mb_5()
                    .gap_2()
                    .child(Self::ship_label("Commit message", cx))
                    .child(
                        div()
                            .w_full()
                            .min_h(px(66.))
                            .rounded(px(crate::ui::style::RADIUS))
                            .border_1()
                            .border_color(crate::ui::design::line(cx))
                            .bg(crate::ui::style::surface(cx))
                            .px_3()
                            .py_2()
                            .child(
                                Input::new(&self.commit_message)
                                    .appearance(false)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .w_full()
                                    .min_w(px(0.)),
                            ),
                    ),
            )
            // ── Push / pull request toggles ──────────────────────────────
            .child(
                h_flex()
                    .w_full()
                    .mb_5()
                    .gap_5()
                    .items_center()
                    .child(Self::ship_check(
                        "ship-toggle-push",
                        "Push to remote",
                        self.push || self.open_pr,
                        self.busy || self.open_pr,
                        |this, _, cx| {
                            this.push = !this.push;
                            this.reset_prepared();
                            this.error = None;
                            if !this.push {
                                crate::ui::onboarding::emit_for_project(
                                    this.project_id,
                                    crate::ui::onboarding::OnboardingEvent::ShipPushDisabled,
                                    cx,
                                );
                            }
                            cx.notify();
                        },
                        cx,
                    ))
                    .child(Self::ship_check(
                        "ship-toggle-pr",
                        "Open pull request",
                        self.open_pr,
                        self.busy,
                        |this, _, cx| {
                            this.open_pr = !this.open_pr;
                            this.reset_prepared();
                            this.error = None;
                            cx.notify();
                        },
                        cx,
                    )),
            )
            // ── Pull request fields (revealed when Open pull request) ─────
            .when(self.open_pr, |dialog| {
                dialog
                    .child(
                        v_flex()
                            .w_full()
                            .mb_5()
                            .gap_2()
                            .child(Self::ship_label("Base branch", cx))
                            .child(self.render_base_branch_selector(cx)),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .mb_5()
                            .gap_2()
                            .child(Self::ship_label("Pull request title", cx))
                            .child(
                                div()
                                    .w_full()
                                    .min_h(px(34.))
                                    .rounded(px(crate::ui::style::RADIUS))
                                    .border_1()
                                    .border_color(crate::ui::design::line(cx))
                                    .bg(crate::ui::style::surface(cx))
                                    .px_3()
                                    .py_1p5()
                                    .child(
                                        Input::new(&self.pr_title)
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .w_full()
                                            .min_w(px(0.)),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .mb_5()
                            .gap_2()
                            .child(Self::ship_label("Pull request description", cx))
                            .child(
                                div()
                                    .w_full()
                                    .min_h(px(66.))
                                    .rounded(px(crate::ui::style::RADIUS))
                                    .border_1()
                                    .border_color(crate::ui::design::line(cx))
                                    .bg(crate::ui::style::surface(cx))
                                    .px_3()
                                    .py_2()
                                    .child(
                                        Input::new(&self.pr_description)
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .w_full()
                                            .min_w(px(0.)),
                                    ),
                            ),
                    )
            })
            // ── Inline warnings / status / errors ────────────────────────
            .when(!staged_outside.is_empty(), |dialog| {
                dialog.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::amber(cx))
                        .bg(crate::ui::design::amber(cx).opacity(0.1))
                        .px_3()
                        .py_2()
                        .child(
                            gpui_component::Icon::new(IconName::TriangleAlert)
                                .size(crate::ui::design::icon())
                                .text_color(crate::ui::design::amber(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::amber(cx))
                                .child("Some staged files are outside the selected scope. Unstage them or choose “All files”."),
                        ),
                )
            })
            .when_some(self.error.clone(), |dialog, error| {
                dialog.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx))
                        .bg(crate::ui::design::rose(cx).opacity(0.1))
                        .px_3()
                        .py_2()
                        .child(
                            gpui_component::Icon::new(IconName::TriangleAlert)
                                .size(crate::ui::design::icon())
                                .text_color(crate::ui::design::rose(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::rose(cx))
                                .child(SharedString::from(error)),
                        ),
                )
            })
            )
            // ── Footer ───────────────────────────────────────────────────
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .pt_3()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!(
                                "{} file{} selected",
                                included_count,
                                if included_count == 1 { "" } else { "s" }
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                crate::ui::style::ghost_button("cancel-agent-ship", "Cancel")
                                    .custom(crate::ui::style::dialog_neutral_variant(cx))
                                    .disabled(self.busy)
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .when(self.prepared && !self.auto_ship, |footer| {
                                footer.child(
                                    crate::ui::style::ghost_button(
                                        "agent-ship-regenerate",
                                        "Regenerate",
                                    )
                                    .disabled(!can_prepare)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.prepare(window, cx);
                                    })),
                                )
                            })
                            .child(Self::ship_check(
                                "agent-ship-auto",
                                "Auto",
                                self.auto_ship,
                                self.busy,
                                |this, _, cx| {
                                    this.auto_ship = !this.auto_ship;
                                    this.reset_prepared();
                                    this.error = None;
                                    cx.notify();
                                },
                                cx,
                            ))
                            .when(self.busy, |footer| {
                                footer.child(gpui_component::spinner::Spinner::new().xsmall())
                            })
                            .child(
                                div()
                                    .relative()
                                    .flex_none()
                                    .child(
                                        crate::ui::style::ship_button_primary(
                                            "agent-ship-run",
                                            primary_label,
                                            cx,
                                        )
                                        .disabled(primary_disabled)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if this.auto_ship {
                                                let action = this.current_action();
                                                this.run_auto(action, window, cx);
                                            } else if this.prepared {
                                                crate::ui::onboarding::emit_for_project(
                                                    this.project_id,
                                                    crate::ui::onboarding::OnboardingEvent::ShipCommitting,
                                                    cx,
                                                );
                                                let action = this.current_action();
                                                this.run(action, window, cx);
                                            } else {
                                                crate::ui::onboarding::emit_for_project(
                                                    this.project_id,
                                                    crate::ui::onboarding::OnboardingEvent::ShipPreparing,
                                                    cx,
                                                );
                                                this.prepare(window, cx);
                                            }
                                        })),
                                    )
                                    .child(crate::ui::onboarding::target_marker(
                                        crate::ui::onboarding::SpotlightTarget::ShipPrimary,
                                        cx,
                                    )),
                            ),
                    ),
            )
    }
}
