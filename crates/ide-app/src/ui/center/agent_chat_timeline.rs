use super::agent_chat_attachments::AttachmentRemoval;
use super::agent_chat_runtime::{verification_lifecycle, VerificationLifecycle};
use super::*;

fn should_show_ship_action(has_project_changed_files: bool, is_rejoined: bool) -> bool {
    has_project_changed_files && !is_rejoined
}

impl CenterArea {
    pub(super) fn render_agent_chat_body(
        &mut self,
        agent: &AgentRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        self.render_agent_chat_body_for_surface(agent, AgentChatSurface::Standard, window, cx)
    }

    pub(super) fn render_agent_chat_body_for_surface(
        &mut self,
        agent: &AgentRecord,
        surface: AgentChatSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let top_down = self.workspace.read(cx).conversation_layout
            == ide_core::config::ConversationLayout::TopDown;
        let compact_design_surface = surface.is_design();
        let compact_assistant_controls = surface.is_document();
        let artifact_filter = VisualizationArtifactFilter::new(agent.id, agent.runtime_path());
        let input = self.agent_chat_input(agent, surface.input_placeholder(), window, cx);
        let session = if let Some(session) = self.agent_chat_render_sessions.get(&agent.id) {
            session.clone()
        } else {
            let mut session = self
                .agent_chats
                .read(cx)
                .session(agent.id)
                .cloned()
                .unwrap_or_else(|| crate::state::agent_chat::AgentChatSession {
                    agent_id: agent.id,
                    title: agent.title.clone(),
                    chat_session_id: agent.chat_session_id.clone(),
                    cli_session_id: None,
                    hidden_from_notifications: false,
                    status: AgentChatStatus::Idle,
                    interaction_mode: AgentInteractionMode::Default,
                    composer_text: String::new(),
                    messages: Vec::new(),
                    timeline: Vec::new(),
                    queued_turns: Vec::new(),
                    work_log: Vec::new(),
                    pending_user_input: None,
                    pending_approval: None,
                    proposed_plan: None,
                    changed_files: Default::default(),
                    usage: None,
                    started_running_at: None,
                    last_activity_at: unix_now_secs(),
                });
            session
                .changed_files
                .remove_visualization_artifacts(agent.id, agent.runtime_path());
            session.timeline.retain_mut(|item| match item {
                AgentChatTimelineItem::ChangedFiles(summary) => {
                    summary.remove_visualization_artifacts(agent.id, agent.runtime_path());
                    !summary.is_empty()
                }
                AgentChatTimelineItem::FileChangeActivity(activity) => {
                    !artifact_filter.is_artifact(&activity.file.path)
                }
                _ => true,
            });
            if !surface.shows_changed_files() {
                session.changed_files = Default::default();
                session.timeline.retain(|item| {
                    !matches!(
                        item,
                        AgentChatTimelineItem::ChangedFiles(_)
                            | AgentChatTimelineItem::FileChangeActivity(_)
                    )
                });
            }
            let session = Rc::new(session);
            self.agent_chat_render_sessions
                .insert(agent.id, session.clone());
            session
        };
        self.sync_agent_chat_visualizations(agent, &session);
        let is_plan_mode =
            surface.allows_plan_mode() && session.interaction_mode == AgentInteractionMode::Plan;
        let attached_files = self
            .agent_chat_attached_files
            .get(&agent.id)
            .cloned()
            .unwrap_or_default();
        let pasted_text_blocks = self
            .agent_chat_pasted_text_blocks
            .get(&agent.id)
            .cloned()
            .unwrap_or_default();
        let selected_command = self.agent_chat_selected_commands.get(&agent.id).cloned();
        let preview_armed = self.agent_chat_preview_armed.contains(&agent.id);
        let input_value = input.read(cx).value().to_string();
        let preview_suggested = should_suggest_choro_preview(
            &input_value,
            preview_armed,
            self.agent_chat_preview_suggestion_dismissed
                .get(&agent.id)
                .map(String::as_str),
        );
        let selected_mentions = self
            .agent_chat_selected_mentions
            .get(&agent.id)
            .cloned()
            .unwrap_or_default();
        let selected_agent_target = self
            .agent_chat_selected_agent_targets
            .get(&agent.id)
            .and_then(|target_id| self.agents.read(cx).agent(*target_id).cloned());
        let has_attachments = !attached_files.is_empty();
        let has_pasted_text_blocks = !pasted_text_blocks.is_empty();
        let has_draft = !input.read(cx).value().trim().is_empty()
            || has_pasted_text_blocks
            || selected_command.is_some()
            || !selected_mentions.is_empty();
        // The draft reads like "remember this" — surface the memory chip so
        // the user sees Choro will capture it before they even send.
        let memory_armed = memory_save_intent(&input.read(cx).value());
        let has_pending_user_input = session.pending_user_input.is_some();
        let has_pending_approval = session.pending_approval.is_some();
        let has_actionable_plan = surface.allows_plan_mode()
            && session
                .proposed_plan
                .as_ref()
                .is_some_and(|plan| plan.implemented_at.is_none());
        let has_verification_prompt = self.verification_prompt_pending.contains(&agent.id);
        let is_reverification =
            verification_lifecycle(&session.timeline) == VerificationLifecycle::Fixing;
        let has_composer_decision =
            has_pending_approval || has_pending_user_input || has_actionable_plan;
        let has_changed_files = session
            .changed_files
            .files
            .iter()
            .any(|file| !artifact_filter.is_artifact(&file.path))
            || session
                .changed_files
                .observed_files
                .iter()
                .any(|file| !artifact_filter.is_artifact(&file.path));
        let project_gits = self.git_states.read(cx).repositories(agent.project_id);
        let active_git = if let Some(repository_path) = agent.repository_path.as_deref() {
            self.git_states
                .read(cx)
                .get_for_path(agent.project_id, repository_path)
        } else {
            project_gits
                .iter()
                .find(|git| {
                    git.read(cx)
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| !snapshot.entries.is_empty())
                })
                .cloned()
                .or_else(|| self.git_states.read(cx).get(agent.project_id))
        };
        let has_project_changed_files = surface.allows_project_actions()
            && if agent.is_active_solo() {
                // A Solo ships from its lane — the project tree is clean by
                // design, so the gate reads the agent's own changed files.
                agent.lane_path.is_some()
                    && agent
                        .changed_files
                        .iter()
                        .any(|file| !artifact_filter.is_artifact(&file.path))
            } else {
                let has_changes = |git: &Entity<GitState>| {
                    let git = git.read(cx);
                    git.is_repo
                        && git
                            .snapshot
                            .as_ref()
                            .is_some_and(|snapshot| !snapshot.entries.is_empty())
                };
                if agent.repository_path.is_none() {
                    project_gits.iter().any(has_changes)
                } else {
                    active_git.as_ref().is_some_and(has_changes)
                }
            };
        let show_ship_action = should_show_ship_action(
            has_project_changed_files,
            agent.solo_rejoined_branch.is_some(),
        );
        let compact_actions = self.agent_chat_rail_compact;
        let supported_efforts = agent.supported_efforts();
        let is_running = matches!(
            session.status,
            AgentChatStatus::Running | AgentChatStatus::Cancelling
        );
        // Advance the paced reveal for the streaming message and, while it is
        // still typing or cooling, schedule the next repaint.
        self.sync_agent_chat_reveal(agent.id, &session, is_running, cx);
        let chat_view = cx.entity().clone();
        let has_saved_session = session.chat_session_id.is_some()
            || session.cli_session_id.is_some()
            || agent.chat_session_id.is_some()
            || agent.cli_session_id.is_some();
        let provider_switch_locked = has_saved_session
            || !session.messages.is_empty()
            || self.agent_chats.read(cx).has_backend(agent.id);
        let is_initial_design_turn = should_show_design_start(
            compact_design_surface,
            provider_switch_locked,
            !session.messages.is_empty(),
        );
        let is_hydrating = self.agent_chat_hydrating.contains(&agent.id)
            && session.messages.is_empty()
            && session.timeline.is_empty();
        let rows = agent_chat_rows(&session, is_running, has_saved_session, &artifact_filter);
        let (display_order, newest_turn_len) = agent_chat_display_order(&rows, &session, top_down);
        // When the resume prompt is the only content, it's rendered as a
        // full-height centered panel (like the tab empty states) rather than a
        // top-aligned list row.
        let is_resume_only = matches!(rows.as_slice(), [AgentChatRow::ResumeSavedSession]);
        let row_count = rows.len();
        // A page can contain many adjacent tool events that collapse into only
        // a handful of display rows. Keep prepending until there is enough
        // content to scroll; otherwise the user could never reach the top
        // threshold that requests the next page.
        if row_count <= 12
            && self
                .agent_chat_history
                .get(&agent.id)
                .is_some_and(|history| history.has_more && !history.loading && !history.failed)
        {
            self.load_older_agent_chat_history(agent.id, cx);
        }
        let row_fingerprints = display_order
            .iter()
            .filter_map(|index| rows.get(*index))
            .map(|row| {
                agent_chat_row_fingerprint(row, &session, self.agent_chat_active_reveal.as_ref())
            })
            .collect::<Vec<_>>();
        let list_state = self.agent_chat_list_state(
            agent.id,
            row_count,
            newest_turn_len,
            top_down,
            &row_fingerprints,
            cx,
        );
        let list_agent = agent.clone();
        let list_session = session.clone();
        let scrolled_up = !is_hydrating
            && !is_resume_only
            && self
                .agent_chat_scrolled_up
                .get(&agent.id)
                .copied()
                .unwrap_or(false);
        let scroll_button = scrolled_up.then(|| {
            self.render_agent_chat_scroll_to_latest(
                agent.id,
                list_state.clone(),
                row_count,
                top_down,
                cx,
            )
        });

        v_flex()
            .size_full()
            .min_w(px(0.))
            .bg(crate::ui::design::base(cx))
            .when(top_down, |layout| layout.flex_col_reverse())
            .child(
                div()
                    .relative()
                    .flex_1()
                    .w_full()
                    .min_w(px(0.))
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(if is_hydrating {
                        self.render_agent_chat_resume_loader(agent, cx)
                    } else if is_resume_only {
                        self.render_agent_resume_saved_session(agent, cx)
                    } else {
                        list(
                            list_state,
                            cx.processor(move |this, index: usize, window, cx| {
                                let source_index =
                                    display_order.get(index).copied().unwrap_or(index);
                                this.render_agent_chat_list_row(
                                    &list_agent,
                                    &list_session,
                                    index,
                                    rows.get(source_index).copied(),
                                    window,
                                    cx,
                                )
                            }),
                        )
                        .size_full()
                        .py_5()
                        .into_any_element()
                    })
                    .when_some(scroll_button, |area, button| area.child(button)),
            )
            .child(
                v_flex()
                    .relative()
                    .when(top_down, |composer| composer.flex_col_reverse())
                    .w_full()
                    .px(crate::ui::design::agent_chat_gutter_x())
                    .when(compact_assistant_controls, |composer| composer.px_2())
                    // Tight bottom padding drops the composer to sit just above
                    // the Terminal/Files/Notes bar, level with the commit box.
                    .pt_3()
                    .pb_1()
                    .bg(crate::ui::design::base(cx))
                    // Pending turns live above the composer as their own quiet
                    // layer, keeping the input itself uncluttered.
                    .when(!session.queued_turns.is_empty(), |wrap| {
                        wrap.child(self.render_agent_chat_queue(
                            agent.id,
                            &session.queued_turns,
                            cx,
                        ))
                    })
                    // Verification is optional and must never replace or lock
                    // the composer. Keep it as a separate decision card above
                    // the input so the user can simply continue chatting.
                    .when(has_verification_prompt, |wrap| {
                        wrap.child(
                            div()
                                .w_full()
                                .max_w(crate::ui::design::agent_chat_content_max_w())
                                .mx_auto()
                                .mb_2()
                                .child(self.render_verification_decision_panel(
                                    agent.id,
                                    is_reverification,
                                    cx,
                                )),
                        )
                    })
                    // The Solo band perches on the composer's top border —
                    // outside the frame, flush against it.
                    .when_some(self.render_solo_lane_band(&agent, cx), |wrap, band| {
                        wrap.child(
                            div()
                                .w_full()
                                .max_w(crate::ui::design::agent_chat_content_max_w())
                                .mx_auto()
                                .child(band),
                        )
                    })
                    .child(
                        v_flex()
                            .relative()
                            .w_full()
                            .min_w(px(0.))
                            .max_w(crate::ui::design::agent_chat_content_max_w())
                            .mx_auto()
                            .gap_2()
                            .child(crate::ui::onboarding::target_marker(
                                crate::ui::onboarding::SpotlightTarget::Composer,
                                cx,
                            ))
                            .capture_action(cx.listener({
                                let agent = agent.clone();
                                move |this, _: &MoveDown, _window, cx| {
                                    if this.move_agent_chat_context_picker(&agent, 1, cx) {
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .capture_action(cx.listener({
                                let agent = agent.clone();
                                move |this, _: &MoveUp, _window, cx| {
                                    if this.move_agent_chat_context_picker(&agent, -1, cx) {
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .capture_action(cx.listener({
                                let agent = agent.clone();
                                move |this, _: &Escape, _window, cx| {
                                    if this.dismiss_agent_chat_context_picker(&agent, cx) {
                                        cx.stop_propagation();
                                        return;
                                    }
                                    let can_stop = this
                                        .agent_chats
                                        .read(cx)
                                        .session(agent.id)
                                        .is_some_and(|session| {
                                            matches!(
                                                session.status,
                                                AgentChatStatus::Running
                                                    | AgentChatStatus::Cancelling
                                            )
                                        });
                                    if can_stop {
                                        // First Escape requests a graceful stop;
                                        // pressing Escape again while Cancelling
                                        // follows the same force-stop path as a
                                        // second click on the stop control.
                                        this.request_agent_chat_stop(agent.id, cx);
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .capture_action(cx.listener({
                                let agent = agent.clone();
                                let input = input.clone();
                                move |this, _: &IndentInline, window, cx| {
                                    if this.accept_agent_chat_context_picker(
                                        &agent,
                                        input.clone(),
                                        window,
                                        cx,
                                    ) {
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .capture_action(cx.listener({
                                let agent = agent.clone();
                                let input = input.clone();
                                let surface = surface.clone();
                                move |this, action: &Enter, window, cx| {
                                    if window.modifiers().shift {
                                        cx.stop_propagation();
                                        input.update(cx, |input, cx| {
                                            input.insert("\n", window, cx);
                                            input.focus(window, cx);
                                        });
                                        return;
                                    }
                                    if action.secondary {
                                        cx.stop_propagation();
                                        if this
                                            .agent_chats
                                            .read(cx)
                                            .session(agent.id)
                                            .and_then(|session| session.pending_user_input.as_ref())
                                            .is_some()
                                        {
                                            this.continue_pending_user_input(
                                                agent.id,
                                                input.clone(),
                                                window,
                                                cx,
                                            );
                                        } else if surface.allows_plan_mode()
                                            && this
                                                .agent_chats
                                                .read(cx)
                                                .session(agent.id)
                                                .and_then(|session| session.proposed_plan.as_ref())
                                                .is_some_and(|plan| plan.implemented_at.is_none())
                                        {
                                            this.continue_proposed_plan(
                                                agent.id,
                                                input.clone(),
                                                window,
                                                cx,
                                            );
                                        } else {
                                            this.submit_agent_chat_message_for_surface(
                                                &agent,
                                                input.clone(),
                                                true,
                                                &surface,
                                                window,
                                                cx,
                                            );
                                        }
                                        return;
                                    }
                                    if this.accept_agent_chat_context_picker(
                                        &agent,
                                        input.clone(),
                                        window,
                                        cx,
                                    ) {
                                        cx.stop_propagation();
                                        return;
                                    }
                                    cx.stop_propagation();
                                    if this
                                        .agent_chats
                                        .read(cx)
                                        .session(agent.id)
                                        .and_then(|session| session.pending_user_input.as_ref())
                                        .is_some()
                                    {
                                        this.continue_pending_user_input(
                                            agent.id,
                                            input.clone(),
                                            window,
                                            cx,
                                        );
                                    } else if surface.allows_plan_mode()
                                        && this
                                            .agent_chats
                                            .read(cx)
                                            .session(agent.id)
                                            .and_then(|session| session.proposed_plan.as_ref())
                                            .is_some_and(|plan| plan.implemented_at.is_none())
                                    {
                                        this.continue_proposed_plan(
                                            agent.id,
                                            input.clone(),
                                            window,
                                            cx,
                                        );
                                    } else {
                                        this.submit_agent_chat_message_for_surface(
                                            &agent,
                                            input.clone(),
                                            false,
                                            &surface,
                                            window,
                                            cx,
                                        );
                                    }
                                }
                            }))
                            .capture_action(cx.listener({
                                let agent = agent.clone();
                                move |this, _: &Paste, _window, cx| {
                                    if this.paste_image_into_agent_chat(&agent, false, cx)
                                        || this.paste_long_text_into_agent_chat(agent.id, cx)
                                    {
                                        cx.stop_propagation();
                                    }
                                }
                            }))
                            .relative()
                            .min_h(crate::ui::design::composer_frame_h())
                            .rounded(crate::ui::design::r_lg())
                            // Under the Solo band (active or settled) the frame
                            // squares its top corners so they read as one piece.
                            .when(agent.is_solo(), |frame| {
                                frame.rounded_tl(px(0.)).rounded_tr(px(0.))
                            })
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::focus(cx))
                            .shadow(crate::ui::design::shadow())
                            .px_3p5()
                            .pt_3p5()
                            .pb_3()
                            .when(compact_assistant_controls, |frame| {
                                frame.px_2().pt_2().pb_2()
                            })
                            .can_drop(|dragged, _, _| dragged.is::<ExternalPaths>())
                            .on_drop::<ExternalPaths>(cx.listener({
                                let agent_id = agent.id;
                                move |this, paths: &ExternalPaths, _, cx| {
                                    this.attach_agent_chat_paths(agent_id, paths.paths(), cx);
                                }
                            }))
                            .when_some(session.pending_approval.as_ref(), |card, pending| {
                                card.child(self.render_pending_approval_panel(
                                    agent.id,
                                    pending,
                                    cx,
                                ))
                            })
                            .when_some(
                                session
                                    .pending_user_input
                                    .as_ref()
                                    .filter(|_| !has_pending_approval),
                                |card, pending| {
                                card.child(self.render_pending_user_input_panel(
                                    agent.id,
                                    pending,
                                    input.clone(),
                                    window,
                                    cx,
                                ))
                            })
                            .when_some(
                                session
                                    .proposed_plan
                                    .as_ref()
                                    .filter(|plan| surface.allows_plan_mode() && !has_pending_approval && !has_pending_user_input && plan.implemented_at.is_none()),
                                |card, plan| {
                                    card.child(self.render_proposed_plan_decision_panel(
                                        agent.id,
                                        plan,
                                        input.clone(),
                                        window,
                                        cx,
                                    ))
                                },
                            )
                            .when(!has_composer_decision, |card| {
                                let picker =
                                    self.render_agent_chat_context_picker(agent, input.clone(), window, cx);
                                card.when_some(picker, |card, picker| {
                                    card.child(
                                        div()
                                            .absolute()
                                            .left(px(0.))
                                            .right(px(0.))
                                            // Open toward the conversation in
                                            // either layout. The composer grows
                                            // with attachments and multiline input,
                                            // so anchor to its actual edge.
                                            .when(top_down, |picker| {
                                                picker.top(gpui::relative(1.)).pt(px(5.))
                                            })
                                            .when(!top_down, |picker| {
                                                picker.bottom(gpui::relative(1.)).pb(px(5.))
                                            })
                                            .child(picker),
                                    )
                                })
                            })
                            .when(!has_composer_decision, |card| {
                                card.child(
                                    v_flex()
                                        .w_full()
                                        .min_w(px(0.))
                                        .min_h(crate::ui::design::composer_input_min_h())
                                        .gap_2()
                                        .when(has_pasted_text_blocks, |col| {
                                            col.child(
                                                v_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .children(pasted_text_blocks.iter().map(
                                                        |block| {
                                                            self.render_agent_chat_pasted_text_block(
                                                                agent.id,
                                                                block,
                                                                window,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(has_attachments, |col| {
                                            col.child(
                                                h_flex()
                                                    .w_full()
                                                    .gap_2()
                                                    .flex_wrap()
                                                    .children(attached_files.iter().enumerate().map(
                                                        |(index, path)| {
                                                            self.render_agent_attachment_preview(
                                                                ("agent-chat-composer-attachment", index),
                                                                path.clone(),
                                                                Some(AttachmentRemoval::AgentChat {
                                                                    agent_id: agent.id,
                                                                    path: path.clone(),
                                                                }),
                                                                58.,
                                                                58.,
                                                                cx,
                                                            )
                                                        },
                                                    )),
                                            )
                                        })
                                        .when(
                                                selected_command.is_some()
                                                || !selected_mentions.is_empty()
                                                || selected_agent_target.is_some()
                                                || preview_armed
                                                || preview_suggested,
                                            |col| {
                                                col.child(self.render_agent_chat_input_prefix(
                                                    agent.id,
                                                    input.clone(),
                                                    selected_command.as_ref(),
                                                    &selected_mentions,
                                                    selected_agent_target.as_ref(),
                                                    preview_armed,
                                                    preview_suggested,
                                                    cx,
                                                ))
                                            },
                                        )
                                        .child(
                                            div().flex_1().min_h(px(0.)).child(
                                                Input::new(&input)
                                                    .appearance(false)
                                                    .bordered(false)
                                                    .focus_bordered(false)
                                                    .w_full()
                                                    .min_w(px(0.))
                                                    .h_full(),
                                            ),
                                        ),
                                )
                            })
                            .when(!has_composer_decision, |card| card.child(
                                h_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .relative()
                                    .gap_1()
                                    .items_center()
                                    .when(compact_assistant_controls, |row| {
                                        row.child(
                                            gpui_component::popover::Popover::new((
                                                "agent-chat-model-popover",
                                                agent.id.as_u128() as u64,
                                            ))
                                            .flex_none()
                                            .anchor(gpui::Corner::BottomLeft)
                                            .appearance(false)
                                            .open(self.composer_model_expanded)
                                            .on_open_change({
                                                let model_view = chat_view.clone();
                                                move |open, _, cx| {
                                                    model_view.update(cx, |this, cx| {
                                                        this.composer_model_expanded = *open;
                                                        if *open {
                                                            this.composer_model_provider = None;
                                                            this.refresh_open_code_models(false, cx);
                                                        }
                                                        cx.notify();
                                                    });
                                                }
                                            })
                                            .trigger(
                                                crate::ui::style::composer_chip(
                                                    (
                                                        "agent-chat-model",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    compact_assistant_model_label(
                                                        agent.model_short_label(),
                                                    ),
                                                    Some(
                                                        provider_brand_icon(agent.provider)
                                                            .size(crate::ui::design::icon_sm())
                                                            .into_any_element(),
                                                    ),
                                                    cx,
                                                )
                                                .tooltip(agent.model_label().to_string()),
                                            )
                                            .content({
                                                let model_view = chat_view.clone();
                                                let picker_agent = agent.clone();
                                                let picker_surface = surface.clone();
                                                move |_, _, cx| {
                                                    model_view.update(cx, |this, cx| {
                                                        this.render_agent_chat_model_picker(
                                                            &picker_agent,
                                                            picker_surface.clone(),
                                                            provider_switch_locked,
                                                            cx,
                                                        )
                                                        .into_any_element()
                                                    })
                                                }
                                            }),
                                        )
                                    })
                                    .when(!compact_assistant_controls, |row| {
                                        row.child(
                                            crate::ui::style::composer_chip(
                                                (
                                                    "agent-chat-model",
                                                    agent.id.as_u128() as u64,
                                                ),
                                                agent.model_short_label().to_string(),
                                                Some(
                                                    provider_brand_icon(agent.provider)
                                                        .size(crate::ui::design::icon_sm())
                                                        .into_any_element(),
                                                ),
                                                cx,
                                            )
                                            .tooltip(agent.model_label().to_string())
                                            .dropdown_menu({
                                                let agent_id = agent.id;
                                                let provider = agent.provider;
                                                let current_model = agent.model;
                                                let current_effort = agent.effort;
                                                let model_view = chat_view.clone();
                                                let surface = surface.clone();
                                                move |mut menu, window, _| {
                                                    let provider_label =
                                                        if provider == AgentKind::Codex {
                                                            "GPT-5.6"
                                                        } else {
                                                            provider.label()
                                                        };
                                                    menu = menu
                                                        .item(PopupMenuItem::label(provider_label));
                                                    for candidate in AgentModel::models_for(provider)
                                                        .iter()
                                                        .copied()
                                                    {
                                                        let surface = surface.clone();
                                                        let next_effort = candidate
                                                            .normalize_effort(current_effort);
                                                        menu = menu.item(
                                                            PopupMenuItem::new(
                                                                candidate.menu_label(),
                                                            )
                                                            .checked(candidate == current_model)
                                                            .on_click(window.listener_for(
                                                                &model_view,
                                                                move |this: &mut CenterArea,
                                                                      _,
                                                                      _,
                                                                      cx| {
                                                                    this.update_agent_chat_surface_model_effort(
                                                                        &surface,
                                                                        agent_id,
                                                                        provider,
                                                                        candidate,
                                                                        next_effort,
                                                                        cx,
                                                                    );
                                                                },
                                                            )),
                                                        );
                                                    }
                                                    menu
                                                }
                                            }),
                                        )
                                    })
                                    .when(!supported_efforts.is_empty(), |controls| {
                                        controls
                                            .child(crate::ui::style::composer_control_divider(cx))
                                            .child(
                                                crate::ui::style::composer_chip(
                                                    (
                                                        "agent-chat-effort",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    if compact_assistant_controls {
                                                        ""
                                                    } else {
                                                        agent.effort.label()
                                                    },
                                                    Some(composer_effort_icon(
                                                        agent.effort,
                                                        crate::ui::design::t3(cx),
                                                    )),
                                                    cx,
                                                )
                                                .tooltip("Reasoning effort")
                                                .dropdown_menu({
                                                    let agent_id = agent.id;
                                                    let provider = agent.provider;
                                                    let current_effort = agent.effort;
                                                    let current_model = agent.model;
                                                    let effort_options = supported_efforts.clone();
                                                    let effort_view = chat_view.clone();
                                                    let surface = surface.clone();
                                                    move |mut menu, window, _| {
                                                        for candidate in
                                                            effort_options.iter().copied()
                                                        {
                                                            let surface = surface.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new(
                                                                    candidate.menu_label(),
                                                                )
                                                                .checked(candidate == current_effort)
                                                                .on_click(window.listener_for(
                                                                    &effort_view,
                                                                    move |this: &mut CenterArea,
                                                                          _,
                                                                          _,
                                                                          cx| {
                                                                        this.update_agent_chat_surface_model_effort(
                                                                            &surface,
                                                                            agent_id,
                                                                            provider,
                                                                            current_model,
                                                                            candidate,
                                                                            cx,
                                                                        );
                                                                    },
                                                                )),
                                                            );
                                                        }
                                                        menu
                                                    }
                                                }),
                                            )
                                    })
                                    .child(crate::ui::style::composer_control_divider(cx))
                                    .child(
                                                crate::ui::style::composer_chip(
                                                    (
                                                        "agent-chat-access-mode",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    if compact_assistant_controls {
                                                        ""
                                                    } else {
                                                        agent
                                                            .access_mode
                                                            .label_for(agent.provider)
                                                    },
                                                    Some(composer_access_icon(
                                                        agent.access_mode,
                                                        crate::ui::design::t3(cx),
                                                    )),
                                                    cx,
                                                )
                                                .tooltip("Access mode")
                                                .dropdown_menu({
                                                    let agent_id = agent.id;
                                                    let provider = agent.provider;
                                                    let current_access_mode = agent.access_mode;
                                                    let access_view = chat_view.clone();
                                                    let surface = surface.clone();
                                                    move |mut menu, window, _| {
                                                        for candidate in AgentAccessMode::ALL {
                                                            let surface = surface.clone();
                                                            menu = menu.item(
                                                                PopupMenuItem::new(
                                                                    candidate.label_for(provider),
                                                                )
                                                                .checked(
                                                                    candidate
                                                                        == current_access_mode,
                                                                )
                                                                .on_click(window.listener_for(
                                                                    &access_view,
                                                                    move |this: &mut CenterArea,
                                                                          _,
                                                                          _,
                                                                          cx| {
                                                                        this.update_agent_chat_surface_access_mode(
                                                                            &surface,
                                                                            agent_id,
                                                                            candidate,
                                                                            cx,
                                                                        );
                                                                    },
                                                                )),
                                                            );
                                                        }
                                                        menu
                                                    }
                                                }),
                                    )
                                    .when(is_plan_mode, |row| {
                                        row.child(crate::ui::style::composer_control_divider(cx)).child(
                                            crate::ui::style::composer_toggle_chip(
                                                (
                                                    "agent-chat-plan-mode",
                                                    agent.id.as_u128() as u64,
                                                ),
                                                "Plan",
                                                Some(composer_mode_icon(
                                                    true,
                                                    crate::ui::design::accent(cx),
                                                )),
                                                true,
                                                cx,
                                            )
                                            .tooltip(
                                                "Plan mode is active. Click to return to build mode",
                                            )
                                            .on_click(cx.listener({
                                                let agent_id = agent.id;
                                                move |this, _, _, cx| {
                                                    if crate::ui::onboarding::locks_onboarding_plan_mode(cx) {
                                                        return;
                                                    }
                                                    this.agent_chats.update(cx, |chats, cx| {
                                                        let session = chats.ensure_session(
                                                            agent_id,
                                                            "Agent chat",
                                                            cx,
                                                        );
                                                        session.interaction_mode =
                                                            AgentInteractionMode::Default;
                                                    });
                                                    cx.notify();
                                                }
                                            })),
                                        )
                                    })
                                    .child(div().flex_1().min_w(px(0.)))
                                    // "Remember…" detected: Choro will save
                                    // this to its shared memory. The chip
                                    // makes the capture visible before send.
                                    .when(memory_armed, |row| {
                                        row.child(
                                            h_flex()
                                                .id((
                                                    "agent-chat-memory-armed",
                                                    agent.id.as_u128() as u64,
                                                ))
                                                .gap_1()
                                                .items_center()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded(crate::ui::design::r_sm())
                                                .bg(crate::ui::design::sage(cx).opacity(0.12))
                                                .text_size(crate::ui::design::text_label())
                                                .text_color(crate::ui::design::sage(cx))
                                                .child(
                                                    crate::ui::design::indicator::lucide_icon(
                                                        lucide_icons::Icon::Brain,
                                                        crate::ui::design::sage(cx),
                                                        crate::ui::design::icon_sm(),
                                                    ),
                                                )
                                                .child("Memory"),
                                        )
                                    })
                                    .when(
                                        has_changed_files
                                            && surface.allows_project_actions(),
                                        |row| {
                                        row.child(
                                            if compact_actions {
                                                crate::ui::style::composer_icon_action(
                                                    (
                                                        "agent-chat-code-review",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    crate::ui::design::indicator::lucide_icon(
                                                        lucide_icons::Icon::SearchCode,
                                                        crate::ui::design::t3(cx),
                                                        crate::ui::design::icon_sm(),
                                                    ),
                                                    cx,
                                                )
                                            } else {
                                                crate::ui::style::composer_ghost_action(
                                                    (
                                                        "agent-chat-code-review",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    "Code review",
                                                    crate::ui::design::indicator::lucide_icon(
                                                        lucide_icons::Icon::SearchCode,
                                                        crate::ui::design::t3(cx),
                                                        crate::ui::design::icon_sm(),
                                                    ),
                                                    cx,
                                                )
                                            }
                                            .tooltip("Ask the agent to review the changed files")
                                            .on_click(cx.listener({
                                                let agent_id = agent.id;
                                                move |this, _, _, cx| {
                                                    this.request_agent_code_review(agent_id, cx);
                                                }
                                            })),
                                        )
                                        },
                                    )
                                    .when(show_ship_action, |row| {
                                        row.child(
                                            div()
                                                .relative()
                                                .flex_none()
                                                .child(
                                                    crate::ui::style::composer_ship_action(
                                                        (
                                                            "agent-chat-ship",
                                                            agent.id.as_u128() as u64,
                                                        ),
                                                        "Ship",
                                                        false,
                                                        cx,
                                                    )
                                                    .tooltip("Ship project changes")
                                                    .on_click({
                                                        let agent = agent.clone();
                                                        let git = active_git.clone();
                                                        cx.listener(move |this, _, window, cx| {
                                                            if let Some(git) = git.clone() {
                                                                this.open_agent_ship_dialog(
                                                                    agent.clone(),
                                                                    git,
                                                                    window,
                                                                    cx,
                                                                );
                                                            }
                                                        })
                                                    }),
                                                )
                                                .child(crate::ui::onboarding::target_marker(
                                                    crate::ui::onboarding::SpotlightTarget::Ship,
                                                    cx,
                                                )),
                                        )
                                    })
                                    .child({
                                        let can_send = has_draft || has_attachments;
                                        if is_running && !can_send {
                                            if compact_assistant_controls {
                                                crate::ui::style::composer_stop(
                                                    (
                                                        "agent-chat-stop",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    cx,
                                                )
                                            } else {
                                                crate::ui::style::composer_stop_neutral(
                                                    (
                                                        "agent-chat-stop",
                                                        agent.id.as_u128() as u64,
                                                    ),
                                                    cx,
                                                )
                                            }
                                                .tooltip(|window, cx| {
                                                    Tooltip::new("Stop agent").build(window, cx)
                                                })
                                                .on_click(cx.listener({
                                                    let agent_id = agent.id;
                                                    move |this, _, _, cx| {
                                                        this.request_agent_chat_stop(agent_id, cx);
                                                    }
                                                }))
                                                .into_any_element()
                                        } else if is_initial_design_turn {
                                            let button = crate::ui::style::primary_button_compact(
                                                (
                                                    "agent-chat-start-design",
                                                    agent.id.as_u128() as u64,
                                                ),
                                                "Start",
                                                cx,
                                            )
                                            .icon(IconName::ArrowUp)
                                            .disabled(!can_send)
                                            .tooltip(if can_send {
                                                "Start designing with the selected model"
                                            } else {
                                                "Enter a design request before starting"
                                            });
                                            if can_send {
                                                button
                                                    .on_click(cx.listener({
                                                        let agent = agent.clone();
                                                        let input = input.clone();
                                                        let surface = surface.clone();
                                                        move |this, _, window, cx| {
                                                            this.submit_agent_chat_message_for_surface(
                                                                &agent,
                                                                input.clone(),
                                                                false,
                                                                &surface,
                                                                window,
                                                                cx,
                                                            );
                                                        }
                                                    }))
                                                    .into_any_element()
                                            } else {
                                                button.into_any_element()
                                            }
                                        } else {
                                            // A Solo sends in sky — the lane
                                            // color rides the act itself.
                                            let solo = agent.is_active_solo();
                                            let send_fill = if solo {
                                                crate::ui::design::sky(cx)
                                            } else {
                                                crate::ui::design::accent(cx)
                                            };
                                            let send_hover = if solo {
                                                crate::ui::design::sky(cx).opacity(0.85)
                                            } else {
                                                crate::ui::design::accent_2(cx)
                                            };
                                            crate::ui::style::composer_send_in(
                                                ("agent-chat-send", agent.id.as_u128() as u64),
                                                send_fill,
                                                cx,
                                            )
                                                .when(can_send, |button| {
                                                    button
                                                        .cursor_pointer()
                                                        .hover(move |button| {
                                                            button.bg(send_hover)
                                                        })
                                                        .on_click(cx.listener({
                                                            let agent = agent.clone();
                                                            let input = input.clone();
                                                            let surface = surface.clone();
                                                            move |this, _, window, cx| {
                                                                this.submit_agent_chat_message_for_surface(
                                                                    &agent,
                                                                    input.clone(),
                                                                    false,
                                                                    &surface,
                                                                    window,
                                                                    cx,
                                                                );
                                                            }
                                                        }))
                                                })
                                                .into_any_element()
                                        }
                                    })
                                    // Zero-footprint width probe, matching the
                                    // new-agent composer rail. At narrow widths
                                    // the contextual actions retain their icons
                                    // but drop their labels.
                                    .child(
                                        canvas(
                                            {
                                                let measure_view = chat_view.clone();
                                                move |bounds, _window, cx| {
                                                    let compact = bounds.size.width < px(660.);
                                                    measure_view.update(cx, |this, cx| {
                                                        if this.agent_chat_rail_compact != compact {
                                                            this.agent_chat_rail_compact = compact;
                                                            cx.notify();
                                                        }
                                                    });
                                                }
                                            },
                                            |_, _, _, _| {},
                                        )
                                        .absolute()
                                        .inset_0(),
                                    ),
                            )),
                    ),
            )
            .into_any_element()
    }
}

fn compact_assistant_model_label(label: &str) -> String {
    const MAX_CHARS: usize = 18;
    let mut chars = label.chars();
    let prefix = chars.by_ref().take(MAX_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn should_show_design_start(
    is_design_surface: bool,
    provider_switch_locked: bool,
    has_messages: bool,
) -> bool {
    is_design_surface && !provider_switch_locked && !has_messages
}

fn assistant_open_code_effort(variants: &[String], current: AgentEffort) -> AgentEffort {
    let supported = AgentEffort::supported_variants(variants);
    if supported.is_empty() || supported.contains(&current) {
        return current;
    }
    [
        AgentEffort::High,
        AgentEffort::Medium,
        AgentEffort::Low,
        AgentEffort::XHigh,
        AgentEffort::Max,
    ]
    .into_iter()
    .find(|effort| supported.contains(effort))
    .unwrap_or(current)
}

#[cfg(test)]
mod assistant_control_tests {
    use super::*;

    #[test]
    fn compact_model_label_preserves_short_names_and_truncates_long_ones() {
        assert_eq!(compact_assistant_model_label("Sonnet 5"), "Sonnet 5");
        assert_eq!(
            compact_assistant_model_label("A very long provider model name"),
            "A very long provid…"
        );
    }

    #[test]
    fn open_code_effort_prefers_high_when_current_variant_is_unsupported() {
        let variants = vec!["low".to_string(), "high".to_string()];
        assert_eq!(
            assistant_open_code_effort(&variants, AgentEffort::Max),
            AgentEffort::High
        );
    }

    #[test]
    fn design_start_is_only_shown_before_the_first_explicit_turn() {
        assert!(should_show_design_start(true, false, false));
        assert!(!should_show_design_start(false, false, false));
        assert!(!should_show_design_start(true, true, false));
        assert!(!should_show_design_start(true, false, true));
    }

    #[test]
    fn ship_action_disappears_after_a_solo_is_rejoined() {
        assert!(should_show_ship_action(true, false));
        assert!(!should_show_ship_action(true, true));
        assert!(!should_show_ship_action(false, false));
    }
}
