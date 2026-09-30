#![allow(dead_code, reason = "retained task-detail presentation helpers")]

use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn start_personal_task_title_edit(
        &mut self,
        task_id: Uuid,
        current_title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Task title")
                .default_value(current_title)
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.task_title_edit = Some(TaskTitleEdit { task_id, input });
        self.hovered_task_title = false;
        cx.notify();
    }

    pub(in crate::ui::center) fn cancel_personal_task_title_edit(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        if self.task_title_edit.is_some() {
            self.task_title_edit = None;
            cx.notify();
        }
    }

    pub(in crate::ui::center) fn save_personal_task_title_edit(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) {
        let Some((task_id, title)) = self
            .task_title_edit
            .as_ref()
            .map(|edit| (edit.task_id, edit.input.read(cx).value().trim().to_string()))
        else {
            return;
        };
        if !title.is_empty() {
            if let Some(mut task) = self.tasks.read(cx).load_personal_task(project, task_id) {
                task.title = title;
                self.tasks
                    .update(cx, |tasks, cx| tasks.save_personal_task(task, cx));
            }
        }
        self.task_title_edit = None;
        cx.notify();
    }

    pub(in crate::ui::center) fn render_task_detail(
        &mut self,
        project: ProjectId,
        summary: Option<TaskSummary>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(summary) = summary else {
            return style::empty_state(
                IconName::CircleCheck,
                "Select a task",
                "Choose a task from the board to read it.",
                cx,
            )
            .into_any_element();
        };
        let reference = summary.reference.clone();
        let detail = self.tasks.read(cx).detail(&reference);
        let detail_error = self.tasks.read(cx).detail_error(&reference);
        let detail_loading = self.tasks.read(cx).detail_loading(&reference);
        let implementation_agents = self
            .agents
            .read(cx)
            .task_implementors(project, &summary.reference);
        let (_, active_pr) = self.linked_agent_and_pr(project, &summary.reference, cx);
        let issue_url = summary.reference.issue_url.clone();
        let task_element_id = task_element_id(&summary.reference);
        let personal_task = (summary.reference.provider
            == ide_core::IssueTrackerProvider::Personal)
            .then(|| Uuid::parse_str(&summary.reference.issue_id).ok())
            .flatten()
            .and_then(|id| self.tasks.read(cx).load_personal_task(project, id));

        let is_personal = summary.reference.provider == ide_core::IssueTrackerProvider::Personal;
        let is_pocketcomet =
            summary.reference.provider == ide_core::IssueTrackerProvider::PocketComet;
        let status_color = crate::ui::tasks_panel::task_status_color(&summary, cx);
        let crumb_label = if is_personal {
            "Personal".to_string()
        } else if is_pocketcomet {
            "PocketComet".to_string()
        } else {
            summary.reference.issue_key.clone()
        };
        let implement_action =
            self.render_task_implement_action(project, &summary, detail.as_ref(), cx);
        let design_action = {
            let creating = self.studio_creating.contains(&project);
            let design_summary = summary.clone();
            let design_detail = detail.clone();
            style::accent_button_compact(("task-design", task_element_id), if creating { "Creating…" } else { "Design" }, cx)
                .icon(crate::ui::design::design_icon()).disabled(creating)
                .tooltip("Create a Studio design from this task")
                .on_click(cx.listener(move |this, _, _, cx| this.create_studio_for_task(project, design_summary.clone(), design_detail.clone(), cx)))
        };
        let status_control = self.render_task_status_control(
            project,
            &summary.reference,
            is_personal,
            &personal_task,
            summary.status.clone(),
            status_color,
            task_element_id,
            cx,
        );

        // --- Canonical element header: identity and reciprocal links on the
        // left, mutable state and actions on the right. ---
        let title = summary.reference.title.clone();
        let meta = self.task_meta_items(
            project,
            &summary,
            personal_task.as_ref(),
            crumb_label,
            is_personal,
            implementation_agents,
            active_pr,
            cx,
        );
        let has_meta = !meta.is_empty();
        // Personal task titles rename inline — the same hover-pencil mechanic
        // as agent titles. External tracker tasks stay read-only here.
        let title_edit_input = self
            .task_title_edit
            .as_ref()
            .filter(|edit| {
                personal_task
                    .as_ref()
                    .is_some_and(|task| task.id == edit.task_id)
            })
            .map(|edit| edit.input.clone());
        let title_hovered = self.hovered_task_title;
        let personal_title = personal_task.as_ref().map(|task| task.title.clone());
        let title_element: gpui::AnyElement = match title_edit_input {
            Some(input) => crate::ui::design::header::title_row()
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.))
                        .child(Input::new(&input).small().h(crate::ui::design::control_h())),
                )
                .child(
                    crate::ui::style::header_icon_button(
                        ("cancel-task-title-edit", task_element_id),
                        IconName::Close,
                        cx,
                    )
                    .tooltip("Cancel")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.cancel_personal_task_title_edit(cx);
                    })),
                )
                .child(
                    crate::ui::style::header_icon_button(
                        ("save-task-title-edit", task_element_id),
                        IconName::Check,
                        cx,
                    )
                    .tooltip("Save")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.save_personal_task_title_edit(project, cx);
                    })),
                )
                .into_any_element(),
            None => crate::ui::design::header::title_row()
                .id(("task-title-hover", task_element_id))
                .on_hover(cx.listener(move |this, hovered, _, cx| {
                    this.hovered_task_title = *hovered;
                    cx.notify();
                }))
                .child(
                    crate::ui::design::header::title(SharedString::from(title), cx)
                        .max_w(crate::ui::design::element_title_edit_max_w())
                        .min_w(px(0.)),
                )
                .when(
                    is_personal && title_hovered && personal_title.is_some(),
                    |row| {
                        let current_title = personal_title.clone().unwrap_or_default();
                        let rename_task_id = personal_task.as_ref().map(|task| task.id);
                        row.child(
                            crate::ui::style::header_svg_button(
                                ("rename-task-title", task_element_id),
                                svg()
                                    .path("agent-icons/pencil.svg")
                                    .size(crate::ui::design::icon_ind())
                                    .text_color(crate::ui::design::t3(cx)),
                                cx,
                            )
                            .tooltip("Rename")
                            .on_click(cx.listener(
                                move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    if let Some(task_id) = rename_task_id {
                                        this.start_personal_task_title_edit(
                                            task_id,
                                            current_title.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                },
                            )),
                        )
                    },
                )
                .into_any_element(),
        };
        let top = crate::ui::design::header::bar(cx)
            .child(
                crate::ui::design::header::title_col(cx)
                    .child(title_element)
                    .when(has_meta, move |col| {
                        col.child(crate::ui::design::header::subline().children(meta))
                    }),
            )
            .child(
                crate::ui::design::header::actions()
                    .child(status_control)
                    .when(!is_personal && !issue_url.is_empty(), |actions| {
                        actions.child(
                            style::header_icon_button(
                                ("task-open-external", task_element_id),
                                IconName::ExternalLink,
                                cx,
                            )
                            .tooltip(if is_pocketcomet {
                                "Open in PocketComet"
                            } else {
                                "Open in browser"
                            })
                            .on_click(move |_, _, _| {
                                crate::ui::git::git_panel::open_url(&issue_url);
                            }),
                        )
                    })
                    .child(
                        style::refresh_icon_button(("task-detail-refresh", task_element_id), cx)
                            .tooltip(if detail_loading {
                                "Refreshing…"
                            } else {
                                "Refresh"
                            })
                            .disabled(detail_loading)
                            .on_click({
                                let reference = summary.reference.clone();
                                cx.listener(move |this, _, _, cx| {
                                    this.tasks.update(cx, |tasks, cx| {
                                        tasks.refresh_detail(project, reference.clone(), cx)
                                    });
                                })
                            }),
                    )
                    .when(is_personal, |actions| {
                        let task_id = personal_task.as_ref().map(|task| task.id);
                        actions.child(
                            style::destructive_icon_button(("task-archive", task_element_id), cx)
                                .tooltip("Archive task")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(id) = task_id {
                                        this.tasks.update(cx, |tasks, cx| {
                                            tasks.archive_personal_task(project, id, true, cx);
                                        });
                                    }
                                })),
                        )
                    })
                    .child(design_action)
                    .children(implement_action),
            );

        // --- Description ---
        let task_key = summary.reference.issue_id.clone();
        let desc_expanded = self.task_desc_expanded.contains(&task_key);
        let description_text = detail
            .as_ref()
            .map(|detail| detail.description.text.trim().to_string())
            .filter(|text| !text.is_empty());

        // Personal tasks are a live doc: edit inline. External tasks show rendered
        // markdown capped at a preview length with Show more, so comments stay in
        // reach without scrolling past a wall of description.
        let description_section: gpui::AnyElement = if let Some(task) = personal_task.clone() {
            let editor = self.ensure_personal_editor(task, cx);
            v_flex()
                .gap_2()
                .child(section_title("Description", cx))
                .child(div().w_full().min_h(px(160.)).child(editor))
                .into_any_element()
        } else if detail.as_ref().is_some_and(|detail| {
            detail
                .description
                .blocks
                .iter()
                .any(|block| matches!(block, ide_core::TaskContentBlock::Image(_)))
        }) {
            // The description embeds inline images (mockups/screenshots). Render
            // the rich blocks so they actually display in place, instead of the
            // flattened "[image: …]" placeholder that the plain-text path emits.
            v_flex()
                .gap_2()
                .child(section_title("Description", cx))
                .child(render_rich_text_block(
                    detail.as_ref().map(|detail| &detail.description),
                    "No description.",
                    "task-detail-description",
                    cx,
                ))
                .into_any_element()
        } else {
            let body: gpui::AnyElement = match description_text {
                Some(text) => {
                    let is_long = text.lines().count() > TASK_DESC_PREVIEW_LINES;
                    let shown = if is_long && !desc_expanded {
                        truncate_text_lines(&text, TASK_DESC_PREVIEW_LINES)
                    } else {
                        text
                    };
                    let rendered = div().text_color(style::focus_text(cx)).child(
                        render_chat_message_markdown(&shown, task_element_id, window, cx),
                    );
                    if is_long {
                        let key = task_key.clone();
                        v_flex()
                            .gap_1()
                            .child(rendered)
                            .child(
                                div()
                                    .id(("task-desc-toggle", task_element_id))
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::accent(cx))
                                    .cursor_pointer()
                                    .child(if desc_expanded {
                                        "Show less"
                                    } else {
                                        "Show more"
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if !this.task_desc_expanded.remove(&key) {
                                            this.task_desc_expanded.insert(key.clone());
                                        }
                                        cx.notify();
                                    })),
                            )
                            .into_any_element()
                    } else {
                        rendered.into_any_element()
                    }
                }
                None => div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child("No description.")
                    .into_any_element(),
            };
            v_flex()
                .gap_2()
                .child(section_title("Description", cx))
                .child(body)
                .into_any_element()
        };
        let images = (!is_personal).then(|| self.render_task_images(detail.as_ref(), cx));
        let comments = (!is_personal).then(|| {
            let comment_input = self.ensure_task_comment_input(task_element_id, window, cx);
            self.render_task_comments(
                project,
                &reference,
                task_element_id,
                comment_input,
                detail.as_ref(),
                cx,
            )
        });

        let scroll = v_flex()
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scrollbar()
            .child(
                h_flex().w_full().justify_center().child(
                    v_flex()
                        .w_full()
                        .max_w(crate::ui::design::center_content_frame_max_w())
                        .gap_4()
                        .px(crate::ui::design::agent_chat_gutter_x())
                        .py_5()
                        .when_some(detail_error, |body, error| {
                            body.child(
                                div()
                                    .rounded(px(style::RADIUS))
                                    .border_1()
                                    .border_color(crate::ui::design::rose(cx).opacity(0.4))
                                    .bg(crate::ui::design::rose(cx).opacity(0.08))
                                    .p_3()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(SharedString::from(error)),
                            )
                        })
                        .when(!is_personal && detail_loading && detail.is_none(), |body| {
                            body.child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Loading issue detail..."),
                            )
                        })
                        .child(description_section)
                        .children(images)
                        .children(comments),
                ),
            );

        v_flex()
            .size_full()
            .child(top)
            .child(scroll)
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_linked_agent_chip(
        &self,
        agent: AgentRecord,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        h_flex()
            .id(("task-linked-agent-chip", agent_id.as_u128() as u64))
            .items_center()
            .gap_2()
            .rounded(px(style::RADIUS))
            .border_1()
            .border_color(crate::ui::design::accent(cx).opacity(0.28))
            .bg(crate::ui::design::accent(cx).opacity(0.08))
            .px_3()
            .py_2()
            .cursor_pointer()
            .hover(|chip| chip.bg(crate::ui::design::hover(cx).opacity(0.56)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_agent(agent_id, window, cx);
            }))
            .child(
                Icon::new(IconName::Bot)
                    .size(crate::ui::design::icon())
                    .text_color(crate::ui::design::accent(cx)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Linked agent"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .truncate()
                            .text_color(crate::ui::design::t1(cx))
                            .child(SharedString::from(agent.title)),
                    ),
            )
            .child(
                style::secondary_button_compact(
                    ("task-linked-agent-open", agent_id.as_u128() as u64),
                    "Open",
                )
                .icon(IconName::Bot)
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_agent(agent_id, window, cx);
                })),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_task_description(
        &self,
        detail: Option<&TaskDetail>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .gap_2()
            .child(section_title("Description", cx))
            .child(render_rich_text_block(
                detail.map(|detail| &detail.description),
                "No description.",
                "task-description",
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_task_images(
        &self,
        detail: Option<&TaskDetail>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(detail) = detail else {
            return div().into_any_element();
        };
        let used_attachment_ids = rich_text_image_attachment_ids(detail);
        let images = detail
            .attachments
            .iter()
            .filter(|attachment| {
                attachment.is_image() && !used_attachment_ids.contains(&attachment.id)
            })
            .collect::<Vec<_>>();
        if images.is_empty() {
            return div().into_any_element();
        }

        v_flex()
            .gap_2()
            .child(section_title("Images", cx))
            .children(images.into_iter().enumerate().map(|(index, attachment)| {
                let image = ide_core::TaskInlineImage {
                    attachment_id: Some(attachment.id.clone()),
                    media_id: None,
                    filename: Some(attachment.filename.clone()),
                    alt: Some(attachment.filename.clone()),
                    mime_type: attachment.mime_type.clone(),
                    local_path: attachment.local_path.clone(),
                    content_url: attachment.content_url.clone(),
                };
                render_task_image_block(&image, "task-attachment-image", index, cx)
            }))
            .into_any_element()
    }

    /// The header's one metadata line: identity, type, assignee, priority, then
    /// the reciprocal agent/PR links. Each item is a glyph carrying the category
    /// plus its bare value — the category never spends a word ("Issue", not
    /// "Type Issue"). Values the tracker spells as a null are dropped, so a task
    /// with nothing to say returns no items and the header renders no row.
    fn task_meta_items(
        &mut self,
        project: ProjectId,
        summary: &TaskSummary,
        personal_task: Option<&ide_core::PersonalTaskRecord>,
        crumb_label: String,
        is_personal: bool,
        implementation_agents: Vec<AgentRecord>,
        active_pr: Option<crate::ui::git::git_panel::BranchPullRequest>,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let mut items: Vec<gpui::AnyElement> = Vec::new();

        items.push(
            crate::ui::design::indicator::subline_indicator_with_icon(
                Icon::empty()
                    .path("icons/target.svg")
                    .size(crate::ui::design::icon_ind())
                    .text_color(crate::ui::design::accent(cx)),
                SharedString::from(crumb_label),
                cx,
            )
            .into_any_element(),
        );

        // A personal task's type ("Personal task") only restates the crumb.
        if !is_personal {
            if let Some(value) = summary
                .issue_type
                .as_ref()
                .map(|value| value.trim())
                .filter(|value| is_meaningful_value(value))
            {
                items.push(
                    crate::ui::design::indicator::subline_indicator_with_icon(
                        crate::ui::design::indicator::lucide_icon(
                            task_type_icon(value),
                            crate::ui::design::sky(cx),
                            crate::ui::design::icon_ind(),
                        ),
                        SharedString::from(value.to_string()),
                        cx,
                    )
                    .into_any_element(),
                );
            }
        }

        if let Some(value) = summary
            .assignee
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| is_meaningful_value(value))
        {
            items.push(
                crate::ui::design::indicator::subline_indicator_with_icon(
                    avatar_badge_sm(value),
                    SharedString::from(value.to_string()),
                    cx,
                )
                .into_any_element(),
            );
        }

        // Personal priority is user-owned, so it stays an inline dropdown; a
        // tracker's priority is read-only here and reads as plain text.
        match personal_task {
            Some(task) => {
                let color = priority_color(task.priority.label(), cx);
                items.push(
                    h_flex()
                        .items_center()
                        .gap_1()
                        .child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::Flag,
                            color,
                            crate::ui::design::icon_ind(),
                        ))
                        .child(self.render_personal_task_priority_control(task.clone(), cx))
                        .into_any_element(),
                );
            }
            None => {
                if let Some(value) = summary
                    .priority
                    .as_ref()
                    .map(|value| value.trim())
                    .filter(|value| is_meaningful_value(value))
                {
                    items.push(
                        crate::ui::design::indicator::subline_indicator_with_icon(
                            crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Flag,
                                priority_color(value, cx),
                                crate::ui::design::icon_ind(),
                            ),
                            SharedString::from(value.to_string()),
                            cx,
                        )
                        .into_any_element(),
                    );
                }
            }
        }

        for design in self.studio_designs_for_task(project, &summary.reference) {
            items.push(self.render_linked_studio_indicator("task-linked-studio", project, design, cx));
        }
        if let Some(agent) = implementation_agents.first() {
            let agent_id = agent.id;
            let short_id = agent_id
                .simple()
                .to_string()
                .chars()
                .take(6)
                .collect::<String>();
            let label = SharedString::from(format!("Agent {short_id}"));
            let tooltip = SharedString::from(format!("{} — open", agent.title));
            let color = crate::ui::agent_status_style::implement_status_color(agent.status, cx);
            if implementation_agents.len() == 1 {
                items.push(
                    crate::ui::design::indicator::subline_link(
                        ("task-linked-agent", agent_id.as_u128() as u64),
                        IconName::Bot,
                        label,
                        color,
                        cx,
                    )
                    .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_agent(agent_id, window, cx);
                    }))
                    .into_any_element(),
                );
            } else {
                let center = cx.entity().clone();
                let history = implementation_agents.clone();
                items.push(
                    style::header_dropdown_button(
                        ("task-linked-agent-history", agent_id.as_u128() as u64),
                        cx,
                    )
                    .p_0()
                    .child(crate::ui::design::indicator::subline_indicator(
                        IconName::Bot,
                        label,
                        color,
                        cx,
                    ))
                    .tooltip("Implementation agents")
                    .dropdown_menu(move |mut menu, window, _| {
                        for (index, implementation) in history.iter().cloned().enumerate() {
                            let implementation_id = implementation.id;
                            menu = menu.item(
                                PopupMenuItem::element(move |_, cx| {
                                    super::indicators::task_implementation_agent_history_row(
                                        &implementation,
                                        index == 0,
                                        cx,
                                    )
                                })
                                .checked(index == 0)
                                .on_click(window.listener_for(
                                    &center,
                                    move |this: &mut Self, _, window, cx| {
                                        this.open_agent(implementation_id, window, cx);
                                    },
                                )),
                            );
                        }
                        menu
                    })
                    .into_any_element(),
                );
            }
        }

        if let Some(pr) = active_pr {
            let (_, color) = crate::ui::git::git_panel::pull_request_status_style(&pr, cx);
            let url = pr.url.clone();
            items.push(
                crate::ui::design::indicator::subline_link_with_icon(
                    ("task-linked-pr", pr.number as usize),
                    Icon::empty()
                        .path("icons/branch.svg")
                        .size(crate::ui::design::icon_ind())
                        .text_color(color),
                    SharedString::from(format!("#{}", pr.number)),
                    cx,
                )
                .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url))
                .into_any_element(),
            );
        }

        items
    }
}
