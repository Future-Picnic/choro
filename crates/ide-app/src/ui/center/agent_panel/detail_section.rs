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
        let mut linked_design_indicators = Vec::new();
        for design in self.studio_designs_for_agent(&agent) {
            linked_design_indicators.push(self.render_linked_studio_indicator("agent-linked-studio", agent.project_id, design, cx));
        }
        // Deliberately the project root, not `runtime_path()`: linked docs are
        // project-level documents, so a Solo agent's chips open the project's
        // copy rather than a shadow inside its lane.
        let project_path = agent.project_path.clone();
        let solo_branch = agent.solo_branch.clone();
        let pocketcomet_task = agent.origin.as_ref().and_then(pocketcomet_task_link);
        let is_from_pocketcomet = pocketcomet_task.is_some();
        let has_header_metadata = pocketcomet_task.is_some()
            || !linked_tasks.is_empty()
            || agent_ship_pr.is_some()
            || !linked_docs.is_empty()
            || !linked_design_indicators.is_empty();
        let agent_status_accent = status_accent(agent.status, cx);
        let expert_status = self.render_expert_chat_status(&agent, cx);
        let band_header_toggle = self.render_band_header_toggle(&agent, cx);
        let bandmate_index = self.expert_bandmate_index(&agent, cx);
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
                                    .when(is_from_pocketcomet, |row| {
                                        row.child(
                                            crate::ui::design::indicator::pocketcomet_icon(
                                                crate::ui::design::accent(cx),
                                                crate::ui::design::icon_sm(),
                                            ),
                                        )
                                    })
                                    .when(solo_branch.is_some(), |row| {
                                        row.child(crate::ui::design::indicator::solo_icon(
                                            crate::ui::design::sky(cx),
                                            crate::ui::design::icon_sm(),
                                        ))
                                    })
                                    .when_some(bandmate_index, |row, index| {
                                        row.child(crate::ui::design::indicator::bandmate_icon(
                                            index,
                                            crate::ui::design::amber(cx),
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
                            .when_some(agent.expert_snapshot.as_ref(), |column, snapshot| {
                                // Profile identity survives beta-toggle changes and comes from
                                // this chat's immutable snapshot, not the editable catalog.
                                column.child(h_flex().min_w(px(0.)).child(
                                    crate::ui::style::bandmate_profile_chip(snapshot.profile.name.clone(), cx),
                                ))
                            })
                            .when(has_header_metadata, |column| {
                                column.child(
                                    crate::ui::design::header::subline()
                                        .relative()
                                        .child(crate::ui::onboarding::target_marker(
                                            crate::ui::onboarding::SpotlightTarget::AgentContext,
                                            cx,
                                        ))
                                        .when_some(pocketcomet_task.clone(), |row, task| {
                                            let tooltip = SharedString::from(format!(
                                                "Open “{}” in PocketComet",
                                                task.full_title
                                            ));
                                            let url = task.url.clone();
                                            row.child(
                                                crate::ui::design::indicator::subline_link_with_icon(
                                                    ("agent-pocketcomet-task", agent_id.as_u128() as u64),
                                                    crate::ui::design::indicator::pocketcomet_icon(
                                                        crate::ui::design::accent(cx),
                                                        crate::ui::design::icon_ind(),
                                                    ),
                                                    SharedString::from(task.label),
                                                    cx,
                                                )
                                                .tooltip(move |window, cx| {
                                                    Tooltip::new(tooltip.clone()).build(window, cx)
                                                })
                                                .on_click(move |_, _, _| {
                                                    crate::ui::git::git_panel::open_url(&url)
                                                }),
                                            )
                                        })
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
                    )
                    .child(crate::ui::design::header::actions().map(|actions| {
                        let actions = actions.children(band_header_toggle);
                        if let Some(status) = expert_status {
                            return actions.child(status);
                        }
                        actions.child(
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
                            )
                        }),
                    ),
                ),
            )
            .when_some(
                self.agent_start_errors.get(&agent.id).cloned(),
                |view, error| {
                    view.child(
                        crate::ui::style::agent_attention_strip(
                            ("copy-agent-error", agent.id.as_u128() as u64),
                            error,
                            cx,
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

const POCKETCOMET_TASK_LABEL_CHARS: usize = 32;

#[derive(Clone)]
struct PocketCometTaskLink {
    label: String,
    full_title: String,
    url: String,
}

fn pocketcomet_task_link(origin: &AgentOrigin) -> Option<PocketCometTaskLink> {
    let AgentOrigin::PocketComet {
        workspace_id,
        project_id,
        task_id,
        task_title,
    } = origin
    else {
        return None;
    };
    let full_title = task_title.trim();
    let mut url = url::Url::parse("pocketcomet://task").ok()?;
    url.path_segments_mut().ok()?.push(task_id);
    url.query_pairs_mut()
        .append_pair("workspace_id", workspace_id)
        .append_pair("project_id", project_id);
    Some(PocketCometTaskLink {
        label: compact_pocketcomet_task_label(full_title),
        full_title: if full_title.is_empty() {
            "PocketComet task".to_string()
        } else {
            full_title.to_string()
        },
        url: url.into(),
    })
}

fn compact_pocketcomet_task_label(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return "PocketComet task".to_string();
    }
    if title.chars().count() <= POCKETCOMET_TASK_LABEL_CHARS {
        return title.to_string();
    }
    let mut label = title
        .chars()
        .take(POCKETCOMET_TASK_LABEL_CHARS - 1)
        .collect::<String>();
    label.push('…');
    label
}

#[cfg(test)]
mod pocketcomet_task_link_tests {
    use super::*;

    #[test]
    fn task_link_uses_a_compact_title_and_encoded_identity() {
        let origin = AgentOrigin::PocketComet {
            workspace_id: "workspace one".into(),
            project_id: "project/one".into(),
            task_id: "task/one".into(),
            task_title: "Implement the extremely long PocketComet task link".into(),
        };

        let link = pocketcomet_task_link(&origin).unwrap();
        assert_eq!(link.label.chars().count(), POCKETCOMET_TASK_LABEL_CHARS);
        assert!(link.label.ends_with('…'));
        assert_eq!(
            link.full_title,
            "Implement the extremely long PocketComet task link"
        );
        let url = url::Url::parse(&link.url).unwrap();
        assert_eq!(url.host_str(), Some("task"));
        assert_eq!(url.path(), "/task%2Fone");
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "workspace_id")
                .unwrap()
                .1,
            "workspace one"
        );
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "project_id")
                .unwrap()
                .1,
            "project/one"
        );
    }
}
