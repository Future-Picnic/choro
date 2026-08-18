use super::agent_naming::{implementation_agent_title, initial_agent_title};
use super::*;

fn update_new_agent_draft(
    drafts: &mut HashMap<ProjectId, String>,
    project: ProjectId,
    value: String,
) {
    if value.is_empty() {
        drafts.remove(&project);
    } else {
        drafts.insert(project, value);
    }
}

impl CenterArea {
    pub fn open_agent(&mut self, agent_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        self.new_agent_composer = None;
        self.agents
            .update(cx, |agents, cx| agents.select(project, agent_id, cx));
        crate::notifications::acknowledge_agent(project, agent_id);
        self.agent_detail_tabs
            .insert(agent_id, AgentDetailTab::Terminal);
        // Opening an agent is a content selection. Keep the user's current
        // Agents sidebar tool (for example, Board) unchanged.
        self.set_view_mode(CenterMode::Agents, cx);
        let opened_chat = self
            .agents
            .read(cx)
            .agent(agent_id)
            .is_some_and(|agent| agent.runtime == AgentRuntimeKind::Chat);
        if opened_chat {
            let opened_chat_session_id = self.agents.read(cx).agent(agent_id).and_then(|agent| {
                self.agent_chats
                    .read(cx)
                    .session(agent_id)
                    .and_then(|session| {
                        session
                            .chat_session_id
                            .clone()
                            .or_else(|| session.cli_session_id.clone())
                    })
                    .or_else(|| {
                        agent
                            .chat_session_id
                            .clone()
                            .or_else(|| agent.cli_session_id.clone())
                    })
            });
            if let Some(session_id) = opened_chat_session_id {
                self.terminals.update(cx, |manager, cx| {
                    manager.acknowledge_attention(&session_id, cx)
                });
            }
            cx.notify();
            return;
        }
        if let Some(id) = self
            .terminals
            .read(cx)
            .agent_record_terminal(project, agent_id)
        {
            self.focus_agent_terminal(project, id, window, cx);
            return;
        }
        cx.notify();
    }

    pub fn open_new_agent_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let project = {
            let workspace = self.workspace.read(cx);
            workspace
                .active
                .or_else(|| workspace.projects.first().map(|project| project.id))
        };
        let Some(project) = project else {
            return;
        };
        self.open_new_agent_composer_for_project(project, window, cx);
        if crate::ui::onboarding::expects_first_agent(project, cx) {
            let prompt = crate::onboarding::manifest().first_agent.prompt.clone();
            if let Some(composer) = self.new_agent_composer.as_mut() {
                composer.prompt.update(cx, |input, cx| {
                    input.set_value(prompt.clone(), window, cx);
                    input.set_cursor_position(
                        input_position_for_byte_offset(&prompt, prompt.len()),
                        window,
                        cx,
                    );
                    input.focus(window, cx);
                });
                composer.interaction_mode = AgentInteractionMode::Default;
                composer.error = None;
            }
            crate::ui::onboarding::emit_for_project(
                project,
                crate::ui::onboarding::OnboardingEvent::NewAgentOpened,
                cx,
            );
            cx.notify();
        }
    }

    pub fn open_new_agent_composer_for_project(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.project_by_id(project, cx).is_none() {
            return;
        }
        let agent_defaults = self.workspace.read(cx).new_agent_defaults();
        let default_provider = agent_defaults.provider;
        let default_model = agent_defaults.model;
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        let saved_draft = self
            .new_agent_drafts
            .get(&project)
            .cloned()
            .unwrap_or_default();
        let prompt = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(4, 8)
                .placeholder("Do anything — / skills, @ files, @@ docs, ## projects")
        });
        prompt.update(cx, |input, cx| {
            if !saved_draft.is_empty() {
                input.set_value(saved_draft.clone(), window, cx);
                input.set_cursor_position(
                    input_position_for_byte_offset(&saved_draft, saved_draft.len()),
                    window,
                    cx,
                );
            }
            input.focus(window, cx);
        });
        // Keep the doc-mention picker live as the prompt is edited: re-render on
        // every change and reset the highlight/dismissal so a narrowing query
        // always starts at the top and a dismissed picker reopens.
        cx.subscribe(&prompt, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let mut draft_update = None;
                if let Some(composer) = this.new_agent_composer.as_mut() {
                    composer.slash_selection = 0;
                    composer.project_mention_selected = 0;
                    composer.doc_mention_selected = 0;
                    composer.doc_mention_dismissed_query = None;
                    composer.file_mention_selected = 0;
                    composer.file_mention_dismissed_query = None;
                    let value = composer.prompt.read(cx).value().to_string();
                    let active_query = agent_chat_slash_query(&value).map(|query| query.query);
                    if composer.slash_dismissed_query.as_ref() != active_query.as_ref() {
                        composer.slash_dismissed_query = None;
                    }
                    let project_query = active_composer_project_mention(&composer.prompt.read(cx))
                        .map(|mention| mention.query);
                    if composer.project_mention_dismissed_query.as_ref() != project_query.as_ref() {
                        composer.project_mention_dismissed_query = None;
                    }
                    if composer.selected_command.as_ref().is_some_and(|command| {
                        !command.is_choro_riff()
                            && !value.trim_start().starts_with(command.invocation.trim())
                    }) {
                        composer.selected_command = None;
                    }
                    if composer.preview_suggestion_dismissed.as_ref() != Some(&value) {
                        composer.preview_suggestion_dismissed = None;
                    }
                    if composer.preview_suggestion_dismissed.as_ref() != Some(&value)
                        && choro_preview_intent(&value) == ChoroPreviewIntent::Automatic
                    {
                        composer.preview_armed = true;
                    }
                    draft_update = Some((composer.project, value));
                }
                if let Some((project, value)) = draft_update {
                    update_new_agent_draft(&mut this.new_agent_drafts, project, value);
                }
                cx.notify();
            }
        })
        .detach();
        let repositories = self.git_states.read(cx).repositories(project);
        let repository_path =
            (repositories.len() == 1).then(|| repositories[0].read(cx).repo_path.clone());
        self.new_agent_composer = Some(NewAgentComposer {
            project,
            repository_path,
            prompt,
            provider: default_provider,
            runtime: AgentRuntimeKind::Chat,
            interaction_mode: AgentInteractionMode::Default,
            model: default_model,
            external_model_id: agent_defaults.external_model_id,
            external_model_label: agent_defaults.external_model_label,
            external_model_variants: Vec::new(),
            effort: agent_defaults.effort,
            access_mode: AgentAccessMode::FullAccess,
            linked_docs: Vec::new(),
            selected_command: None,
            solo: false,
            lane_profile: ide_core::LaneProfile::Full,
            solo_base: None,
            preview_armed: false,
            preview_suggestion_dismissed: None,
            selected_mentions: Vec::new(),
            attached_files: Vec::new(),
            attachment_pastes_pending: 0,
            suggested_title: None,
            source_doc: None,
            linked_tasks: Vec::new(),
            source_task: None,
            implementation_design: None,
            design_browser_open_confirmed: false,
            error: None,
            slash_selection: 0,
            slash_dismissed_query: None,
            doc_mention_selected: 0,
            doc_mention_dismissed_query: None,
            file_mention_selected: 0,
            file_mention_dismissed_query: None,
            project_mention_selected: 0,
            project_mention_dismissed_query: None,
        });
        if let (Some(defaults), Some(composer)) = (
            crate::ui::onboarding::agent_defaults(project, cx),
            self.new_agent_composer.as_mut(),
        ) {
            composer.provider = defaults.provider;
            composer.model = defaults.model;
            composer.effort = defaults.effort;
            composer.external_model_id = defaults.external_model_id;
            composer.external_model_label = defaults.external_model_label;
            composer.external_model_variants.clear();
        }
        self.composer_branch_query
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.composer_model_query
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.composer_branch_expanded = false;
        self.refresh_open_code_models(false, cx);
        self.set_view_mode(CenterMode::Agents, cx);
        cx.notify();
    }

    pub(super) fn open_implementation_agent_for_doc(
        &mut self,
        project: ProjectId,
        relative_doc_path: PathBuf,
        doc_title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let linked_designs = self
            .penpot
            .read(cx)
            .designs_for_doc(project, &relative_doc_path);
        let doc_path = relative_doc_path.to_string_lossy().to_string();
        let prompt = format!(
            "Implement the work described in @@{doc_path}.\n\nRead that doc first and use it as the source of truth. Make the needed code changes, update or add focused tests where they matter, and keep the implementation aligned with the doc. If the doc is ambiguous or conflicts with the codebase, stop and ask before making broad changes."
        );

        self.open_new_agent_composer_for_project(project, window, cx);

        let Some(composer) = self.new_agent_composer.as_mut() else {
            return;
        };
        if composer.attachment_pastes_pending > 0 {
            composer.error = Some("Wait for the image to finish attaching.".into());
            cx.notify();
            return;
        }
        composer.prompt.update(cx, |input, cx| {
            input.set_value(prompt.clone(), window, cx);
            input.set_cursor_position(
                input_position_for_byte_offset(&prompt, prompt.len()),
                window,
                cx,
            );
            input.focus(window, cx);
        });
        composer.linked_docs.clear();
        composer.linked_docs.push(relative_doc_path.clone());
        composer.suggested_title = Some(implementation_agent_title(&doc_title, "doc"));
        composer.source_doc = Some(relative_doc_path);
        if crate::ui::onboarding::defaults_doc_agent_to_plan(cx) {
            composer.interaction_mode = AgentInteractionMode::Plan;
        }
        composer.error = None;
        composer.doc_mention_selected = 0;
        composer.doc_mention_dismissed_query = None;
        composer.file_mention_selected = 0;
        composer.file_mention_dismissed_query = None;
        self.add_design_mentions_to_new_agent(linked_designs, cx);
        crate::ui::onboarding::emit_for_project(
            project,
            crate::ui::onboarding::OnboardingEvent::DocImplementOpened,
            cx,
        );
        cx.notify();
    }

    pub(super) fn start_new_agent_composer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.project)
        else {
            return;
        };
        let Some((project, workspace_root)) = self.project_by_id(project, cx) else {
            if let Some(composer) = self.new_agent_composer.as_mut() {
                composer.error = Some("Choose a project before starting.".into());
            }
            cx.notify();
            return;
        };
        let projects = self.workspace.read(cx).projects.clone();
        let Some(composer) = self.new_agent_composer.as_mut() else {
            return;
        };
        let draft = composer.prompt.read(cx).value().trim().to_string();
        let selected_mentions = composer.selected_mentions.clone();
        let raw_doc = composer_mentions_submission_text(&draft, &selected_mentions, &projects);
        let raw_doc = agent_chat_submission_text(&raw_doc, composer.selected_command.as_ref());
        if raw_doc.is_empty() {
            composer.error = Some("Describe what the agent should do first.".into());
            cx.notify();
            return;
        }
        let preview_armed = composer.runtime == AgentRuntimeKind::Chat && composer.preview_armed;
        let raw_doc = preview_submission_text(&raw_doc, preview_armed);
        let raw_doc = memory_save_submission_text(&raw_doc);
        // Choro memory rides the first turn of every new agent — one budgeted
        // block, identical across Claude, Codex, and OpenCode.
        let (raw_doc, memory_ids) = memory_submission_text(&raw_doc, project);
        if !memory_ids.is_empty() {
            // Stamp usage off the UI thread; purely bookkeeping.
            cx.background_executor()
                .spawn(async move {
                    if let Ok(store) = ide_core::local_store::LocalStore::open_default() {
                        let _ = store.touch_memories_last_used(&memory_ids);
                    }
                })
                .detach();
        }
        let title = composer.suggested_title.clone().unwrap_or_else(|| {
            initial_agent_title(
                &draft,
                composer
                    .selected_command
                    .as_ref()
                    .map(|command| command.title.as_str()),
            )
        });
        let provider = composer.provider;
        let runtime = composer.runtime;
        let interaction_mode = composer.interaction_mode;
        let model = if composer.model.belongs_to(provider) {
            composer.model
        } else {
            AgentModel::default_for(provider)
        };
        let effort = composer.effort;
        let external_model_id = composer.external_model_id.clone();
        let external_model_label = composer.external_model_label.clone();
        let external_model_variants = composer.external_model_variants.clone();
        let access_mode = composer.access_mode;
        if provider == AgentKind::OpenCode && external_model_id.is_none() {
            composer.error = Some("Choose a model from your OpenCode installation first.".into());
            cx.notify();
            return;
        }
        let solo = composer.solo;
        let repository_path = composer.repository_path.clone();
        let lane_profile = composer.lane_profile;
        let solo_base = composer.solo_base.clone();
        let git_root = repository_path.as_deref().unwrap_or(&workspace_root);
        if solo && repository_path.is_none() {
            composer.error = Some("Choose a repository for Solo first.".into());
            cx.notify();
            return;
        }
        if solo && ide_core::git::read_head(git_root).is_err() {
            composer.error = Some("Solo needs a git repository with at least one commit.".into());
            cx.notify();
            return;
        }
        let message_display_text = composer_message_display_text(
            &draft,
            composer.selected_command.as_ref(),
            &selected_mentions,
        );
        let message_tags = composer_message_tags(
            composer.selected_command.as_ref(),
            &selected_mentions,
            preview_armed,
        );
        let linked_docs = composer.linked_docs.clone();
        let linked_tasks = composer.linked_tasks.clone();
        let attached_files = composer.attached_files.clone();
        let implementation_design = composer.implementation_design;
        let design_browser_open_confirmed = composer.design_browser_open_confirmed;
        let doc = prompt_with_attached_files(&raw_doc, &attached_files);
        let doc = if solo {
            format!(
                "{doc}\n\nChoro Docs available under `{DOCS_DIR_NAME}/` are read-only snapshots of the canonical project documents. Read them as source-of-truth context, but do not change their permissions or edit them in this Solo worktree. If a document itself should change, describe the proposed update explicitly so it can be reviewed against the canonical document."
            )
        } else {
            doc
        };
        let source_doc = composer
            .source_doc
            .clone()
            .filter(|source_doc| linked_docs.contains(source_doc));
        let source_task = composer
            .source_task
            .clone()
            .filter(|source_task| linked_tasks.iter().any(|task| task.same_issue(source_task)));
        if let Some(design_id) = implementation_design {
            let prompt_dismissed = self.workspace.read(cx).design_browser_open_prompt_dismissed;
            if !design_browser_open_confirmed && !prompt_dismissed {
                if let Some(design) = self.penpot.read(cx).design(project, design_id) {
                    self.confirm_design_browser_open(
                        project,
                        design_id,
                        design.name.into(),
                        window,
                        cx,
                    );
                    return;
                }
            }
            self.open_design_browser_for_agent(project, design_id, cx);
        }
        let onboarding_source = if source_doc.is_some() {
            crate::ui::onboarding::AgentSource::Doc
        } else if source_task.is_some() {
            crate::ui::onboarding::AgentSource::Task
        } else {
            crate::ui::onboarding::AgentSource::FirstPrompt
        };
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        // The base is captured while `cwd` is still ours. The branch name is
        // assigned after agent creation so its id acts as a collision-free
        // reservation even before the asynchronous worktree exists.
        let solo_base_branch = if solo {
            solo_base.or_else(|| {
                ide_core::git::read_head(git_root)
                    .ok()
                    .and_then(|head| head.branch)
            })
        } else {
            None
        };
        let agent_id = self.agents.update(cx, |agents, cx| {
            let agent_id = agents.create_agent(
                project,
                workspace_root,
                repository_path.clone(),
                title.clone(),
                doc.clone(),
                provider,
                runtime,
                model,
                effort,
                access_mode,
                linked_docs,
                source_doc.clone(),
                linked_tasks,
                source_task,
                AgentStatus::InProgress,
                cx,
            );
            if provider == AgentKind::OpenCode {
                if let (Some(id), Some(label)) =
                    (external_model_id.clone(), external_model_label.clone())
                {
                    agents.update_external_model(
                        agent_id,
                        id,
                        label,
                        external_model_variants.clone(),
                        cx,
                    );
                }
            }
            if solo {
                let branch = ide_core::lanes::solo_branch_name(&title, agent_id);
                agents.configure_solo(agent_id, branch, solo_base_branch, lane_profile, cx);
            }
            agent_id
        });
        // The provider can call Choro MCP tools on its first turn. Capture the
        // scoped record now, persist it away from GPUI, and only then start the
        // backend so MCP tools cannot race the normal debounced agent save.
        let (save_revision, agent_store) = self
            .agents
            .update(cx, |agents, _| agents.durable_snapshot());
        self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(agent_id, title.clone(), cx);
            session.interaction_mode = interaction_mode;
            chats.append_message(
                agent_id,
                AgentChatMessage::User {
                    text: doc,
                    display_text: Some(message_display_text),
                    tags: message_tags,
                    created_at: unix_now_secs(),
                },
                cx,
            );
        });
        self.new_agent_drafts.remove(&project);
        self.new_agent_composer = None;
        cx.notify();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let persisted = cx
                .background_executor()
                .spawn(async move {
                    crate::state::agents::persist_agent_store_snapshot(save_revision, agent_store)
                })
                .await;
            if let Err(error) = persisted {
                this.update(cx, |this, cx| {
                    let message = format!("Could not save agent before starting: {error:#}");
                    eprintln!("{message}");
                    this.agent_start_errors.insert(agent_id, message);
                    cx.notify();
                })
                .ok();
                return;
            }

            // A Solo's start is deferred behind lane setup and needs no
            // window; every other runtime starts after the durable write.
            if solo {
                this.update(cx, |this, cx| {
                    let started =
                        this.ensure_solo_lane_then_start(agent_id, CenterMode::Agents, cx);
                    if started {
                        crate::ui::onboarding::emit_for_project(
                            project,
                            crate::ui::onboarding::OnboardingEvent::AgentStarted {
                                id: agent_id,
                                source: onboarding_source,
                            },
                            cx,
                        );
                        if let Some(source_doc) = source_doc {
                            if let Err(error) = this.docs.update(cx, |docs, cx| {
                                docs.set_doc_implementor(project, &source_doc, Some(agent_id), cx)
                            }) {
                                eprintln!("failed to set doc implementor: {error:#}");
                            }
                        }
                    }
                })
                .ok();
            } else {
                window_handle
                    .update(cx, |_, window, cx| {
                        this.update(cx, |this, cx| {
                            let started =
                                this.start_agent_in_mode(agent_id, CenterMode::Agents, window, cx);
                            if started {
                                crate::ui::onboarding::emit_for_project(
                                    project,
                                    crate::ui::onboarding::OnboardingEvent::AgentStarted {
                                        id: agent_id,
                                        source: onboarding_source,
                                    },
                                    cx,
                                );
                                if let Some(source_doc) = source_doc {
                                    if let Err(error) = this.docs.update(cx, |docs, cx| {
                                        docs.set_doc_implementor(
                                            project,
                                            &source_doc,
                                            Some(agent_id),
                                            cx,
                                        )
                                    }) {
                                        eprintln!("failed to set doc implementor: {error:#}");
                                    }
                                }
                            }
                        })
                        .ok();
                    })
                    .ok();
            }
        })
        .detach();
    }

    pub(super) fn paste_image_into_new_agent_composer(
        &mut self,
        report_missing: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let _slow_operation =
            crate::ui::performance::UiOperationTimer::start("new_agent.clipboard_read");
        let Some(project) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.project)
        else {
            return false;
        };
        let Some(image) = cx.read_from_clipboard().and_then(clipboard_image_from_item) else {
            if report_missing {
                if let Some(composer) = self.new_agent_composer.as_mut() {
                    composer.error = Some("Copy an image first, then paste it here.".into());
                }
                cx.notify();
            }
            return false;
        };
        if self.project_by_id(project, cx).is_none() {
            if let Some(composer) = self.new_agent_composer.as_mut() {
                composer.error = Some("Choose a project before attaching images.".into());
            }
            cx.notify();
            return false;
        }

        if let Some(composer) = self.new_agent_composer.as_mut() {
            composer.attachment_pastes_pending += 1;
            composer.error = None;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { materialize_project_clipboard_image(project, &image) })
                .await;
            this.update(cx, |this, cx| {
                let Some(composer) = this
                    .new_agent_composer
                    .as_mut()
                    .filter(|composer| composer.project == project)
                else {
                    return;
                };
                composer.attachment_pastes_pending =
                    composer.attachment_pastes_pending.saturating_sub(1);
                match result {
                    Ok(path) => {
                        composer.attached_files.push(path);
                        composer.error = None;
                    }
                    Err(error) => {
                        composer.error = Some(format!("Could not attach image: {error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        true
    }

    pub(super) fn attach_paths_to_new_agent_composer(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> bool {
        let dropped_files = paths
            .iter()
            .filter(|path| path.is_file())
            .cloned()
            .collect::<Vec<_>>();
        if dropped_files.is_empty() {
            return false;
        }

        if let Some(composer) = self.new_agent_composer.as_mut() {
            for path in dropped_files {
                if !composer
                    .attached_files
                    .iter()
                    .any(|existing| existing == &path)
                {
                    composer.attached_files.push(path);
                }
            }
            composer.error = None;
            cx.notify();
            true
        } else {
            false
        }
    }

    pub(super) fn insert_doc_mention_into_composer(
        &mut self,
        relative_path: PathBuf,
        mention: ComposerDocMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.prompt.clone())
        else {
            return;
        };
        let current = prompt.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = apply_composer_doc_mention(&current, &mention, &relative_path);
        prompt.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        if let Some(composer) = self.new_agent_composer.as_mut() {
            let title = relative_path
                .file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_string)
                .unwrap_or_else(|| relative_path.to_string_lossy().to_string());
            let token = ComposerMentionToken::doc(title, &relative_path);
            if !composer.selected_mentions.contains(&token) {
                composer.selected_mentions.push(token);
            }
            if !composer.linked_docs.contains(&relative_path) {
                composer.linked_docs.push(relative_path);
            }
            composer.doc_mention_selected = 0;
            composer.doc_mention_dismissed_query = None;
            composer.error = None;
        }
        cx.notify();
    }

    pub(super) fn insert_project_mention_into_composer(
        &mut self,
        project: ComposerProjectEntry,
        mention: ComposerProjectMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.prompt.clone())
        else {
            return;
        };
        let current = prompt.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = remove_composer_project_mention(&current, &mention);
        prompt.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        if let Some(composer) = self.new_agent_composer.as_mut() {
            let token = ComposerMentionToken::project_entry(&project);
            if !composer.selected_mentions.iter().any(|selected| {
                selected.kind == ComposerMentionKind::Project
                    && selected.project_id == token.project_id
            }) {
                composer.selected_mentions.push(token);
            }
            composer.project_mention_selected = 0;
            composer.project_mention_dismissed_query = None;
            composer.error = None;
        }
        cx.notify();
    }

    pub(super) fn insert_slash_command_into_composer(
        &mut self,
        command: AgentCapability,
        query: AgentChatSlashQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.prompt.clone())
        else {
            return;
        };
        let current = prompt.read(cx).value().to_string();
        if query.range.start > query.range.end || query.range.end > current.len() {
            return;
        }
        let (next, cursor) = remove_agent_chat_slash_query(&current, &query);
        let (next, cursor) = insert_agent_chat_command_invocation(&next, cursor, &command);
        prompt.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        if let Some(composer) = self.new_agent_composer.as_mut() {
            if command.is_choro_preview() {
                composer.preview_armed = true;
                composer.preview_suggestion_dismissed = None;
            } else {
                composer.selected_command = Some(command);
            }
            composer.slash_selection = 0;
            composer.slash_dismissed_query = None;
            composer.error = None;
        }
        cx.notify();
    }

    pub(super) fn insert_design_mention_into_composer(
        &mut self,
        reference: ProjectReference,
        mention: ComposerDocMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.prompt.clone())
        else {
            return;
        };
        let current = prompt.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let penpot_token = ComposerMentionToken::penpot_design(&reference);
        let preview = crate::state::designs::reference_absolute_preview_path(&reference)
            .filter(|path| path.is_file());
        let replacement = if let Some(token) = penpot_token.as_ref() {
            token.invocation()
        } else if preview.is_some() {
            String::new()
        } else {
            format!("{} ", reference.source)
        };
        let next = format!(
            "{}{}{}",
            &current[..mention.range.start],
            replacement,
            &current[mention.range.end..]
        );
        let cursor = mention.range.start + replacement.len();
        prompt.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        if let Some(composer) = self.new_agent_composer.as_mut() {
            if let Some(token) = penpot_token {
                if !composer.selected_mentions.contains(&token) {
                    composer.selected_mentions.push(token);
                }
            }
            if let Some(path) = preview {
                if !composer.attached_files.contains(&path) {
                    composer.attached_files.push(path);
                }
            }
            composer.doc_mention_selected = 0;
            composer.doc_mention_dismissed_query = None;
            composer.error = None;
        }
        cx.notify();
    }

    pub(super) fn insert_file_mention_into_composer(
        &mut self,
        file: ComposerFileEntry,
        mention: ComposerFileMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(prompt) = self
            .new_agent_composer
            .as_ref()
            .map(|composer| composer.prompt.clone())
        else {
            return;
        };
        let current = prompt.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = apply_composer_file_mention(&current, &mention, &file.relative_label);
        prompt.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        if let Some(composer) = self.new_agent_composer.as_mut() {
            let token = ComposerMentionToken::file(&file);
            if !composer.selected_mentions.contains(&token) {
                composer.selected_mentions.push(token);
            }
            composer.file_mention_selected = 0;
            composer.file_mention_dismissed_query = None;
            composer.error = None;
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_agent_drafts_are_kept_per_project_and_cleared_when_empty() {
        let first = ProjectId(Uuid::new_v4());
        let second = ProjectId(Uuid::new_v4());
        let mut drafts = HashMap::new();

        update_new_agent_draft(&mut drafts, first, "First project draft".into());
        update_new_agent_draft(&mut drafts, second, "Second project draft".into());

        assert_eq!(
            drafts.get(&first).map(String::as_str),
            Some("First project draft")
        );
        assert_eq!(
            drafts.get(&second).map(String::as_str),
            Some("Second project draft")
        );

        update_new_agent_draft(&mut drafts, first, String::new());

        assert!(!drafts.contains_key(&first));
        assert_eq!(
            drafts.get(&second).map(String::as_str),
            Some("Second project draft")
        );
    }
}
