use super::*;

impl CenterArea {
    pub(super) fn render_agent_chat_context_picker(
        &mut self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if let Some(picker) = self.render_agent_chat_slash_picker(agent, input.clone(), window, cx)
        {
            return Some(picker);
        }
        if let Some(view) = self.active_agent_chat_project_mention_view(agent, cx) {
            return Some(self.render_agent_chat_project_picker(agent, input, &view, cx));
        }
        if let Some(view) = self.active_agent_chat_agent_mention_view(agent, cx) {
            return Some(self.render_agent_chat_agent_picker(agent, input, &view, cx));
        }
        if let Some(view) = self.active_agent_chat_doc_mention_view(agent.id, agent.project_id, cx)
        {
            return Some(self.render_agent_chat_doc_mention_picker(agent, input, &view, cx));
        }
        let view = self.active_agent_chat_file_mention_view(agent, cx)?;
        Some(self.render_agent_chat_file_mention_picker(agent, input, &view, cx))
    }

    pub(super) fn render_agent_chat_project_picker(
        &self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        view: &ComposerProjectMentionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let content = if view.matches.is_empty() {
            vec![h_flex()
                .w_full()
                .h(px(COMPOSER_PICKER_ROW_H))
                .px_2()
                .gap_1p5()
                .items_center()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(
                    gpui_component::Icon::new(IconName::FolderOpen)
                        .size(crate::ui::design::icon_md()),
                )
                .child(if view.mention.query.is_empty() {
                    "No other projects in Choro".to_string()
                } else {
                    format!("No projects matching {}", view.mention.query)
                })
                .into_any_element()]
        } else {
            view.matches
                .iter()
                .enumerate()
                .map(|(index, project)| {
                    let selected = index == view.selected;
                    let project_for_click = project.clone();
                    let mention_for_click = view.mention.clone();
                    let input_for_click = input.clone();
                    let agent_id = agent.id;
                    h_flex()
                        .id(("agent-chat-project-mention-row", index))
                        .w_full()
                        .min_w(px(0.))
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .gap_1p5()
                        .items_center()
                        .px_2()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .bg(if selected {
                            crate::ui::design::surface_2(cx)
                        } else {
                            gpui::transparent_black()
                        })
                        .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.insert_agent_chat_project_mention(
                                agent_id,
                                input_for_click.clone(),
                                project_for_click.clone(),
                                mention_for_click.clone(),
                                window,
                                cx,
                            );
                        }))
                        .child(
                            gpui_component::Icon::new(IconName::FolderOpen)
                                .size(crate::ui::design::icon_md())
                                .text_color(if selected {
                                    crate::ui::design::rose(cx)
                                } else {
                                    crate::ui::design::t3(cx)
                                }),
                        )
                        .child(
                            div()
                                .max_w(px(190.))
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t1(cx))
                                .child(project.name.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(project.path.to_string_lossy().to_string()),
                        )
                        .child(
                            div()
                                .flex_none()
                                .rounded(crate::ui::design::r_sm())
                                .bg(crate::ui::design::rose(cx).opacity(0.12))
                                .px_1p5()
                                .py_0p5()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::rose(cx))
                                .child("Project"),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        };

        v_flex()
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .children(content)
            .into_any_element()
    }

    pub(super) fn render_agent_chat_agent_picker(
        &self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        view: &ComposerAgentMentionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let content = if view.matches.is_empty() {
            vec![div()
                .px_2()
                .py_1p5()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(if view.mention.query.is_empty() {
                    "No other agents in this project".to_string()
                } else {
                    format!("No agents matching {}", view.mention.query)
                })
                .into_any_element()]
        } else {
            view.matches
                .iter()
                .enumerate()
                .map(|(index, target)| {
                    let selected = index == view.selected;
                    let target_for_click = target.clone();
                    let mention_for_click = view.mention.clone();
                    let input_for_click = input.clone();
                    let source_agent_id = agent.id;
                    h_flex()
                        .id(("agent-chat-agent-mention-row", index))
                        .w_full()
                        .min_w(px(0.))
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .gap_2()
                        .items_center()
                        .px_2()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .bg(if selected {
                            crate::ui::design::surface_2(cx)
                        } else {
                            gpui::transparent_black()
                        })
                        .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.insert_agent_chat_agent_target(
                                source_agent_id,
                                input_for_click.clone(),
                                target_for_click.clone(),
                                mention_for_click.clone(),
                                window,
                                cx,
                            );
                        }))
                        .child(crate::ui::agent_status_style::status_dot(
                            target.status,
                            6.,
                            cx,
                        ))
                        .child(
                            div()
                                .max_w(px(180.))
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child(target.title.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t4(cx))
                                .child(format!(
                                    "{} · {} · {}",
                                    target.status.label(),
                                    target.project_name,
                                    if target.active { "Active" } else { "Inactive" }
                                )),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        };

        v_flex()
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .children(content)
            .into_any_element()
    }

    pub(super) fn render_agent_chat_slash_picker(
        &mut self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        // Share the exact command list and selection with keyboard handling.
        // Rebuilding from cached provider skills alone hides Choro delegation.
        let AgentChatSlashView {
            query,
            matches,
            selected,
        } = self.active_agent_chat_slash_view(agent, cx)?;
        self.agent_chat_slash_selection.insert(agent.id, selected);

        let empty_row = |title: &'static str,
                         detail: Option<String>,
                         cx: &mut Context<Self>|
         -> gpui::AnyElement {
            v_flex()
                .gap_1()
                .px_2()
                .py_1()
                .text_size(crate::ui::design::text_ui())
                .text_color(crate::ui::design::t3(cx))
                .child(title)
                .when_some(detail, |row, detail| {
                    row.child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .truncate()
                            .child(detail),
                    )
                })
                .into_any_element()
        };

        let content = if matches.is_empty() {
            vec![empty_row("No matching commands", None, cx)]
        } else {
            matches
                .iter()
                .enumerate()
                .map(|(index, command)| {
                    let command_for_click = command.clone();
                    let query_for_click = query.clone();
                    let input_for_click = input.clone();
                    let agent_id = agent.id;
                    let selected = index == selected;
                    let is_riff = command.is_choro_riff();
                    let is_orbit = command.is_orbit();
                    let is_preview = command.is_choro_preview();
                    let detail = command
                        .description
                        .as_ref()
                        .filter(|description| !description.trim().is_empty())
                        .cloned()
                        .unwrap_or_else(|| {
                            if is_orbit {
                                "Added to this project".to_string()
                            } else if is_riff {
                                "Available in every project".to_string()
                            } else {
                                command.invocation.trim().to_string()
                            }
                        });
                    let icon = if is_preview {
                        crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::MonitorPlay,
                            crate::ui::design::accent(cx),
                            crate::ui::design::icon_md(),
                        )
                        .into_any_element()
                    } else if is_orbit {
                        Icon::new(IconName::Network)
                            .size(crate::ui::design::icon_md())
                            .text_color(crate::ui::design::accent(cx))
                            .into_any_element()
                    } else if is_riff {
                        crate::ui::style::choro_riff_icon(
                            crate::ui::design::icon_md(),
                            crate::ui::design::accent(cx),
                        )
                    } else {
                        gpui_component::Icon::new(IconName::Asterisk)
                            .size(crate::ui::design::icon_md())
                            .text_color(if selected {
                                crate::ui::design::accent(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .into_any_element()
                    };
                    h_flex()
                        .id(("agent-chat-slash-command", index))
                        .w_full()
                        .min_w(px(0.))
                        .h(px(COMPOSER_PICKER_ROW_H))
                        .gap_1p5()
                        .items_center()
                        .px_2()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .bg(if selected {
                            crate::ui::design::surface_2(cx)
                        } else {
                            gpui::transparent_black()
                        })
                        .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.insert_agent_chat_slash_command(
                                agent_id,
                                input_for_click.clone(),
                                command_for_click.clone(),
                                query_for_click.clone(),
                                window,
                                cx,
                            );
                        }))
                        .child(icon)
                        .child(
                            div()
                                .max_w(px(190.))
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::style::focus_text(cx))
                                .child(command.title.clone()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(detail),
                        )
                        .child(
                            div()
                                .rounded(crate::ui::design::r_sm())
                                .bg(if is_preview {
                                    crate::ui::design::accent_soft(cx)
                                } else {
                                    crate::ui::design::base(cx).opacity(0.42)
                                })
                                .px_1p5()
                                .py_0p5()
                                .text_size(crate::ui::design::text_label())
                                .text_color(if is_preview {
                                    crate::ui::design::accent(cx)
                                } else {
                                    crate::ui::design::t3(cx)
                                })
                                .child(command.source.label()),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>()
        };

        Some(
            v_flex()
                .w_full()
                .max_h(px(COMPOSER_PICKER_MAX_H))
                .overflow_hidden()
                .rounded(crate::ui::design::r_md())
                .border_1()
                .border_color(crate::ui::design::line(cx).opacity(0.42))
                .bg(crate::ui::design::focus(cx))
                .shadow_lg()
                .p_1()
                .gap_0p5()
                .children(content)
                .into_any_element(),
        )
    }

    pub(super) fn render_agent_chat_selected_command_token(
        &self,
        agent_id: Uuid,
        input: Entity<InputState>,
        command: &AgentCapability,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let command_for_remove = command.clone();
        let is_riff = command.is_choro_riff();
        let is_orbit = command.is_orbit();
        let is_product_capability = is_riff || is_orbit;
        let foreground = if is_product_capability {
            crate::ui::design::accent(cx)
        } else {
            crate::ui::design::t2(cx)
        };
        let icon = if is_orbit {
            Icon::new(IconName::Network)
                .size(crate::ui::design::icon_md())
                .text_color(foreground)
                .into_any_element()
        } else if is_riff {
            crate::ui::style::choro_riff_icon(crate::ui::design::icon_md(), foreground)
        } else {
            gpui_component::Icon::new(IconName::Asterisk)
                .size(crate::ui::design::icon_md())
                .text_color(foreground)
                .into_any_element()
        };
        h_flex()
            .id((
                "agent-chat-selected-command-token",
                agent_id.as_u128() as u64,
            ))
            .min_w(px(0.))
            .items_center()
            .gap_1()
            .h(crate::ui::design::control_h_xs())
            .px_1p5()
            .py_0p5()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(if is_product_capability {
                crate::ui::design::accent(cx).opacity(0.34)
            } else {
                crate::ui::design::line_2(cx)
            })
            .bg(if is_product_capability {
                crate::ui::design::accent_soft(cx)
            } else {
                crate::ui::design::surface_2(cx)
            })
            .cursor_pointer()
            .hover(|chip| {
                chip.bg(if is_product_capability {
                    crate::ui::design::accent(cx).opacity(0.2)
                } else {
                    crate::ui::design::hover(cx)
                })
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.agent_chat_selected_commands.remove(&agent_id);
                let current = input.read(cx).value().to_string();
                let (next, cursor) =
                    remove_agent_chat_command_invocation(&current, &command_for_remove);
                if next != current {
                    input.update(cx, |input, cx| {
                        input.set_value(next.clone(), window, cx);
                        input.set_cursor_position(
                            input_position_for_byte_offset(&next, cursor),
                            window,
                            cx,
                        );
                        input.focus(window, cx);
                    });
                }
                cx.notify();
            }))
            .child(icon)
            .child(
                div()
                    .max_w(px(170.))
                    .truncate()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(foreground)
                    .child(command.title.clone()),
            )
            .child(
                gpui_component::Icon::new(IconName::Close)
                    .size(crate::ui::design::icon_sm())
                    .text_color(foreground.opacity(0.72)),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_chat_input_prefix(
        &self,
        agent_id: Uuid,
        input: Entity<InputState>,
        command: Option<&AgentCapability>,
        mentions: &[ComposerMentionToken],
        target: Option<&AgentRecord>,
        preview_armed: bool,
        preview_suggested: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .min_w(px(0.))
            .gap_1()
            .items_center()
            .overflow_hidden()
            .when(preview_armed, |row| {
                row.child(
                    crate::ui::style::preview_attachment_chip(
                        ("agent-chat-preview-token", agent_id.as_u128() as u64),
                        cx,
                    )
                    .tooltip("Remove Choro Preview from this message")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agent_chat_preview_armed.remove(&agent_id);
                        this.agent_chat_preview_suggestion_dismissed
                            .insert(agent_id);
                        cx.notify();
                    })),
                )
            })
            .when(preview_suggested, |row| {
                row.child(
                    crate::ui::style::composer_toggle_chip(
                        ("agent-chat-use-preview", agent_id.as_u128() as u64),
                        "Use Choro Preview?",
                        Some(
                            crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::MonitorPlay,
                                crate::ui::design::accent(cx),
                                crate::ui::design::icon_sm(),
                            )
                            .into_any_element(),
                        ),
                        false,
                        cx,
                    )
                    .tooltip("Route this request to Choro's project Preview")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agent_chat_preview_armed.insert(agent_id);
                        this.agent_chat_preview_suggestion_dismissed
                            .remove(&agent_id);
                        cx.notify();
                    })),
                )
                .child(
                    crate::ui::style::composer_icon_action(
                        ("agent-chat-dismiss-preview", agent_id.as_u128() as u64),
                        gpui_component::Icon::new(IconName::Close)
                            .size(crate::ui::design::icon_sm()),
                        cx,
                    )
                    .tooltip("Don't use Preview for this message")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agent_chat_preview_suggestion_dismissed
                            .insert(agent_id);
                        cx.notify();
                    })),
                )
            })
            .when_some(command, |row, command| {
                row.child(self.render_agent_chat_selected_command_token(
                    agent_id,
                    input.clone(),
                    command,
                    cx,
                ))
            })
            .when_some(target, |row, target| {
                let source_agent_id = agent_id;
                let target_id = target.id;
                let request_kind = self
                    .agent_chat_agent_request_kind_overrides
                    .get(&source_agent_id)
                    .copied()
                    .unwrap_or_else(|| classify_agent_request(&input.read(cx).value()));
                let request_icon = match request_kind {
                    AgentRequestKind::Ask => lucide_icons::Icon::CircleHelp,
                    AgentRequestKind::Delegate => lucide_icons::Icon::ListTodo,
                };
                let request_color = match request_kind {
                    AgentRequestKind::Ask => crate::ui::design::sky(cx),
                    AgentRequestKind::Delegate => crate::ui::design::amber(cx),
                };
                row.child(
                    h_flex()
                        .id((
                            "agent-chat-selected-agent-target",
                            target_id.as_u128() as u64,
                        ))
                        .min_w(px(0.))
                        .items_center()
                        .gap_1()
                        .h(crate::ui::design::control_h_xs())
                        .px_1p5()
                        .py_0p5()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::sky(cx).opacity(0.34))
                        .bg(crate::ui::design::sky(cx).opacity(0.12))
                        .cursor_pointer()
                        .hover(|chip| chip.bg(crate::ui::design::sky(cx).opacity(0.18)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.agent_chat_selected_agent_targets
                                .remove(&source_agent_id);
                            this.agent_chat_agent_request_kind_overrides
                                .remove(&source_agent_id);
                            this.agent_handoff_preparations_pending
                                .remove(&source_agent_id);
                            cx.notify();
                        }))
                        .child(
                            gpui_component::Icon::new(IconName::Bot)
                                .size(crate::ui::design::icon_md())
                                .text_color(crate::ui::design::sky(cx)),
                        )
                        .child(
                            div()
                                .max_w(px(140.))
                                .truncate()
                                .text_size(crate::ui::design::text_head())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::sky(cx))
                                .child(format!("#{}", target.title)),
                        )
                        .child(
                            gpui_component::Icon::new(IconName::Close)
                                .size(crate::ui::design::icon_sm())
                                .text_color(crate::ui::design::sky(cx).opacity(0.72)),
                        ),
                )
                .child(
                    crate::ui::style::composer_toggle_chip(
                        (
                            "agent-chat-agent-request-kind",
                            source_agent_id.as_u128() as u64,
                        ),
                        request_kind.display_label(),
                        Some(
                            crate::ui::design::indicator::lucide_icon(
                                request_icon,
                                request_color,
                                crate::ui::design::icon_sm(),
                            )
                            .into_any_element(),
                        ),
                        false,
                        cx,
                    )
                    .tooltip("Inferred from your text · click to switch Question / Task")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agent_chat_agent_request_kind_overrides
                            .insert(source_agent_id, request_kind.toggled());
                        this.agent_handoff_preparations_pending
                            .remove(&source_agent_id);
                        cx.notify();
                    })),
                )
            })
            .children(mentions.iter().enumerate().map(|(index, mention)| {
                self.render_agent_chat_selected_mention_token(
                    agent_id,
                    input.clone(),
                    index,
                    mention,
                    cx,
                )
            }))
            .into_any_element()
    }

    pub(super) fn render_agent_chat_selected_mention_token(
        &self,
        agent_id: Uuid,
        input: Entity<InputState>,
        index: usize,
        mention: &ComposerMentionToken,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (icon, color) = match mention.kind {
            ComposerMentionKind::Doc => {
                (crate::ui::design::docs_icon(), crate::ui::design::amber(cx))
            }
            ComposerMentionKind::File => (IconName::File, crate::ui::design::sage(cx)),
            ComposerMentionKind::Folder => (IconName::FolderOpen, crate::ui::design::sage(cx)),
            ComposerMentionKind::StudioDesign => (
                crate::ui::design::design_icon(),
                crate::ui::design::accent(cx),
            ),
            ComposerMentionKind::Project => (IconName::FolderOpen, crate::ui::design::rose(cx)),
        };
        let label = if mention.kind == ComposerMentionKind::Project {
            format!("##{}", mention.chip_label())
        } else {
            mention.chip_label().to_string()
        };
        h_flex()
            .id((
                "agent-chat-selected-mention-token",
                (agent_id.as_u128() as u64).wrapping_add(index as u64),
            ))
            .min_w(px(0.))
            .items_center()
            .gap_1()
            .h(crate::ui::design::control_h_xs())
            .px_1p5()
            .py_0p5()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(color.opacity(0.3))
            .bg(color.opacity(0.12))
            .cursor_pointer()
            .hover(move |chip| chip.bg(color.opacity(0.18)))
            .on_click(cx.listener({
                let mention_for_remove = mention.clone();
                move |this, _, window, cx| {
                    if let Some(mentions) = this.agent_chat_selected_mentions.get_mut(&agent_id) {
                        if index < mentions.len() {
                            mentions.remove(index);
                        }
                        if mentions.is_empty() {
                            this.agent_chat_selected_mentions.remove(&agent_id);
                        }
                    }
                    input.update(cx, |input, cx| {
                        let current = input.value().to_string();
                        let (next, cursor) =
                            remove_composer_mention_invocation(&current, &mention_for_remove);
                        input.set_value(next.clone(), window, cx);
                        input.set_cursor_position(
                            input_position_for_byte_offset(&next, cursor),
                            window,
                            cx,
                        );
                        input.focus(window, cx);
                    });
                    cx.notify();
                }
            }))
            .child(
                gpui_component::Icon::new(icon)
                    .size(crate::ui::design::icon_md())
                    .text_color(color),
            )
            .child(
                div()
                    .max_w(px(140.))
                    .truncate()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(label),
            )
            .child(
                gpui_component::Icon::new(IconName::Close)
                    .size(crate::ui::design::icon_sm())
                    .text_color(color.opacity(0.72)),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_chat_doc_mention_picker(
        &self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        view: &ComposerDocMentionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let ComposerDocMentionView {
            mention,
            matches,
            designs,
            selected,
        } = view;
        let selected = *selected;
        let design_count = designs.len();

        v_flex()
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .when(matches.is_empty() && designs.is_empty(), |picker| {
                picker.child(
                    h_flex()
                        .w_full()
                        .h(crate::ui::design::control_h())
                        .px_2()
                        .gap_1p5()
                        .items_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(
                            gpui_component::Icon::new(crate::ui::design::docs_icon())
                                .size(crate::ui::design::icon_md()),
                        )
                        .child(if mention.query.is_empty() {
                            "No docs or assets in this project".to_string()
                        } else {
                            format!("Nothing matching {}", mention.query)
                        }),
                )
            })
            .children(designs.iter().enumerate().map(|(design_index, reference)| {
                let index = design_index;
                let is_active = index == selected;
                let kind = reference.kind;
                let is_native_design = project_reference_is_native_design(reference);
                let accent = if is_native_design {
                    crate::ui::design::accent(cx)
                } else {
                    crate::ui::designs_panel::design_kind_color(kind, cx)
                };
                let title = SharedString::from(reference.title.clone());
                let source = SharedString::from(reference.source.clone());
                let mention = mention.clone();
                let input_for_click = input.clone();
                let reference_for_click = reference.clone();
                let agent_id = agent.id;
                h_flex()
                    .id(("agent-chat-design-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_agent_chat_design_mention(
                            agent_id,
                            input_for_click.clone(),
                            reference_for_click.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .child(if is_native_design {
                        gpui_component::Icon::new(crate::ui::design::design_icon())
                            .size(crate::ui::design::icon_md())
                            .text_color(accent)
                            .into_any_element()
                    } else {
                        crate::ui::designs_panel::design_kind_glyph(
                            kind,
                            crate::ui::design::icon_md(),
                            accent,
                        )
                    })
                    .child(
                        div()
                            .max_w(px(180.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::style::focus_text(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(source),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(accent.opacity(0.14))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(accent)
                            .child(if is_native_design {
                                "Design"
                            } else {
                                crate::ui::designs_panel::design_kind_label(kind)
                            }),
                    )
                    .into_any_element()
            }))
            .children(matches.iter().enumerate().map(|(index, doc)| {
                let relative = doc.relative_path.clone();
                let mention = mention.clone();
                let input_for_click = input.clone();
                let agent_id = agent.id;
                let is_active = design_count + index == selected;
                h_flex()
                    .id(("agent-chat-doc-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_agent_chat_doc_mention(
                            agent_id,
                            input_for_click.clone(),
                            relative.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .child(
                        gpui_component::Icon::new(crate::ui::design::docs_icon())
                            .size(crate::ui::design::icon_md())
                            .text_color(if is_active {
                                crate::ui::design::amber(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            }),
                    )
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::style::focus_text(cx))
                            .child(doc.title.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(doc.relative_path.to_string_lossy().to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::base(cx).opacity(0.42))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Doc"),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }

    pub(super) fn render_agent_chat_file_mention_picker(
        &self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        view: &ComposerFileMentionView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let ComposerFileMentionView {
            mention,
            matches,
            selected,
            loading,
        } = view;
        let selected = *selected;

        v_flex()
            .w_full()
            .max_h(px(COMPOSER_PICKER_MAX_H))
            .overflow_hidden()
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow_lg()
            .p_1()
            .gap_0p5()
            .when(matches.is_empty(), |picker| {
                picker.child(
                    h_flex()
                        .w_full()
                        .h(crate::ui::design::control_h())
                        .px_2()
                        .gap_1p5()
                        .items_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child(if *loading {
                            gpui_component::spinner::Spinner::new()
                                .xsmall()
                                .into_any_element()
                        } else {
                            gpui_component::Icon::new(IconName::File)
                                .size(crate::ui::design::icon_md())
                                .into_any_element()
                        })
                        .child(if *loading {
                            "Indexing project files and folders…".to_string()
                        } else if mention.query.is_empty() {
                            "No files or folders in this project".to_string()
                        } else {
                            format!("No files or folders matching {}", mention.query)
                        }),
                )
            })
            .children(matches.iter().enumerate().map(|(index, file)| {
                let file_for_click = file.clone();
                let mention = mention.clone();
                let input_for_click = input.clone();
                let agent_id = agent.id;
                let is_active = index == selected;
                h_flex()
                    .id(("agent-chat-file-mention-row", index))
                    .w_full()
                    .min_w(px(0.))
                    .h(px(COMPOSER_PICKER_ROW_H))
                    .gap_1p5()
                    .items_center()
                    .px_2()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if is_active {
                        crate::ui::design::surface_2(cx)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.46)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.insert_agent_chat_file_mention(
                            agent_id,
                            input_for_click.clone(),
                            file_for_click.clone(),
                            mention.clone(),
                            window,
                            cx,
                        );
                    }))
                    .child(
                        gpui_component::Icon::new(if file.is_directory {
                            IconName::FolderOpen
                        } else {
                            IconName::File
                        })
                        .size(crate::ui::design::icon_md())
                        .text_color(if is_active {
                            crate::ui::design::sage(cx)
                        } else {
                            crate::ui::design::t3(cx)
                        }),
                    )
                    .child(
                        div()
                            .max_w(px(190.))
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::style::focus_text(cx))
                            .child(file.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(file.relative_label.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .rounded(crate::ui::design::r_sm())
                            .bg(crate::ui::design::base(cx).opacity(0.42))
                            .px_1p5()
                            .py_0p5()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx))
                            .child(if file.is_directory { "Folder" } else { "File" }),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }
}
