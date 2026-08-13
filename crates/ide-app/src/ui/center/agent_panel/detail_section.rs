use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn render_agent_section(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if self
            .new_agent_composer
            .as_ref()
            .is_some_and(|composer| composer.project == project)
        {
            return self.render_new_agent_composer(project, cx);
        }

        let Some(agent) = self.agents.read(cx).selected_agent(project) else {
            return v_flex()
                .size_full()
                .min_h(px(280.))
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .w(px(160.))
                        .h(px(110.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::ui::illustrations::illustration(
                                crate::ui::illustrations::Illustration::Agents,
                                cx,
                            )
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_title())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child("No agents yet"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Create one from the Agents tab on the right"),
                        ),
                )
                .into_any_element();
        };

        let detail_tab = *self
            .agent_detail_tabs
            .entry(agent.id)
            .or_insert(AgentDetailTab::Terminal);
        let notes_input = self.agent_notes_input(&agent, window, cx);
        let runtime = self.agent_runtime(&agent, project, cx);
        let (terminal_view, terminal_exited) = {
            let manager = self.terminals.read(cx);
            manager
                .agent_record_session(project, agent.id)
                .map(|session| (Some(session.view.clone()), session.exited))
                .unwrap_or((None, false))
        };
        let start_label = match runtime {
            AgentRuntime::NotStarted => "Start",
            AgentRuntime::Working | AgentRuntime::Waiting | AgentRuntime::Open => "Focus",
            AgentRuntime::Idle | AgentRuntime::Ended => "Resume",
        };
        if !self.agent_ship_pr_targets.contains_key(&agent.id) {
            if let (Some(repo_path), Some(branch)) = (
                agent.ship_pr_repo_path.clone(),
                agent.ship_pr_branch.clone(),
            ) {
                self.track_agent_ship_pr_branch(agent.id, repo_path, branch, cx);
            }
        }
        self.sync_agent_ship_pull_request(agent.id, cx);
        let agent_ship_pr = self.agent_ship_prs.get(&agent.id).cloned();
        let status_view = cx.entity().clone();
        let current_status = agent.status;
        let agent_id = agent.id;
        let linked_docs = agent.linked_docs.clone();
        let linked_tasks = agent.linked_tasks.clone();
        let linked_designs = self.penpot.read(cx).designs_for_agent(&agent);
        let linked_design_indicators = linked_designs
            .into_iter()
            .map(|design| {
                self.render_linked_design_indicator(
                    ("agent-linked-design", design.id.as_u128() as u64),
                    design,
                    cx,
                )
            })
            .collect::<Vec<_>>();
        // Deliberately the project root, not `runtime_path()`: linked docs are
        // project-level documents, so a Solo agent's chips open the project's
        // copy rather than a shadow inside its lane.
        let project_path = agent.project_path.clone();
        let solo_branch = agent.solo_branch.clone();
        let has_header_metadata = !linked_tasks.is_empty()
            || agent_ship_pr.is_some()
            || !linked_docs.is_empty()
            || !linked_design_indicators.is_empty();
        let agent_status_accent = status_accent(agent.status, cx);
        let title_edit_input = self
            .agent_title_edit
            .as_ref()
            .filter(|edit| edit.agent_id == agent_id)
            .map(|edit| edit.input.clone());
        let title_hovered = self.hovered_agent_title == Some(agent_id);
        let plan_drawer = (agent.runtime == AgentRuntimeKind::Chat
            && detail_tab == AgentDetailTab::Plan)
            .then(|| {
                self.agent_chats
                    .read(cx)
                    .session(agent.id)
                    .and_then(|session| session.proposed_plan.clone())
            })
            .flatten();
        let diff_drawer = (detail_tab == AgentDetailTab::Diff)
            .then(|| self.agent_diff_drawers.get(&agent.id).cloned())
            .flatten();
        let detail_body: gpui::AnyElement = if agent.runtime == AgentRuntimeKind::Chat {
            self.render_agent_chat_body(&agent, window, cx)
        } else if let Some(view) = terminal_view {
            div()
                .relative()
                .size_full()
                .child(view)
                .when(terminal_exited, |area| {
                    area.child(
                        div()
                            .absolute()
                            .top(px(12.))
                            .right(px(12.))
                            .px_2()
                            .py_1()
                            .rounded(crate::ui::design::r_sm())
                            .text_size(crate::ui::design::text_ui())
                            .bg(crate::ui::design::surface(cx))
                            .text_color(crate::ui::design::t3(cx))
                            .child("Terminal ended"),
                    )
                })
                .into_any_element()
        } else {
            v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(
                    gpui_component::Icon::new(IconName::SquareTerminal)
                        .size_8()
                        .text_color(crate::ui::design::t3(cx)),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("No terminal attached"),
                )
                .child(
                    Button::new(("start-agent-empty", agent.id.as_u128() as u64))
                        .outline()
                        .small()
                        .icon(IconName::Plus)
                        .label(start_label)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.start_agent(agent_id, window, cx);
                        })),
                )
                .into_any_element()
        };

        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::agent_chat_shell(cx).child(
                    crate::ui::design::header::agent_chat_bar(cx).child(
                        crate::ui::design::header::title_col(cx)
                            .child(match title_edit_input {
                                Some(input) => {
                                    crate::ui::design::header::title_row()
                                        .child(
                                            div().flex_1().min_w(px(0.)).child(
                                                Input::new(&input)
                                                    .small()
                                                    .h(crate::ui::design::control_h()),
                                            ),
                                        )
                                        .child(
                                            Button::new((
                                                "cancel-agent-title-edit",
                                                agent.id.as_u128() as u64,
                                            ))
                                            .ghost()
                                            .xsmall()
                                            .compact()
                                            .h(crate::ui::design::control_h_xs())
                                            .icon(IconName::Close)
                                            .tooltip("Cancel")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.cancel_agent_title_edit(agent_id, cx);
                                            })),
                                        )
                                        .child(
                                            Button::new((
                                                "save-agent-title-edit",
                                                agent.id.as_u128() as u64,
                                            ))
                                            .ghost()
                                            .xsmall()
                                            .compact()
                                            .h(crate::ui::design::control_h_xs())
                                            .icon(IconName::Check)
                                            .tooltip("Save")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.save_agent_title_edit(agent_id, cx);
                                            })),
                                        )
                                        .into_any_element()
                                }
                                None => crate::ui::design::header::title_row()
                                    .id(("agent-title-hover", agent.id.as_u128() as u64))
                                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                                        this.hovered_agent_title =
                                            if *hovered { Some(agent_id) } else { None };
                                        cx.notify();
                                    }))
                                    // A Solo wears just the fork by its name —
                                    // branch and actions live in the lane card
                                    // above the composer.
                                    .when(solo_branch.is_some(), |row| {
                                        row.child(crate::ui::design::indicator::solo_icon(
                                            crate::ui::design::sky(cx),
                                            crate::ui::design::icon_sm(),
                                        ))
                                    })
                                    .child(
                                        crate::ui::design::header::title(
                                            SharedString::from(agent.title.clone()),
                                            cx,
                                        )
                                        .text_size(crate::ui::design::text_body())
                                        .max_w(crate::ui::design::element_title_edit_max_w())
                                        .min_w(px(0.)),
                                    )
                                    .when(title_hovered, |row| {
                                        row.child(
                                            Button::new((
                                                "rename-agent-title",
                                                agent.id.as_u128() as u64,
                                            ))
                                            .ghost()
                                            .xsmall()
                                            .compact()
                                            .h(crate::ui::design::control_h_xs())
                                            .w(crate::ui::design::control_h_xs())
                                            .tooltip("Rename")
                                            .child(
                                                svg()
                                                    .path("agent-icons/pencil.svg")
                                                    .size(crate::ui::design::icon_ind())
                                                    .text_color(crate::ui::design::t3(cx)),
                                            )
                                            .on_click({
                                                let current_title = agent.title.clone();
                                                cx.listener(move |this, _, window, cx| {
                                                    cx.stop_propagation();
                                                    this.start_agent_title_edit(
                                                        agent_id,
                                                        current_title.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                })
                                            }),
                                        )
                                    })
                                    .into_any_element(),
                            })
                            .when(has_header_metadata, |column| {
                                column.child(
                                    crate::ui::design::header::subline()
                                        .relative()
                                        .child(crate::ui::onboarding::target_marker(
                                            crate::ui::onboarding::SpotlightTarget::AgentContext,
                                            cx,
                                        ))
                                        .children(linked_tasks.iter().enumerate().map(
                                            |(index, task)| {
                                                let task = task.clone();
                                                let label =
                                                    SharedString::from(task.issue_key.clone());
                                                crate::ui::design::indicator::subline_link(
                                                    ("agent-linked-task", index),
                                                    crate::ui::design::tasks_icon(),
                                                    label,
                                                    crate::ui::design::accent(cx),
                                                    cx,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_task(project, task.clone(), cx);
                                                }))
                                                .into_any_element()
                                            },
                                        ))
                                        .children(linked_design_indicators)
                                        .when_some(agent_ship_pr.as_ref(), |row, pr| {
                                            row.child(self.render_agent_ship_pr_indicator(pr, cx))
                                        })
                                        .children(linked_docs.iter().enumerate().map(
                                            |(index, relative)| {
                                                let label = SharedString::from(
                                                    short_doc_chip_label(relative),
                                                );
                                                let absolute = project_path.join(relative);
                                                crate::ui::design::indicator::subline_link(
                                                    ("agent-linked-doc", index),
                                                    crate::ui::design::docs_icon(),
                                                    label,
                                                    crate::ui::design::sky(cx),
                                                    cx,
                                                )
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.open_doc(project, absolute.clone(), cx);
                                                }))
                                                .into_any_element()
                                            },
                                        ))
                                        .when(
                                            crate::ui::onboarding::shows_connected_context_message(
                                                project, cx,
                                            ),
                                            |row| {
                                                row.child(
                                                    div()
                                                        .ml_1()
                                                        .text_size(
                                                            crate::ui::design::text_label(),
                                                        )
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child(
                                                            "Everything stays in context—connected and never lost.",
                                                        ),
                                                )
                                            },
                                        ),
                                )
                            }),
                    ),
                )
                .child(
                    crate::ui::design::header::agent_chat_actions_overlay()
                            .child(
                                crate::ui::style::agent_status_dropdown_button(
                                    ("agent-status", agent.id.as_u128() as u64),
                                    cx,
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_1p5()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(gpui::FontWeight::NORMAL)
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(status_icon(agent.status, agent_status_accent))
                                        .child(
                                            div()
                                                .line_height(gpui::relative(1.))
                                                .child(agent.status.label()),
                                        )
                                        .child(
                                            gpui_component::Icon::new(IconName::ChevronDown)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::t4(cx)),
                                        ),
                                )
                                .dropdown_menu(
                                    move |mut menu, window, _| {
                                        for status in AgentStatus::ALL {
                                            menu = menu.item(
                                                PopupMenuItem::element(move |_, cx| {
                                                    status_menu_row(status, cx)
                                                })
                                                .checked(status == current_status)
                                                .on_click(window.listener_for(
                                                    &status_view,
                                                    move |this: &mut Self, _, window, cx| {
                                                        this.agents.update(cx, |agents, cx| {
                                                            agents.update_status(
                                                                agent_id, status, cx,
                                                            );
                                                        });
                                                        // Rejecting a Solo is
                                                        // the abandon gesture —
                                                        // offer to pack the
                                                        // lane up with it.
                                                        if status == AgentStatus::Rejected {
                                                            this.confirm_discard_solo_lane(
                                                                agent_id, window, cx,
                                                            );
                                                        }
                                                    },
                                                )),
                                            );
                                        }
                                        menu
                                    },
                                ),
                            ),
                    ),
            )
            .when_some(
                self.agent_start_errors.get(&agent.id).cloned(),
                |view, error| {
                    let copy_error = error.clone();
                    let readable_error = error.replace('/', "/\u{200b}");
                    view.child(
                        h_flex()
                            .w_full()
                            .px_3()
                            .py_2()
                            .gap_2()
                            .items_start()
                            .border_b_1()
                            .border_color(crate::ui::design::rose(cx).opacity(0.2))
                            .bg(crate::ui::design::rose(cx).opacity(0.08))
                            .child(
                                gpui_component::Icon::new(IconName::TriangleAlert)
                                    .size(crate::ui::design::icon())
                                    .text_color(crate::ui::design::rose(cx)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .whitespace_normal()
                                    .text_size(crate::ui::design::text_ui())
                                    .line_height(gpui::relative(1.45))
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(readable_error),
                            )
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("copy-agent-error", agent.id.as_u128() as u64),
                                    "Copy error",
                                )
                                .icon(IconName::Copy)
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_error.clone(),
                                    ));
                                }),
                            ),
                    )
                },
            )
            .when_some(
                self.lane_setups.get(&agent.id).cloned(),
                |view, lane_setup| {
                    view.child(self.render_lane_setup_strip(agent_id, &lane_setup, cx))
                },
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h(px(0.))
                    .child(
                        div()
                            .size_full()
                            .pb(crate::ui::design::header_h())
                            .child(detail_body),
                    )
                    .when(detail_tab == AgentDetailTab::Notes, |area| {
                        area.child(self.render_agent_notes_drawer(
                            &agent,
                            notes_input,
                            window,
                            cx,
                        ))
                    })
                    .when(detail_tab == AgentDetailTab::Files, |area| {
                        area.child(self.render_agent_files_drawer(project, &agent, cx))
                    })
                    .when_some(diff_drawer, |area, drawer| {
                        area.child(self.render_agent_diff_drawer(&agent, &drawer, cx))
                    })
                    .when_some(plan_drawer, |area, plan| {
                        area.child(self.render_agent_plan_drawer(&agent, &plan, window, cx))
                    })
                    .child(self.render_agent_detail_switch_footer(&agent, detail_tab, cx)),
            )
            .into_any_element()
    }
}
