#![allow(dead_code, reason = "retained document-to-agent interaction paths")]

use super::*;

fn agent_detail_shows_terminal(runtime: AgentRuntimeKind) -> bool {
    runtime != AgentRuntimeKind::Chat
}

impl CenterArea {
    /// Once a Doc Assistant turn finishes, promote a title it wrote into an
    /// untitled document to the document's filename. Waiting for Running → Idle
    /// ensures the file is never moved out from under an active assistant.
    pub(super) fn maybe_finalize_doc_assistant_titles(&mut self, cx: &mut Context<Self>) {
        let completed_agent_ids = {
            let chats = self.agent_chats.read(cx);
            chats
                .sessions
                .iter()
                .filter_map(|(agent_id, session)| {
                    (self.agent_status_seen.get(agent_id) == Some(&AgentChatStatus::Running)
                        && session.status == AgentChatStatus::Idle)
                        .then_some(*agent_id)
                })
                .collect::<HashSet<_>>()
        };
        if completed_agent_ids.is_empty() {
            return;
        }

        let records = self
            .doc_assistants
            .read(cx)
            .records()
            .iter()
            .filter(|record| completed_agent_ids.contains(&record.chat_agent_id))
            .filter(|record| !ide_core::penpot_assistant::is_record_path(&record.relative_doc_path))
            .cloned()
            .collect::<Vec<_>>();

        for record in records {
            let Some(project_path) = self
                .workspace
                .read(cx)
                .projects
                .iter()
                .find(|project| project.id == record.project_id)
                .map(|project| project.path.clone())
            else {
                continue;
            };
            let previous_relative = record.relative_doc_path.clone();
            let previous_path = project_path.join(&previous_relative);
            let renamed = self.docs.update(cx, |docs, cx| {
                docs.rename_untitled_doc_from_document_title(record.project_id, &previous_path, cx)
            });
            let Ok(Some(next_path)) = renamed else {
                if let Err(error) = renamed {
                    eprintln!("failed to title document from assistant edit: {error:#}");
                }
                continue;
            };
            let Some(next_relative) =
                self.docs
                    .read(cx)
                    .relative_path_for(record.project_id, &next_path, cx)
            else {
                continue;
            };
            self.agents.update(cx, |agents, cx| {
                agents.move_doc_reference(
                    record.project_id,
                    &previous_relative,
                    &next_relative,
                    cx,
                );
            });
            self.doc_assistants.update(cx, |assistants, cx| {
                assistants.move_doc_reference(
                    record.project_id,
                    &previous_relative,
                    next_relative.clone(),
                    cx,
                );
            });
        }
    }

    pub fn open_doc(&mut self, project: ProjectId, path: PathBuf, cx: &mut Context<Self>) {
        self.doc_action_error = None;
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        self.docs
            .update(cx, |docs, cx| docs.select_doc(project, path, cx));
        self.set_context_mode(ContextMode::Docs, cx);
        self.new_agent_composer = None;
        cx.notify();
    }

    pub fn create_doc_from_template(
        &mut self,
        project: ProjectId,
        source: crate::state::docs::DocTemplateSource,
        cx: &mut Context<Self>,
    ) {
        match self.docs.update(cx, |docs, cx| {
            docs.create_doc_from_template(project, &source, cx)
        }) {
            Ok(path) => {
                self.doc_action_error = None;
                self.open_created_doc(project, path, cx);
            }
            Err(error) => {
                self.doc_action_error = Some(format!("Could not create document: {error:#}"));
                cx.notify();
            }
        }
    }

    pub fn confirm_delete_doc(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let relative_label = self
            .docs
            .read(cx)
            .relative_path_for(project, &path, cx)
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let is_template = self
            .docs
            .read(cx)
            .templates_for_project(project)
            .iter()
            .any(|entry| entry.path == path);
        let center = cx.entity();
        let delete_path = path.clone();
        ConfirmDialog::new(
            crate::state::docs::delete_confirmation_title(&path),
            if is_template {
                "This permanently removes the project template. Existing documents are not affected."
            } else {
                "This permanently removes the document from your project. It can't be undone."
            },
        )
        .detail(relative_label)
        .confirm_label("Delete")
        .confirm_id("confirm-delete-doc")
        .on_confirm(move |_, cx| {
            let delete_path = delete_path.clone();
            center.update(cx, move |center, cx| {
                center.doc_title_edit = None;
                let relative = center
                    .docs
                    .read(cx)
                    .relative_path_for(project, &delete_path, cx);
                match center.docs.update(cx, |docs, cx| {
                    docs.delete_doc(project, &delete_path, cx)
                }) {
                    Ok(()) => {
                        if let Some(relative) = relative {
                            center.agents.update(cx, |agents, cx| {
                                agents.remove_doc_reference(project, &relative, cx);
                            });
                            center.doc_assistants.update(cx, |assistants, cx| {
                                assistants.remove_doc_reference(project, &relative, cx);
                            });
                        }
                    }
                    Err(error) => eprintln!("failed to delete doc: {error:#}"),
                }
            });
        })
        .open(window, cx);
    }

    /// The project whose root is the closest ancestor of `path`.
    pub(super) fn project_for_path(&self, path: &Path, cx: &App) -> Option<(ProjectId, PathBuf)> {
        self.workspace
            .read(cx)
            .projects
            .iter()
            .filter(|project| path.starts_with(&project.path))
            .max_by_key(|project| project.path.as_os_str().len())
            .map(|project| (project.id, project.path.clone()))
    }

    /// Resolve a `ref:` target clicked inside a doc editor and navigate to it:
    /// `ref:file:<rel>` opens the file editor, `ref:doc:<rel>` opens the doc,
    /// `ref:design:<uuid>` opens the design source and `ref:penpot:<uuid>`
    /// selects the Penpot file in Choro. Drained from render so a
    /// `Window` is available for the file editor's focus call.
    pub(super) fn apply_pending_reference_open(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((doc_path, target)) = self.pending_reference_open.take() else {
            return;
        };
        let Some((project, root)) = self.project_for_path(&doc_path, cx) else {
            return;
        };
        if let Some(path) = target.strip_prefix("ref:image:") {
            // Open the in-app image preview (same modal as chat attachments),
            // never the OS image viewer.
            self.open_agent_image_preview(PathBuf::from(path), window, cx);
        } else if let Some(relative) = target.strip_prefix("ref:file:") {
            self.open_file(project, root.join(relative), window, cx);
        } else if let Some(relative) = target.strip_prefix("ref:doc:") {
            self.open_doc(project, root.join(relative), cx);
        } else if let Some(id) = target.strip_prefix("ref:penpot:") {
            if let Ok(uuid) = Uuid::parse_str(id) {
                self.open_penpot_design(project, uuid, cx);
            }
        } else if let Some(id) = target.strip_prefix("ref:design:") {
            if let Ok(uuid) = Uuid::parse_str(id) {
                if let Some(reference) = self
                    .designs
                    .read(cx)
                    .references_for_project(project)
                    .into_iter()
                    .find(|reference| reference.id == uuid)
                {
                    crate::ui::center::designs::open_design_reference_source(&reference);
                }
            }
        }
    }

    pub fn set_docs_assistant_open(&mut self, open: bool, cx: &mut Context<Self>) {
        self.docs_focus_mode = if open {
            DocsFocusMode::Assistant
        } else {
            DocsFocusMode::Doc
        };
        if open {
            self.docs_terminal_mode = DocsTerminalMode::Assistant;
        }
        cx.notify();
    }

    /// Left-edge drag handle for the floating doc-assistant panel. Mirrors the
    /// sidebar resize handle: capture the start state on mouse-down, then update
    /// the stored width on drag. The panel is right-anchored, so dragging left
    /// grows it.
    pub(super) fn doc_assistant_resize_handle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("doc-assistant-resize")
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .w(px(6.))
            .h_full()
            .cursor_ew_resize()
            .child(
                div()
                    .w(px(1.))
                    .h_full()
                    .bg(crate::ui::design::line(cx).opacity(0.32)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| {
                    this.doc_assistant_resize = Some(DocAssistantResizeState {
                        start_x: event.position.x.as_f32(),
                        start_width: this.doc_assistant_panel_width,
                    });
                }),
            )
            .on_drag(DocAssistantResizeHandle, |drag, _, _, cx| {
                cx.stop_propagation();
                cx.new(|_| drag.clone())
            })
            .on_drag_move(cx.listener(
                |this, event: &DragMoveEvent<DocAssistantResizeHandle>, _, cx| {
                    let Some(resize) = &this.doc_assistant_resize else {
                        return;
                    };
                    let delta = event.event.position.x.as_f32() - resize.start_x;
                    this.doc_assistant_panel_width = (resize.start_width - delta)
                        .clamp(DOC_ASSISTANT_PANEL_MIN, DOC_ASSISTANT_PANEL_MAX);
                    cx.notify();
                },
            ))
    }

    pub(super) fn doc_assistant_agent_record(
        record: &DocAssistantRecord,
        project_path: PathBuf,
    ) -> AgentRecord {
        let mut agent = AgentRecord::new(
            record.project_id,
            project_path,
            record.title(),
            doc_assistant::system_prompt(&record.relative_doc_path),
            record.provider,
            record.model,
            record.effort,
            record.access_mode,
        );
        agent.id = record.chat_agent_id;
        agent.runtime = AgentRuntimeKind::Chat;
        if record.provider == AgentKind::OpenCode {
            if let (Some(model_id), Some(model_label)) = (
                record.external_model_id.as_deref(),
                record.external_model_label.as_deref(),
            ) {
                agent.set_external_model(
                    model_id,
                    model_label,
                    record.external_model_variants.clone(),
                );
                agent.effort = record.effort;
            }
        }
        agent.linked_docs = vec![record.relative_doc_path.clone()];
        agent.source_doc = Some(record.relative_doc_path.clone());
        agent.hidden_doc_assistant = true;
        agent.chat_session_id = record.chat_session_id.clone().or_else(|| {
            (record.provider == AgentKind::Codex)
                .then(|| record.cli_session_id.clone())
                .flatten()
        });
        agent.cli_session_id = record.cli_session_id.clone();
        agent
    }

    pub(super) fn hydrate_doc_assistant_chat_session(
        &mut self,
        record: &DocAssistantRecord,
        project_path: &Path,
        cx: &mut Context<Self>,
    ) {
        // Rendering a document or Design assistant calls this method for every
        // composer change. `ensure_session` notifies AgentChatState even when
        // the session already exists, which used to invalidate the chat render
        // cache and schedule another full center render for every keystroke.
        // `hidden_from_notifications` is set below as part of the one-time
        // specialized-assistant hydration, so it also serves as the stable
        // initialized marker here.
        if self
            .agent_chats
            .read(cx)
            .session(record.chat_agent_id)
            .is_some_and(|session| session.hidden_from_notifications)
        {
            return;
        }
        let session_id = record
            .chat_session_id
            .as_deref()
            .or(record.cli_session_id.as_deref())
            .map(str::to_string);
        let effective_chat_session_id = record.chat_session_id.clone().or_else(|| {
            (record.provider == AgentKind::Codex)
                .then(|| record.cli_session_id.clone())
                .flatten()
        });
        self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(record.chat_agent_id, record.title(), cx);
            session.hidden_from_notifications = true;
            session.interaction_mode = AgentInteractionMode::Default;
            session.proposed_plan = None;
            session
                .timeline
                .retain(|item| !matches!(item, AgentChatTimelineItem::ProposedPlan(_)));
            if session.status == AgentChatStatus::PlanReady {
                session.status = AgentChatStatus::Idle;
            }
            if session.chat_session_id.is_none() {
                session.chat_session_id = effective_chat_session_id.clone();
            }
            if session.cli_session_id.is_none() {
                session.cli_session_id = record.cli_session_id.clone();
            }
            if !session.messages.is_empty() {
                return;
            }
            let Some(session_id) = session_id.as_deref() else {
                return;
            };
            let messages =
                doc_assistant::read_chat_messages(record.provider, project_path, session_id);
            if messages.is_empty() {
                return;
            }
            let created_at = unix_now_secs();
            let hydrated = messages
                .into_iter()
                .filter_map(|message| match message.role {
                    DocAssistantRole::User => Some(AgentChatMessage::User {
                        text: message.text,
                        display_text: None,
                        tags: Vec::new(),
                        created_at,
                    }),
                    DocAssistantRole::Assistant => Some(AgentChatMessage::Assistant {
                        message_id: None,
                        text: message.text,
                        created_at,
                    }),
                })
                .collect::<Vec<_>>();
            session.timeline = hydrated
                .iter()
                .cloned()
                .map(AgentChatTimelineItem::Message)
                .collect();
            session.messages = hydrated;
        });
    }

    pub(super) fn ensure_doc_assistant_chat_backend(
        &mut self,
        record: &DocAssistantRecord,
        project_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> bool {
        self.hydrate_doc_assistant_chat_session(record, &project_path, cx);
        let agent = Self::doc_assistant_agent_record(record, project_path);
        match self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(record.chat_agent_id, record.title(), cx);
            session.hidden_from_notifications = true;
            chats.start_backend(agent, AgentInteractionMode::Default, cx)
        }) {
            Ok(()) => true,
            Err(error) => {
                self.doc_assistant_errors.insert(
                    record.key(),
                    format!("Could not start assistant: {error:#}"),
                );
                cx.notify();
                false
            }
        }
    }

    pub(super) fn sync_doc_assistant_chat_session_ids(&mut self, cx: &mut Context<Self>) {
        let updates = {
            let assistants = self.doc_assistants.read(cx);
            let chats = self.agent_chats.read(cx);
            assistants
                .records()
                .iter()
                .filter_map(|record| {
                    let session = chats.session(record.chat_agent_id)?;
                    let chat_session_id = session
                        .chat_session_id
                        .clone()
                        .or_else(|| record.chat_session_id.clone());
                    let cli_session_id = session
                        .cli_session_id
                        .clone()
                        .or_else(|| record.cli_session_id.clone());
                    (record.chat_session_id != chat_session_id
                        || record.cli_session_id != cli_session_id)
                        .then(|| (record.key(), chat_session_id, cli_session_id))
                })
                .collect::<Vec<_>>()
        };
        if updates.is_empty() {
            return;
        }
        self.doc_assistants.update(cx, |assistants, cx| {
            for (key, chat_session_id, cli_session_id) in updates {
                assistants.set_chat_session_ids(&key, chat_session_id, cli_session_id, cx);
            }
        });
        for record in self
            .doc_assistants
            .read(cx)
            .records()
            .iter()
            .filter(|record| ide_core::penpot_assistant::is_record_path(&record.relative_doc_path))
        {
            let _ = ide_core::local_store::LocalStore::open_default().and_then(|store| {
                store.update_penpot_conversation_runtime(
                    record.chat_agent_id,
                    record.provider,
                    record.model,
                    record.external_model_id.as_deref(),
                    record.external_model_label.as_deref(),
                    &record.external_model_variants,
                    record.effort,
                    record.access_mode,
                    record.chat_session_id.as_deref(),
                    record.cli_session_id.as_deref(),
                    record.last_transcript_path.as_deref(),
                )
            });
        }
    }

    pub fn open_created_doc(&mut self, project: ProjectId, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(relative) = self.docs.read(cx).relative_path_for(project, &path, cx) {
            let key = DocAssistantState::key_for(project, &relative);
            if let Some(terminal_id) = self
                .terminals
                .read(cx)
                .doc_assistant_session(project, &key)
                .map(|session| session.id)
            {
                self.terminals.update(cx, |terminals, cx| {
                    terminals.close(terminal_id, cx);
                });
            }
            self.doc_assistants.update(cx, |assistants, cx| {
                assistants.reset_record(project, relative, cx);
            });
        }
        self.open_doc(project, path, cx);
    }

    pub(super) fn current_doc_implementor(
        &self,
        project: ProjectId,
        relative_doc_path: &std::path::Path,
        cx: &App,
    ) -> Option<AgentRecord> {
        self.agents
            .read(cx)
            .doc_implementors(project, relative_doc_path)
            .into_iter()
            .next()
    }

    pub(super) fn doc_implementors(
        &self,
        project: ProjectId,
        relative_doc_path: &std::path::Path,
        cx: &App,
    ) -> Vec<AgentRecord> {
        self.agents
            .read(cx)
            .doc_implementors(project, relative_doc_path)
    }

    pub(super) fn open_doc_implementor_terminal(
        &mut self,
        project: ProjectId,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        self.docs_focus_mode = DocsFocusMode::Assistant;
        self.docs_terminal_mode = DocsTerminalMode::Implementor;
        self.set_context_mode(ContextMode::Docs, cx);
        self.start_agent_in_mode(agent_id, CenterMode::Docs, window, cx);
    }

    pub fn add_doc_to_new_agent(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(relative) = self.docs.read(cx).relative_path_for(project, &path, cx) else {
            return;
        };
        let should_open = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.project != project)
            .unwrap_or(true);
        if should_open {
            self.open_new_agent_composer_for_project(project, window, cx);
        }
        if let Some(composer) = self.new_agent_composer.as_mut() {
            if !composer.linked_docs.contains(&relative) {
                composer.linked_docs.push(relative);
                composer.error = None;
            }
        }
        self.set_view_mode(CenterMode::Agents, cx);
        cx.notify();
    }

    pub(super) fn use_doc_in_agent(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agents.read(cx).selected_agent_id(project).is_some() {
            self.link_doc_to_selected_agent(project, &path, cx);
            self.set_view_mode(CenterMode::Agents, cx);
            cx.notify();
        } else {
            self.add_doc_to_new_agent(project, path, window, cx);
        }
    }

    pub(super) fn link_doc_to_selected_agent(
        &mut self,
        project: ProjectId,
        path: &PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(relative) = self.docs.read(cx).relative_path_for(project, path, cx) else {
            return;
        };
        let Some(agent_id) = self.agents.read(cx).selected_agent_id(project) else {
            return;
        };
        let mut linked_docs = self
            .agents
            .read(cx)
            .agent(agent_id)
            .map(|agent| agent.linked_docs.clone())
            .unwrap_or_default();
        if !linked_docs.contains(&relative) {
            linked_docs.push(relative);
        }
        self.agents.update(cx, |agents, cx| {
            agents.update_linked_docs(agent_id, linked_docs, cx)
        });
        cx.notify();
    }

    pub(super) fn send_doc_to_selected_chat(
        &mut self,
        project: ProjectId,
        path: &PathBuf,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(relative) = self.docs.read(cx).linked_path_line(project, path, cx) else {
            return false;
        };
        let Some(agent_id) = self.agents.read(cx).selected_agent_id(project) else {
            return false;
        };
        let Some(terminal_id) = self
            .terminals
            .read(cx)
            .agent_record_terminal(project, agent_id)
        else {
            return false;
        };
        let message = format!("Use these docs for context: {relative}\r");
        self.terminals.update(cx, |terminals, cx| {
            terminals.send_text(terminal_id, &message, cx)
        })
    }

    pub(super) fn has_file_content(&self, project: ProjectId) -> bool {
        self.editors.iter().any(|item| item.project == project)
            || self.diffs.iter().any(|item| item.project == project)
    }

    pub(super) fn has_terminal_content(&self, project: ProjectId, cx: &Context<Self>) -> bool {
        !self.terminals.read(cx).sessions_for(project).is_empty()
    }

    pub(super) fn effective_code_mode(
        &self,
        project: ProjectId,
        has_terminals: bool,
    ) -> CenterMode {
        let has_files = self.has_file_content(project);
        match self.view_mode {
            CenterMode::Split | CenterMode::Files | CenterMode::Terminal => {
                if has_files && has_terminals {
                    self.view_mode
                } else if has_files {
                    CenterMode::Files
                } else if has_terminals {
                    CenterMode::Terminal
                } else {
                    CenterMode::Split
                }
            }
            mode => mode,
        }
    }

    pub(super) fn render_code_mode_switch(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let mode = self.view_mode;
        let center = cx.entity().clone();
        crate::ui::style::agent_detail_tabs()
            .child(
                crate::ui::style::agent_detail_tab(
                    "center-mode-split",
                    lucide_icons::Icon::PanelBottomOpen,
                    "Split",
                    mode == CenterMode::Split,
                    cx,
                )
                .on_click({
                    let center = center.clone();
                    move |_, _, cx| {
                        center.update(cx, |center, cx| center.set_view_mode(CenterMode::Split, cx));
                    }
                }),
            )
            .child(
                crate::ui::style::agent_detail_tab(
                    "center-mode-files",
                    lucide_icons::Icon::File,
                    "Files",
                    mode == CenterMode::Files,
                    cx,
                )
                .on_click({
                    let center = center.clone();
                    move |_, _, cx| {
                        center.update(cx, |center, cx| center.set_view_mode(CenterMode::Files, cx));
                    }
                }),
            )
            .child(
                crate::ui::style::agent_detail_tab(
                    "center-mode-terminal",
                    lucide_icons::Icon::SquareTerminal,
                    "Terminal",
                    mode == CenterMode::Terminal,
                    cx,
                )
                .on_click({
                    let center = center.clone();
                    move |_, _, cx| {
                        center.update(cx, |center, cx| {
                            center.set_view_mode(CenterMode::Terminal, cx)
                        });
                    }
                }),
            )
            .into_any_element()
    }

    pub(super) fn render_code_terminal_action(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        crate::ui::style::ghost_button_compact("editor-new-terminal", "Terminal")
            .icon(IconName::Plus)
            .tooltip("Open terminal beside the file")
            .on_click(cx.listener(|this, _, window, cx| {
                this.spawn_shell(window, cx);
            }))
            .into_any_element()
    }

    pub(super) fn render_docs_terminal_mode_switch(
        &self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mode = self.docs_terminal_mode;
        let center = cx.entity().clone();
        h_flex()
            .w_full()
            .h(crate::ui::design::header_h())
            .px_3()
            .items_center()
            .border_t_1()
            .border_b_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::nav(cx))
            .child(
                crate::ui::style::segmented_container_quiet(cx)
                    .child(
                        crate::ui::style::segment(
                            "docs-terminal-mode-assistant",
                            IconName::Bot,
                            "Assistant",
                            mode == DocsTerminalMode::Assistant,
                            cx,
                        )
                        .on_click({
                            let center = center.clone();
                            move |_, _, cx| {
                                center.update(cx, |center, cx| {
                                    center.docs_terminal_mode = DocsTerminalMode::Assistant;
                                    cx.notify();
                                });
                            }
                        }),
                    )
                    .child(
                        crate::ui::style::segment(
                            "docs-terminal-mode-implementor",
                            IconName::SquareTerminal,
                            "Implementor",
                            mode == DocsTerminalMode::Implementor,
                            cx,
                        )
                        .on_click({
                            let center = center.clone();
                            move |_, _, cx| {
                                center.update(cx, |center, cx| {
                                    center.docs_terminal_mode = DocsTerminalMode::Implementor;
                                    cx.notify();
                                });
                            }
                        }),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn render_agent_detail_switch(
        &self,
        agent: &AgentRecord,
        detail_tab: AgentDetailTab,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let is_chat_agent = agent.runtime == AgentRuntimeKind::Chat;
        let usage = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .and_then(|session| session.usage.as_ref())
            .cloned();
        let has_plan = is_chat_agent
            && self
                .agent_chats
                .read(cx)
                .session(agent_id)
                .and_then(|session| session.proposed_plan.as_ref())
                .is_some();
        let has_diff = self.agent_diff_drawers.contains_key(&agent_id);
        crate::ui::style::agent_detail_tabs()
            .when_some(usage.as_ref(), |switch, usage| {
                switch.child(self.render_agent_footer_usage(agent_id, agent.provider, usage, cx))
            })
            .when(agent_detail_shows_terminal(agent.runtime), |switch| {
                switch.child(
                    crate::ui::style::agent_detail_tab(
                        ("agent-terminal-toggle", agent_id.as_u128() as u64),
                        lucide_icons::Icon::Code,
                        "Terminal",
                        detail_tab == AgentDetailTab::Terminal,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.agent_detail_tabs
                            .insert(agent_id, AgentDetailTab::Terminal);
                        cx.notify();
                    })),
                )
            })
            .child(
                crate::ui::style::agent_detail_tab(
                    ("agent-files-toggle", agent_id.as_u128() as u64),
                    lucide_icons::Icon::Folder,
                    "Files",
                    detail_tab == AgentDetailTab::Files,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let next =
                        if this.agent_detail_tabs.get(&agent_id) == Some(&AgentDetailTab::Files) {
                            AgentDetailTab::Terminal
                        } else {
                            AgentDetailTab::Files
                        };
                    this.agent_detail_tabs.insert(agent_id, next);
                    cx.notify();
                })),
            )
            .child(
                crate::ui::style::agent_detail_tab(
                    ("agent-notes-toggle", agent_id.as_u128() as u64),
                    lucide_icons::Icon::BookOpen,
                    "Notes",
                    detail_tab == AgentDetailTab::Notes,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let next =
                        if this.agent_detail_tabs.get(&agent_id) == Some(&AgentDetailTab::Notes) {
                            AgentDetailTab::Terminal
                        } else {
                            AgentDetailTab::Notes
                        };
                    this.agent_detail_tabs.insert(agent_id, next);
                    cx.notify();
                })),
            )
            .when(has_diff, |switch| {
                switch.child(
                    crate::ui::style::agent_detail_tab(
                        ("agent-diff-toggle", agent_id.as_u128() as u64),
                        lucide_icons::Icon::FileDiff,
                        "Diff",
                        detail_tab == AgentDetailTab::Diff,
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let next = if this.agent_detail_tabs.get(&agent_id)
                            == Some(&AgentDetailTab::Diff)
                        {
                            AgentDetailTab::Terminal
                        } else {
                            AgentDetailTab::Diff
                        };
                        this.agent_detail_tabs.insert(agent_id, next);
                        cx.notify();
                    })),
                )
            })
            .when(has_plan, |switch| {
                switch.child(
                    div()
                        .relative()
                        .child(
                            crate::ui::style::agent_detail_tab(
                                ("agent-plan-toggle", agent_id.as_u128() as u64),
                                lucide_icons::Icon::CheckSquare,
                                "Plan",
                                detail_tab == AgentDetailTab::Plan,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    let next = if this.agent_detail_tabs.get(&agent_id)
                                        == Some(&AgentDetailTab::Plan)
                                    {
                                        AgentDetailTab::Terminal
                                    } else {
                                        AgentDetailTab::Plan
                                    };
                                    this.agent_detail_tabs.insert(agent_id, next);
                                    cx.notify();
                                },
                            )),
                        )
                        .child(crate::ui::onboarding::target_marker(
                            crate::ui::onboarding::SpotlightTarget::AgentPlan,
                            cx,
                        )),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_chat_agents_never_offer_a_terminal_tab() {
        assert!(!agent_detail_shows_terminal(AgentRuntimeKind::Chat));
        assert!(agent_detail_shows_terminal(AgentRuntimeKind::Terminal));
    }
}
