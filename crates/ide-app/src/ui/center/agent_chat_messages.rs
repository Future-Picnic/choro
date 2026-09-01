use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
enum SavedMessageTagNavigation {
    Doc(PathBuf),
    File(PathBuf),
}

fn saved_message_tag_navigation(tag: &AgentChatMessageTag) -> Option<SavedMessageTagNavigation> {
    let detail = tag.detail.as_deref()?.trim();
    if detail.is_empty() {
        return None;
    }
    match tag.kind {
        AgentChatMessageTagKind::Doc => Some(SavedMessageTagNavigation::Doc(PathBuf::from(detail))),
        AgentChatMessageTagKind::File => {
            Some(SavedMessageTagNavigation::File(PathBuf::from(detail)))
        }
        _ => None,
    }
}

fn project_tag_target_path(project_root: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        target.to_path_buf()
    } else {
        project_root.join(target)
    }
}

fn render_saved_message_tag(
    tag: &AgentChatMessageTag,
    id: u64,
    project: ProjectId,
    cx: &mut Context<CenterArea>,
) -> gpui::AnyElement {
    let (color, icon, kind_label) = match tag.kind {
        AgentChatMessageTagKind::Preview => (
            crate::ui::design::accent(cx),
            crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::MonitorPlay,
                crate::ui::design::accent(cx),
                crate::ui::design::icon_sm(),
            )
            .into_any_element(),
            "Preview",
        ),
        AgentChatMessageTagKind::Orbit => (
            crate::ui::design::accent(cx),
            Icon::new(IconName::Network)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::accent(cx))
                .into_any_element(),
            "Orbit",
        ),
        AgentChatMessageTagKind::Riff => (
            crate::ui::design::accent(cx),
            crate::ui::style::choro_riff_icon(
                crate::ui::design::icon_sm(),
                crate::ui::design::accent(cx),
            ),
            "Riff",
        ),
        AgentChatMessageTagKind::Skill => (
            crate::ui::design::t2(cx),
            gpui_component::Icon::new(IconName::Asterisk)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t2(cx))
                .into_any_element(),
            "Skill",
        ),
        AgentChatMessageTagKind::Command => (
            crate::ui::design::t2(cx),
            gpui_component::Icon::new(IconName::SquareTerminal)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t2(cx))
                .into_any_element(),
            "Command",
        ),
        AgentChatMessageTagKind::File => (
            crate::ui::design::sage(cx),
            gpui_component::Icon::new(IconName::File)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::sage(cx))
                .into_any_element(),
            "File",
        ),
        AgentChatMessageTagKind::Folder => (
            crate::ui::design::sage(cx),
            gpui_component::Icon::new(IconName::FolderOpen)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::sage(cx))
                .into_any_element(),
            "Folder",
        ),
        AgentChatMessageTagKind::Doc => (
            crate::ui::design::amber(cx),
            gpui_component::Icon::new(crate::ui::design::docs_icon())
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::amber(cx))
                .into_any_element(),
            "Doc",
        ),
        AgentChatMessageTagKind::Design => (
            crate::ui::design::accent(cx),
            gpui_component::Icon::new(crate::ui::design::design_icon())
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::accent(cx))
                .into_any_element(),
            "Design",
        ),
        AgentChatMessageTagKind::Project => (
            crate::ui::design::rose(cx),
            gpui_component::Icon::new(IconName::FolderOpen)
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::rose(cx))
                .into_any_element(),
            "Project",
        ),
        AgentChatMessageTagKind::Brain => (
            crate::ui::design::sage(cx),
            crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::Brain,
                crate::ui::design::sage(cx),
                crate::ui::design::icon_sm(),
            )
            .into_any_element(),
            "Choro Brain",
        ),
        AgentChatMessageTagKind::LegacyVisual => (
            crate::ui::design::t3(cx),
            crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::Eye,
                crate::ui::design::t3(cx),
                crate::ui::design::icon_sm(),
            )
            .into_any_element(),
            "Visual review",
        ),
    };
    let display_label = if tag.kind == AgentChatMessageTagKind::LegacyVisual {
        "Visual review".to_string()
    } else {
        tag.label.clone()
    };
    let navigation = saved_message_tag_navigation(tag);
    let tooltip = SharedString::from(match tag.detail.as_deref() {
        Some(detail) if !detail.trim().is_empty() && navigation.is_some() => {
            format!("Open {kind_label} · {detail}")
        }
        Some(detail) if !detail.trim().is_empty() => format!("{kind_label} · {detail}"),
        _ => kind_label.to_string(),
    });

    h_flex()
        .id(("agent-chat-saved-message-tag", id))
        .flex_none()
        .min_w(px(0.))
        .max_w(px(190.))
        .h(crate::ui::design::control_h_xs())
        .items_center()
        .gap_1()
        .rounded(crate::ui::design::r_sm())
        .border_1()
        .border_color(color.opacity(0.3))
        .bg(color.opacity(0.11))
        .px_1p5()
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .when_some(navigation, |chip, navigation| {
            chip.cursor_pointer()
                .hover(|chip| {
                    chip.border_color(color.opacity(0.52))
                        .bg(color.opacity(0.17))
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    let Some((_, project_root)) = this.project_by_id(project, cx) else {
                        return;
                    };
                    match &navigation {
                        SavedMessageTagNavigation::Doc(target) => {
                            let path = project_tag_target_path(&project_root, target);
                            this.open_doc(project, path, cx);
                        }
                        SavedMessageTagNavigation::File(target) => {
                            let path = project_tag_target_path(&project_root, target);
                            this.open_file(project, path, window, cx);
                        }
                    }
                }))
        })
        .child(icon)
        .child(
            div()
                .min_w(px(0.))
                .truncate()
                .text_size(crate::ui::design::text_ui())
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(display_label),
        )
        .into_any_element()
}

impl CenterArea {
    /// The queued-turn strip that sits *above* the composer while a run is in
    /// flight. Each pending turn is a quiet borderless line whose Steer / Edit /
    /// Remove actions reveal on hover. Beyond two, the tail collapses behind a
    /// "Show N more" toggle so a long queue never crowds the input.
    pub(super) fn render_agent_chat_queue(
        &self,
        agent_id: Uuid,
        turns: &[QueuedChatTurn],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        const COLLAPSED_LIMIT: usize = 2;
        let expanded = self.agent_chat_queue_expanded.contains(&agent_id);
        let overflowing = turns.len() > COLLAPSED_LIMIT;
        let visible = if expanded || !overflowing {
            turns.len()
        } else {
            COLLAPSED_LIMIT
        };
        let hidden = turns.len().saturating_sub(visible);

        v_flex()
            .w_full()
            .min_w(px(0.))
            .max_w(crate::ui::design::agent_chat_content_max_w())
            .mx_auto()
            .pb_1p5()
            .gap_0p5()
            .children(
                turns
                    .iter()
                    .take(visible)
                    .map(|turn| self.render_agent_queued_turn(agent_id, turn, cx)),
            )
            .when(overflowing, |col| {
                col.child(
                    Button::new(("agent-chat-queue-toggle", agent_id.as_u128() as u64))
                        .ghost()
                        .xsmall()
                        .compact()
                        .h(crate::ui::design::control_h_xs())
                        .label(if expanded {
                            "Show less".to_string()
                        } else {
                            format!("Show {hidden} more")
                        })
                        .dropdown_caret(true)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if !this.agent_chat_queue_expanded.remove(&agent_id) {
                                this.agent_chat_queue_expanded.insert(agent_id);
                            }
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    fn render_agent_queued_turn(
        &self,
        agent_id: Uuid,
        turn: &QueuedChatTurn,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let turn_id = turn.id;
        let turn_key = turn_id.as_u128() as u64;
        let group_name = SharedString::from(format!("agent-queued-{turn_key}"));
        let preview = turn
            .display_text
            .as_deref()
            .unwrap_or_else(|| visible_agent_chat_submission_text(&turn.text))
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(160)
            .collect::<String>();

        let dim = crate::ui::design::t3(cx);
        let hover_bg = crate::ui::design::hover(cx);
        let action_size = crate::ui::design::control_h_xs();

        h_flex()
            .id(("agent-chat-queued-turn", turn_key))
            .group(group_name.clone())
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap_2()
            .px_1p5()
            .py_1()
            .rounded(crate::ui::design::r_sm())
            .hover(|row| row.bg(hover_bg.opacity(0.5)))
            .child(crate::ui::design::indicator::lucide_icon(
                lucide_icons::Icon::Clock,
                dim.opacity(0.7),
                crate::ui::design::icon_sm(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(dim)
                    .child(preview),
            )
            .child(
                h_flex()
                    .flex_none()
                    .items_center()
                    .gap_0p5()
                    .invisible()
                    .group_hover(group_name.clone(), |actions| actions.visible())
                    .child(
                        Button::new(("agent-chat-steer-queued", turn_key))
                            .ghost()
                            .xsmall()
                            .compact()
                            .h(action_size)
                            .icon(IconName::Redo2)
                            .label("Steer")
                            .tooltip("Send this into the current run now")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agent_chats.update(cx, |chats, cx| {
                                    chats.steer_queued_turn(agent_id, turn_id, cx);
                                });
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id(("agent-chat-edit-queued", turn_key))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(action_size)
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .hover(|button| button.bg(hover_bg))
                            .tooltip(|window, cx| {
                                Tooltip::new("Edit in composer").build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.edit_queued_turn_into_composer(agent_id, turn_id, window, cx);
                            }))
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::Pencil,
                                dim,
                                crate::ui::design::icon_sm(),
                            )),
                    )
                    .child(
                        div()
                            .id(("agent-chat-remove-queued", turn_key))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(action_size)
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .hover(|button| button.bg(hover_bg))
                            .tooltip(|window, cx| {
                                Tooltip::new("Remove from queue").build(window, cx)
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agent_chats.update(cx, |chats, cx| {
                                    chats.remove_queued_turn(agent_id, turn_id, cx);
                                });
                                cx.notify();
                            }))
                            .child(crate::ui::design::indicator::lucide_icon(
                                lucide_icons::Icon::X,
                                dim,
                                crate::ui::design::icon_sm(),
                            )),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_chat_message(
        &self,
        agent: &AgentRecord,
        index: usize,
        message: &AgentChatMessage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let hovered = self.agent_chat_hovered_message == Some((agent_id, index));
        let render_key = agent_chat_message_render_key(agent_id, index, message);
        match message {
            AgentChatMessage::User {
                text,
                display_text,
                tags,
                created_at,
            } => {
                // Incoming agent requests already have a dedicated timeline
                // card. The provider still needs this hidden user turn, but a
                // second "Question from …" bubble only repeats the card.
                if super::agent_chat_brain::is_agent_request_submission(text) {
                    return div().into_any_element();
                }
                if let Some(label) = super::agent_chat_brain::summary_request_action_label(text) {
                    return self.render_brain_summary_request_action(render_key, label, cx);
                }
                if let Some((label, icon)) = code_review_request_chip(text) {
                    let _ = created_at;
                    return h_flex()
                        .w_full()
                        .justify_end()
                        .child(
                            h_flex()
                                .items_center()
                                .gap_1p5()
                                .rounded_full()
                                .bg(crate::ui::style::surface(cx))
                                .px_3()
                                .py_1p5()
                                .child(
                                    gpui_component::Icon::new(icon)
                                        .size(crate::ui::design::icon_sm())
                                        .text_color(crate::ui::design::t3(cx)),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(label),
                                ),
                        )
                        .into_any_element();
                }
                let raw_visible_text = visible_agent_chat_submission_text(text);
                let (_, attached_files) = split_prompt_attached_files(raw_visible_text);
                let (display_text, _) = split_prompt_attached_files(
                    display_text.as_deref().unwrap_or(raw_visible_text),
                );
                let display_line_count = text_line_count(&display_text);
                let is_long_user_message = display_line_count > USER_MESSAGE_PREVIEW_LINES;
                let user_message_expanded = self
                    .agent_chat_expanded_user_messages
                    .contains(&(agent_id, index));
                let visible_display_text = if is_long_user_message && !user_message_expanded {
                    truncate_text_lines(&display_text, USER_MESSAGE_PREVIEW_LINES)
                } else {
                    display_text.clone()
                };
                v_flex()
                    .id(("agent-user-message-hover", render_key))
                    .w_full()
                    .min_w(px(0.))
                    .items_end()
                    .gap_0p5()
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        this.agent_chat_hovered_message = if *hovered {
                            Some((agent_id, index))
                        } else {
                            None
                        };
                        cx.notify();
                    }))
                    .child(
                        v_flex()
                            .max_w(px(520.))
                            .rounded(px(crate::ui::style::RADIUS_LG))
                            .rounded_br(px(4.))
                            .bg(crate::ui::style::surface(cx))
                            .px_3p5()
                            .py_2()
                            .gap_2()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.35))
                            .text_color(crate::ui::style::focus_text(cx))
                            .when(!tags.is_empty(), |bubble| {
                                bubble.child(
                                    h_flex()
                                        .w_full()
                                        .min_w(px(0.))
                                        .items_center()
                                        .gap_1()
                                        .flex_wrap()
                                        .children(tags.iter().enumerate().map(
                                            |(tag_index, tag)| {
                                                render_saved_message_tag(
                                                    tag,
                                                    render_key
                                                        .wrapping_add(tag_index as u64)
                                                        .wrapping_add(1),
                                                    agent.project_id,
                                                    cx,
                                                )
                                            },
                                        )),
                                )
                            })
                            .when(!attached_files.is_empty(), |bubble| {
                                bubble.child(h_flex().gap_2().flex_wrap().children(
                                    attached_files.iter().enumerate().map(|(file_ix, path)| {
                                        self.render_agent_attachment_preview(
                                            (
                                                "agent-chat-message-attachment",
                                                index * 1000 + file_ix,
                                            ),
                                            path.clone(),
                                            None,
                                            105.,
                                            73.,
                                            cx,
                                        )
                                    }),
                                ))
                            })
                            .when(!visible_display_text.trim().is_empty(), |bubble| {
                                bubble.child(
                                    TextView::markdown(
                                        ("agent-user-message-text", render_key),
                                        visible_display_text,
                                        window,
                                        cx,
                                    )
                                    .selectable(true)
                                    .style(chat_message_text_style()),
                                )
                            })
                            .when(is_long_user_message, |bubble| {
                                bubble.child(
                                    h_flex().w_full().justify_start().child(
                                        Button::new((
                                            "toggle-agent-user-message",
                                            render_key.wrapping_add(17),
                                        ))
                                        .ghost()
                                        .xsmall()
                                        .compact()
                                        .h(crate::ui::design::control_h_xs())
                                        .label(if user_message_expanded {
                                            "Show less"
                                        } else {
                                            "Show more"
                                        })
                                        .dropdown_caret(true)
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                let key = (agent_id, index);
                                                if !this
                                                    .agent_chat_expanded_user_messages
                                                    .remove(&key)
                                                {
                                                    this.agent_chat_expanded_user_messages
                                                        .insert(key);
                                                }
                                                this.remeasure_agent_chat_list(agent_id);
                                                cx.notify();
                                            }),
                                        ),
                                    ),
                                )
                            }),
                    )
                    .child(message_metadata_row(
                        ("copy-agent-user-message", index),
                        display_text,
                        *created_at,
                        true,
                        hovered,
                        cx,
                    ))
                    .into_any_element()
            }
            AgentChatMessage::Assistant {
                message_id,
                text,
                created_at,
            } => {
                let visualization = self.chat_visualization_render_context(agent);
                // While this is the streaming message, paint only the revealed
                // prefix (paced word-by-word) with a warm lavender trailing edge.
                // Finalised and historical messages fall through to the normal
                // markdown path, rendered exactly as before.
                let reveal = self.agent_chat_active_reveal.as_ref().filter(|reveal| {
                    reveal.agent == agent_id && reveal.key == (message_id.clone(), *created_at)
                });
                let body = match reveal {
                    Some(reveal) if reveal.is_animating() => {
                        let revealed_text = if reveal.is_complete() {
                            text.clone()
                        } else {
                            let end = agent_chat_reveal::char_byte_offset(text, reveal.revealed);
                            text[..end].to_string()
                        };
                        let warm_chars = if reveal.warmth > 0.0 {
                            agent_chat_reveal::WARM_CHARS
                        } else {
                            0
                        };
                        render_streaming_agent_chat_message_markdown(
                            &revealed_text,
                            warm_chars,
                            reveal.warmth,
                            &visualization,
                            render_key,
                            window,
                            cx,
                        )
                    }
                    _ => render_agent_chat_message_markdown(
                        text,
                        &visualization,
                        render_key,
                        window,
                        cx,
                    ),
                };
                v_flex()
                    .id(("agent-assistant-message-hover", render_key))
                    .w_full()
                    .min_w(px(0.))
                    .px_1()
                    .py_1()
                    .gap_0p5()
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        this.agent_chat_hovered_message = if *hovered {
                            Some((agent_id, index))
                        } else {
                            None
                        };
                        cx.notify();
                    }))
                    .child(
                        div()
                            // Keep the live and final Markdown paths on the same
                            // geometry. Without an explicit width this wrapper
                            // can fall back to the final table's min-content
                            // width when the paced reveal ends, making a table
                            // visibly shrink after it was already shown full-size.
                            .w_full()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(crate::ui::design::CHAT_PROSE_LINE_HEIGHT))
                            .text_color(crate::ui::design::chat_body(cx))
                            .child(body),
                    )
                    .child(message_metadata_row(
                        ("copy-agent-assistant-message", index),
                        text.clone(),
                        *created_at,
                        false,
                        hovered,
                        cx,
                    ))
                    .into_any_element()
            }
            AgentChatMessage::Thought { text, .. } => {
                let expanded = self
                    .agent_chat_expanded_thoughts
                    .contains(&(agent_id, index));
                v_flex()
                    .w_full()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        h_flex()
                            .id(("agent-chat-thought-toggle", render_key))
                            .gap_1p5()
                            .items_center()
                            .px_1()
                            .py_0p5()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.5)))
                            .child(
                                gpui_component::Icon::new(if expanded {
                                    IconName::ChevronDown
                                } else {
                                    IconName::ChevronRight
                                })
                                .size(crate::ui::design::icon_sm()),
                            )
                            .child(
                                gpui_component::Icon::new(IconName::Bot)
                                    .size(crate::ui::design::icon_sm()),
                            )
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Thinking"),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let key = (agent_id, index);
                                if !this.agent_chat_expanded_thoughts.remove(&key) {
                                    this.agent_chat_expanded_thoughts.insert(key);
                                }
                                this.remeasure_agent_chat_list(agent_id);
                                cx.notify();
                            })),
                    )
                    .when(expanded, |block| {
                        block.child(
                            div()
                                .ml_2()
                                .pl_3()
                                .border_l_2()
                                .border_color(crate::ui::style::border(cx))
                                .text_size(crate::ui::design::text_body())
                                .line_height(gpui::relative(1.45))
                                .text_color(crate::ui::design::t3(cx))
                                .child(text.clone()),
                        )
                    })
                    .into_any_element()
            }
        }
    }

    pub(super) fn agent_chat_list_state(
        &mut self,
        agent_id: Uuid,
        row_count: usize,
        newest_turn_len: usize,
        top_down: bool,
        row_fingerprints: &[u64],
        cx: &mut Context<Self>,
    ) -> ListState {
        let layout_changed = self
            .agent_chat_list_top_down
            .get(&agent_id)
            .is_some_and(|previous| *previous != top_down);
        if layout_changed {
            self.agent_chat_list_states.remove(&agent_id);
            self.agent_chat_row_fingerprints.remove(&agent_id);
            self.agent_chat_scrolled_up.insert(agent_id, false);
            self.agent_chat_prepended_rows.remove(&agent_id);
        }
        self.agent_chat_list_top_down.insert(agent_id, top_down);
        let is_new = !self.agent_chat_list_states.contains_key(&agent_id);
        let state = self
            .agent_chat_list_states
            .entry(agent_id)
            // Match Zed's agent-thread overdraw. A larger measured area around
            // the viewport keeps variable-height markdown rows from popping in
            // during a fast trackpad gesture.
            .or_insert_with(|| {
                let state = ListState::new(
                    0,
                    if top_down {
                        ListAlignment::Top
                    } else {
                        ListAlignment::Bottom
                    },
                    px(2048.),
                );
                Self::splice_agent_chat_list_rows(&state, 0..0, row_count);
                state
            })
            .clone();
        if is_new {
            // Track whether the list is scrolled away from its active edge so
            // the "jump to latest" button can show. Older history lives at the
            // opposite edge in each layout.
            let weak = cx.entity().downgrade();
            state.set_scroll_handler(move |event, _window, app| {
                let scrolled_up = if top_down {
                    event.visible_range.start > 0
                } else {
                    event.visible_range.end < event.count
                };
                let should_load_older = event.is_scrolled
                    && if top_down {
                        event.visible_range.end.saturating_add(5) >= event.count
                    } else {
                        event.visible_range.start <= 5
                    };
                let _ = weak.update(app, |this, cx| {
                    if this
                        .agent_chat_scrolled_up
                        .get(&agent_id)
                        .copied()
                        .unwrap_or(false)
                        != scrolled_up
                    {
                        this.agent_chat_scrolled_up.insert(agent_id, scrolled_up);
                        cx.notify();
                    }
                    if should_load_older {
                        this.load_older_agent_chat_history(agent_id, cx);
                    }
                });
            });
        }
        if let Some(prepended) = self.agent_chat_prepended_rows.remove(&agent_id) {
            if prepended > 0 {
                if top_down {
                    // Chronologically older rows appear after the existing
                    // newest-first list, so loading history extends the bottom.
                    let count = state.item_count();
                    Self::splice_agent_chat_list_rows(&state, count..count, prepended);
                } else {
                    // GPUI shifts its logical item anchor forward when rows are
                    // inserted before it, keeping the same message under the eye.
                    Self::splice_agent_chat_list_rows(&state, 0..0, prepended);
                }
            }
        }
        let old_count = state.item_count();
        let was_scrolled_up = self
            .agent_chat_scrolled_up
            .get(&agent_id)
            .copied()
            .unwrap_or(false);
        match old_count.cmp(&row_count) {
            std::cmp::Ordering::Less => {
                let added = row_count - old_count;
                if top_down {
                    // Preserve prompt -> work -> answer inside the latest turn.
                    // A whole new turn enters at zero; rows that extend the
                    // active turn enter after its already-visible rows.
                    let insertion_index = newest_turn_len
                        .saturating_sub(added)
                        .min(state.item_count());
                    Self::splice_agent_chat_list_rows(
                        &state,
                        insertion_index..insertion_index,
                        added,
                    );
                    if !was_scrolled_up {
                        Self::scroll_agent_chat_list_to_latest(&state, row_count, true);
                    }
                } else {
                    Self::splice_agent_chat_list_rows(&state, old_count..old_count, added);
                    // A user who is already at the latest row should always see a
                    // newly appended outcome, especially "Stopped by user".
                    if !was_scrolled_up {
                        Self::scroll_agent_chat_list_to_latest(&state, row_count, false);
                    }
                }
            }
            std::cmp::Ordering::Greater => {
                if top_down {
                    let removed = old_count - row_count;
                    let removal_index = newest_turn_len.min(row_count);
                    Self::splice_agent_chat_list_rows(
                        &state,
                        removal_index..removal_index.saturating_add(removed),
                        0,
                    );
                } else {
                    Self::splice_agent_chat_list_rows(&state, row_count..old_count, 0);
                }
            }
            std::cmp::Ordering::Equal => {}
        }

        // A row whose content grew in place (a streaming answer, a work-log
        // group gaining steps) never splices, so `ListState` would keep serving
        // the height it measured when that row first appeared. Re-measure only
        // the rows that actually changed — a blanket re-measure would relayout
        // every visible row on every token.
        let previous = self
            .agent_chat_row_fingerprints
            .insert(agent_id, row_fingerprints.to_vec());
        if old_count == row_count && state.item_count() == row_fingerprints.len() {
            if let Some(previous) = previous.filter(|prev| prev.len() == row_fingerprints.len()) {
                let changed = previous
                    .iter()
                    .zip(row_fingerprints)
                    .enumerate()
                    .filter_map(|(index, (before, after))| (before != after).then_some(index))
                    .collect::<Vec<_>>();
                if !changed.is_empty() {
                    // Splicing collapses any scroll anchor inside the spliced
                    // range, which would snap the view; restore it afterwards
                    // exactly as `remeasure_agent_chat_list` does.
                    let anchor = state.logical_scroll_top();
                    for index in changed {
                        state.splice(index..index + 1, 1);
                    }
                    state.scroll_to(anchor);
                }
            }
        }
        state
    }

    /// GPUI's sum-tree leaves use a fixed-capacity `ArrayVec`. Building a long
    /// chat list in one release-mode splice can trap inside that collector.
    /// Keeping each insertion below the leaf capacity avoids the overflow while
    /// preserving the list's normal splice and scroll-anchor behavior.
    fn splice_agent_chat_list_rows(
        state: &ListState,
        old_range: std::ops::Range<usize>,
        count: usize,
    ) {
        const SAFE_CHUNK_SIZE: usize = 16;

        if count == 0 {
            state.splice(old_range, 0);
            return;
        }

        let insertion_start = old_range.start;
        let first_chunk = count.min(SAFE_CHUNK_SIZE);
        state.splice(old_range, first_chunk);

        let mut inserted = first_chunk;
        while inserted < count {
            let chunk = (count - inserted).min(SAFE_CHUNK_SIZE);
            let insertion_index = insertion_start + inserted;
            state.splice(insertion_index..insertion_index, chunk);
            inserted += chunk;
        }
    }

    pub(super) fn scroll_agent_chat_list_to_latest(
        state: &ListState,
        row_count: usize,
        top_down: bool,
    ) {
        if row_count == 0 {
            return;
        }
        if top_down {
            state.scroll_to(gpui::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
        } else {
            state.reset(0);
            Self::splice_agent_chat_list_rows(state, 0..0, row_count);
        }
    }

    /// A floating "jump to latest" affordance shown over the chat list when it
    /// is scrolled up. Clicking it scrolls to the last row.
    pub(super) fn render_agent_chat_scroll_to_latest(
        &self,
        agent_id: Uuid,
        list_state: ListState,
        row_count: usize,
        top_down: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .absolute()
            .when(top_down, |button| button.top(px(10.)))
            .when(!top_down, |button| button.bottom(px(10.)))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id(("chat-scroll-latest", agent_id.as_u128() as u64))
                    .size(px(30.))
                    .rounded_full()
                    .bg(crate::ui::design::focus(cx))
                    .border_1()
                    .border_color(crate::ui::style::border(cx))
                    .shadow_lg()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(|style| style.bg(crate::ui::design::surface(cx)))
                    .child(
                        gpui_component::Icon::new(if top_down {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(crate::ui::design::icon())
                        .text_color(crate::ui::design::t3(cx)),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        Self::scroll_agent_chat_list_to_latest(&list_state, row_count, top_down);
                        this.agent_chat_scrolled_up.insert(agent_id, false);
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    /// Re-measure the chat list's items in place after an inline expand/collapse,
    /// without dropping the `ListState`.
    ///
    /// Splicing the full `0..count` range marks every row unmeasured so the toggled
    /// row is re-laid-out — but it also *resets* the scroll anchor to the top
    /// (`splice_focusable` collapses any anchor inside `old_range` to `{0, 0}`),
    /// which snaps the view to the start of the conversation. To keep the user where
    /// they are, we capture the current anchor first and restore it afterwards.
    pub(super) fn remeasure_agent_chat_list(&self, agent_id: Uuid) {
        if let Some(state) = self.agent_chat_list_states.get(&agent_id) {
            let anchor = state.logical_scroll_top();
            let count = state.item_count();
            state.splice(0..count, count);
            state.scroll_to(anchor);
        }
    }

    pub(super) fn render_agent_chat_list_row(
        &mut self,
        agent: &AgentRecord,
        session: &AgentChatSession,
        row_index: usize,
        row: Option<AgentChatRow>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(row) = row else {
            return div().into_any_element();
        };

        let content = match row {
            AgentChatRow::Message(index) => session
                .messages
                .get(index)
                .map(|message| self.render_agent_chat_message(agent, index, message, window, cx))
                .unwrap_or_else(|| div().into_any_element()),
            AgentChatRow::TimelineItem(index) => session
                .timeline
                .get(index)
                .map(|item| {
                    self.render_agent_chat_timeline_item(agent, row_index, index, item, window, cx)
                })
                .unwrap_or_else(|| div().into_any_element()),
            AgentChatRow::ActivityGroup { start, end } => {
                let activity = session.timeline.get(start..end).unwrap_or_default();
                self.render_agent_work_log_group(agent, start, activity, cx)
            }
            AgentChatRow::ResumeSavedSession => self.render_agent_resume_saved_session(agent, cx),
            AgentChatRow::Activity => self.render_agent_activity_indicator(session, cx),
        };

        self.wrap_agent_chat_row(content)
    }

    /// Apply the canonical agent-timeline geometry around rendered message
    /// content. Lightweight conversations reuse this instead of approximating
    /// the agent chat's gutters, measure, and row rhythm.
    pub(crate) fn wrap_agent_chat_row(&self, content: gpui::AnyElement) -> gpui::AnyElement {
        div()
            .w_full()
            .min_w(px(0.))
            .px(crate::ui::design::agent_chat_gutter_x())
            .pb_2()
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.))
                    .max_w(crate::ui::design::agent_chat_content_max_w())
                    .mx_auto()
                    .child(content),
            )
            .into_any_element()
    }

    /// Render through the real agent message path and wrap it as a normal
    /// timeline row. This keeps Markdown, lists, code, tables, metadata, and
    /// hover behavior identical on every conversation surface.
    pub(crate) fn render_agent_chat_message_row(
        &self,
        agent: &AgentRecord,
        index: usize,
        message: &AgentChatMessage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let content = self.render_agent_chat_message(agent, index, message, window, cx);
        self.wrap_agent_chat_row(content)
    }

    pub(super) fn render_agent_chat_timeline_item(
        &self,
        agent: &AgentRecord,
        row_index: usize,
        index: usize,
        item: &AgentChatTimelineItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match item {
            AgentChatTimelineItem::Message(message) => {
                self.render_agent_chat_message(agent, index, message, window, cx)
            }
            AgentChatTimelineItem::WorkLog(entry) => {
                self.render_agent_work_log_entry(agent.id, index, entry, cx)
            }
            AgentChatTimelineItem::FileChangeActivity(activity) => {
                self.render_file_change_activity(activity, cx)
            }
            AgentChatTimelineItem::PendingUserInput(_) => div().into_any_element(),
            AgentChatTimelineItem::ProposedPlan(plan) => {
                self.render_proposed_plan_card(agent.id, row_index, plan, window, cx)
            }
            AgentChatTimelineItem::CodeReview(review) => {
                self.render_code_review_card(agent.id, review, window, cx)
            }
            AgentChatTimelineItem::Verification(verification) => {
                self.render_verification_card(agent.id, verification, window, cx)
            }
            AgentChatTimelineItem::ReviewChecklist(checklist) => {
                self.render_review_checklist_card(agent.id, checklist, cx)
            }
            AgentChatTimelineItem::ChangedFiles(summary) => {
                self.render_changed_files_card(agent, index, summary, window, cx)
            }
            AgentChatTimelineItem::ShipResult(result) => {
                self.render_ship_result_card(agent, result, window, cx)
            }
            AgentChatTimelineItem::Rejoined(card) => self.render_rejoined_card(agent, card, cx),
            AgentChatTimelineItem::RejoinConflict(card) => {
                self.render_rejoin_conflict_card(agent.id, card, cx)
            }
            AgentChatTimelineItem::Memorized(card) => {
                self.render_memorized_card(agent.id, card, cx)
            }
            AgentChatTimelineItem::MemoryProposal(card) => {
                self.render_memory_proposal_card(agent.id, card, cx)
            }
            AgentChatTimelineItem::OrbitUpdate(card) => {
                self.render_orbit_update_card(agent.id, card, cx)
            }
            AgentChatTimelineItem::AgentSummary(card) => {
                self.render_agent_summary_card(agent.id, card, window, cx)
            }
            AgentChatTimelineItem::AgentMessage(card) => {
                self.render_agent_message_card(card, window, cx)
            }
        }
    }

    fn render_orbit_update_card(
        &self,
        agent_id: Uuid,
        card: &OrbitUpdateCard,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let invocation_id = card.invocation_id;
        let module_id = card.module_id;
        let changes = [
            (card.inserted, "added"),
            (card.updated, "updated"),
            (card.deleted, "removed"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, label)| format!("{count} {label}"))
        .collect::<Vec<_>>()
        .join(" · ");
        crate::ui::style::chat_card(cx)
            .child(
                crate::ui::style::chat_card_head(cx)
                    .child(
                        Icon::new(IconName::Network)
                            .size(crate::ui::design::icon_sm())
                            .text_color(crate::ui::design::accent(cx)),
                    )
                    .child(if card.undone {
                        "Orbit update undone"
                    } else {
                        "Orbit updated"
                    })
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::ghost_button_compact(
                            ("orbit-update-open", invocation_id.as_u128() as u64),
                            "Open Orbit",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let Some(project) = this
                                .agents
                                .read(cx)
                                .agent(agent_id)
                                .map(|agent| agent.project_id)
                            else {
                                return;
                            };
                            this.workspace
                                .update(cx, |workspace, cx| workspace.set_active(project, cx));
                            this.orbit.update(cx, |orbit, cx| {
                                orbit.select(project, OrbitModuleId::Custom(module_id), cx)
                            });
                            this.show_services(cx);
                        })),
                    )
                    .when(!card.undone, |head| {
                        head.child(
                            crate::ui::style::ghost_button_compact(
                                ("orbit-update-undo", invocation_id.as_u128() as u64),
                                "Undo",
                            )
                            .disabled(self.orbit_undos_pending.contains(&invocation_id))
                            .tooltip("Restore the Orbit records from before this turn")
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.undo_orbit_update(agent_id, invocation_id, cx);
                                },
                            )),
                        )
                    }),
            )
            .child(
                h_flex()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child(card.module_name.clone()),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(if changes.is_empty() {
                                "No record changes".to_string()
                            } else {
                                changes
                            }),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_and_file_tags_keep_navigable_targets() {
        let doc = AgentChatMessageTag {
            kind: AgentChatMessageTagKind::Doc,
            label: "Notification improvement".to_string(),
            detail: Some("choro_docs/notifications-improvement.choro".to_string()),
        };
        let file = AgentChatMessageTag {
            kind: AgentChatMessageTagKind::File,
            label: "main.rs".to_string(),
            detail: Some("src/main.rs".to_string()),
        };

        assert_eq!(
            saved_message_tag_navigation(&doc),
            Some(SavedMessageTagNavigation::Doc(PathBuf::from(
                "choro_docs/notifications-improvement.choro"
            )))
        );
        assert_eq!(
            saved_message_tag_navigation(&file),
            Some(SavedMessageTagNavigation::File(PathBuf::from(
                "src/main.rs"
            )))
        );
    }

    #[test]
    fn non_resource_and_empty_tags_stay_static() {
        let command = AgentChatMessageTag {
            kind: AgentChatMessageTagKind::Command,
            label: "Review".to_string(),
            detail: Some("Run a review".to_string()),
        };
        let empty_doc = AgentChatMessageTag {
            kind: AgentChatMessageTagKind::Doc,
            label: "Missing path".to_string(),
            detail: Some("   ".to_string()),
        };

        assert_eq!(saved_message_tag_navigation(&command), None);
        assert_eq!(saved_message_tag_navigation(&empty_doc), None);
    }

    #[test]
    fn project_relative_targets_resolve_inside_their_project() {
        let root = Path::new("/work/choro");
        assert_eq!(
            project_tag_target_path(root, Path::new("choro_docs/plan.choro")),
            PathBuf::from("/work/choro/choro_docs/plan.choro")
        );
        assert_eq!(
            project_tag_target_path(root, Path::new("/shared/plan.choro")),
            PathBuf::from("/shared/plan.choro")
        );
    }
}
