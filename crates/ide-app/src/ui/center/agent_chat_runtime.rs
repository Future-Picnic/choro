#![allow(dead_code, reason = "retained agent-chat command paths")]

use super::*;

/// The prompt the "Code review" composer button sends. Modelled on how Claude
/// Code's `/review` and Codex's review do it, plus AI-review best practices:
/// senior-engineer framing, a strict severity rubric, an evidence requirement
/// (every claim cites `path:line`) to cut false positives, skip empty
/// categories, and demand actionable fixes — all inside a parseable block.
///
/// `pub(super)` so the timeline can recognise this exact turn and render it as a
/// compact "Sent for code review" chip instead of echoing the whole prompt.
pub(super) const AGENT_CODE_REVIEW_PROMPT: &str = ide_core::config::DEFAULT_CODE_REVIEW_PROMPT;
pub(super) const AGENT_CODE_REVIEW_REQUEST_MARKER: &str = "<!-- choro:code-review -->";

/// Stable opening of the "Fix all" / "Fix selected" turns. The findings to fix
/// are appended after it, so the timeline recognises the turn by this prefix and
/// renders it as a chip rather than echoing the instruction.
pub(super) const AGENT_CODE_REVIEW_FIX_PREFIX: &str =
    "Apply the fixes for these code-review findings";

/// Marks the hidden verification directive turn: the agent is asked to check
/// its finished work against the written intent (doc / plan / task), so the
/// timeline collapses the turn to a "Sent for verification" chip.
pub(super) const AGENT_VERIFY_REQUEST_MARKER: &str = "<!-- choro:verify -->";

/// Records that the user declined verification for this agent. Unlike the
/// transient decision panel, this marker is persisted with the timeline so
/// later turns and app restarts do not revive either verification prompt.
pub(super) const AGENT_VERIFY_DISMISS_MARKER: &str = "<!-- choro:verify-dismissed -->";

/// Historical marker written by releases that only persisted dismissal of the
/// follow-up verification prompt. Keep recognising it in resumed transcripts.
pub(super) const AGENT_REVERIFY_DISMISS_MARKER: &str = "<!-- choro:reverify-dismissed -->";

/// Stable opening of the "Ask to fix" turn that lists a verification's unmet
/// requirements back to the agent. Recognised by prefix, rendered as a chip.
pub(super) const AGENT_VERIFY_FIX_PREFIX: &str =
    "Address these unmet requirements from the verification";

/// The longest task-description excerpt re-injected into a verification
/// directive. Docs are referenced (not inlined) and plans are already bounded;
/// tracker descriptions are the one unbounded input.
const VERIFY_TASK_DESCRIPTION_MAX_CHARS: usize = 4000;

impl CenterArea {
    /// One shared stop path for both the composer stop control and Escape.
    /// `AgentChatState::stop_backend` escalates a second request while the
    /// session is cancelling into a force-stop of the backend process.
    pub(super) fn request_agent_chat_stop(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        self.sync_chat_session_ids(cx);
        self.agent_chats
            .update(cx, |chats, cx| chats.stop_backend(agent_id, cx));
        cx.notify();
    }

    pub fn stop_selected_agent(&mut self, cx: &mut Context<Self>) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let Some(agent_id) = self.agents.read(cx).selected_agent_id(project) else {
            return;
        };
        self.request_agent_chat_stop(agent_id, cx);
    }

    pub(super) fn update_agent_chat_model_effort(
        &mut self,
        agent_id: Uuid,
        model: AgentModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        let effort = self
            .agents
            .read(cx)
            .agent(agent_id)
            .map(|agent| agent.normalize_effort_for_model(model, effort))
            .unwrap_or_else(|| model.normalize_effort(effort));
        let active_project = self.active_project(cx).map(|(project, _)| project);
        if let Some(project) = active_project {
            if let Some(id) = self
                .terminals
                .read(cx)
                .agent_record_terminal(project, agent_id)
            {
                self.terminals.update(cx, |terminals, cx| {
                    terminals.close(id, cx);
                });
            }
        }
        self.agents.update(cx, |agents, cx| {
            agents.update_model_effort(agent_id, model, effort, cx);
        });
        self.agent_chats.update(cx, |chats, cx| {
            chats.update_model_effort(agent_id, model, effort, cx);
        });
        cx.notify();
    }

    pub(super) fn update_agent_chat_surface_model_effort(
        &mut self,
        surface: &AgentChatSurface,
        agent_id: Uuid,
        provider: AgentKind,
        model: AgentModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        let (project, relative_doc_path) = match surface {
            AgentChatSurface::Standard => {
                self.update_agent_chat_model_effort(agent_id, model, effort, cx);
                return;
            }
            AgentChatSurface::Document {
                project,
                relative_doc_path,
            } => (*project, relative_doc_path.clone()),
            AgentChatSurface::Design {
                project,
                relative_doc_path,
                ..
            } => (*project, relative_doc_path.clone()),
        };
        let Some(record) = self
            .doc_assistants
            .read(cx)
            .record_for(project, &relative_doc_path)
        else {
            return;
        };
        let provider_changed = record.provider != provider;
        if provider_changed {
            let provider_locked = self.agent_chats.read(cx).has_backend(agent_id)
                || self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .is_some_and(|session| {
                        session.chat_session_id.is_some()
                            || session.cli_session_id.is_some()
                            || !session.messages.is_empty()
                    });
            if provider_locked {
                return;
            }
        }
        let effort = if provider == AgentKind::OpenCode {
            let supported = AgentEffort::supported_variants(&record.external_model_variants);
            if supported.is_empty() || supported.contains(&effort) {
                effort
            } else if supported.contains(&AgentEffort::High) {
                AgentEffort::High
            } else {
                supported.first().copied().unwrap_or(AgentEffort::Medium)
            }
        } else {
            model.normalize_effort(effort)
        };
        self.doc_assistants.update(cx, |assistants, cx| {
            assistants.update_runtime(project, &relative_doc_path, provider, model, effort, cx);
        });
        if let Some(record) = self
            .doc_assistants
            .read(cx)
            .record_for(project, &relative_doc_path)
        {
            self.persist_design_assistant_runtime(surface, &record, cx);
        }
        if !provider_changed {
            self.agent_chats.update(cx, |chats, cx| {
                chats.update_model_effort(agent_id, model, effort, cx);
            });
        }
        cx.notify();
    }

    pub(super) fn update_agent_chat_surface_external_model(
        &mut self,
        surface: &AgentChatSurface,
        agent_id: Uuid,
        model: OpenCodeModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        let (project, relative_doc_path) = match surface {
            AgentChatSurface::Standard => return,
            AgentChatSurface::Document {
                project,
                relative_doc_path,
            }
            | AgentChatSurface::Design {
                project,
                relative_doc_path,
                ..
            } => (*project, relative_doc_path.clone()),
        };
        let Some(record) = self
            .doc_assistants
            .read(cx)
            .record_for(project, &relative_doc_path)
        else {
            return;
        };
        let provider_changed = record.provider != AgentKind::OpenCode;
        if provider_changed {
            let provider_locked = self.agent_chats.read(cx).has_backend(agent_id)
                || self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .is_some_and(|session| {
                        session.chat_session_id.is_some()
                            || session.cli_session_id.is_some()
                            || !session.messages.is_empty()
                    });
            if provider_locked {
                return;
            }
        }
        self.doc_assistants.update(cx, |assistants, cx| {
            assistants.update_external_model(
                project,
                &relative_doc_path,
                model.id,
                model.name,
                model.variants,
                effort,
                cx,
            );
        });
        if let Some(record) = self
            .doc_assistants
            .read(cx)
            .record_for(project, &relative_doc_path)
        {
            self.persist_design_assistant_runtime(surface, &record, cx);
        }
        cx.notify();
    }

    fn persist_design_assistant_runtime(
        &mut self,
        surface: &AgentChatSurface,
        record: &DocAssistantRecord,
        cx: &mut Context<Self>,
    ) {
        let AgentChatSurface::Design { design_id, .. } = surface else {
            return;
        };
        let persisted = ide_core::local_store::LocalStore::open_default().and_then(|store| {
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
        if let Err(error) = persisted {
            self.penpot_setup_error = Some(format!(
                "Could not save the Design Assistant settings: {error:#}"
            ));
            cx.notify();
            return;
        }
        if let Err(error) = self.penpot.update(cx, |penpot, cx| {
            penpot.refresh_conversations(*design_id, cx)
        }) {
            self.penpot_setup_error = Some(format!(
                "Could not refresh the Design Assistant settings: {error:#}"
            ));
        }
    }

    pub(super) fn update_agent_chat_surface_access_mode(
        &mut self,
        surface: &AgentChatSurface,
        agent_id: Uuid,
        access_mode: AgentAccessMode,
        cx: &mut Context<Self>,
    ) {
        match surface {
            AgentChatSurface::Standard => {
                self.agents.update(cx, |agents, cx| {
                    agents.update_access_mode(agent_id, access_mode, cx);
                });
            }
            AgentChatSurface::Document {
                project,
                relative_doc_path,
            } => {
                self.doc_assistants.update(cx, |assistants, cx| {
                    assistants.update_access_mode(*project, relative_doc_path, access_mode, cx);
                });
            }
            AgentChatSurface::Design {
                project,
                relative_doc_path,
                ..
            } => {
                self.doc_assistants.update(cx, |assistants, cx| {
                    assistants.update_access_mode(*project, relative_doc_path, access_mode, cx);
                });
            }
        }
        self.agent_chats.update(cx, |chats, cx| {
            chats.update_access_mode(agent_id, access_mode, cx);
        });
        if let Some(record) = self
            .doc_assistants
            .read(cx)
            .records()
            .iter()
            .find(|record| record.chat_agent_id == agent_id)
            .cloned()
        {
            self.persist_design_assistant_runtime(surface, &record, cx);
        }
        cx.notify();
    }

    pub(super) fn restart_agent_chat_connection(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        if agent.project_id != project {
            return;
        }

        if agent.runtime != AgentRuntimeKind::Chat {
            if let Some(id) = self
                .terminals
                .read(cx)
                .agent_record_terminal(project, agent_id)
            {
                if let Err(error) = self
                    .terminals
                    .update(cx, |manager, cx| manager.restart(id, cx))
                {
                    self.agent_start_errors
                        .insert(agent_id, format!("failed to restart terminal: {error:#}"));
                    cx.notify();
                    return;
                }
                if let Some(id) = self
                    .terminals
                    .read(cx)
                    .agent_record_terminal(project, agent_id)
                {
                    self.focus_agent_terminal(project, id, window, cx);
                }
            } else {
                self.start_agent_in_mode(agent_id, CenterMode::Agents, window, cx);
            }
            return;
        }

        self.cancel_agent_chat_hydration(agent_id);
        self.sync_chat_session_ids(cx);
        if let Some(id) = self
            .terminals
            .read(cx)
            .agent_record_terminal(project, agent_id)
        {
            self.terminals.update(cx, |terminals, cx| {
                terminals.close(id, cx);
            });
        }
        self.agent_chats
            .update(cx, |chats, cx| chats.force_stop_backend(agent_id, cx));
        self.agent_chat_selected_commands.remove(&agent_id);
        self.agent_chat_selected_mentions.remove(&agent_id);
        self.agent_chat_selected_agent_targets.remove(&agent_id);
        self.agent_chat_agent_request_kind_overrides
            .remove(&agent_id);
        self.agent_chat_slash_dismissed_query.remove(&agent_id);
        self.agent_chat_agent_dismissed_query.remove(&agent_id);
        self.agent_chat_project_dismissed_query.remove(&agent_id);
        self.agent_chat_doc_dismissed_query.remove(&agent_id);
        self.agent_chat_file_dismissed_query.remove(&agent_id);

        if self.start_chat_agent_in_mode(agent, CenterMode::Agents, cx) {
            self.agent_start_errors.remove(&agent_id);
        }
        cx.notify();
    }

    pub(super) fn agent_runtime(
        &self,
        agent: &AgentRecord,
        project: ProjectId,
        cx: &App,
    ) -> AgentRuntime {
        if agent.runtime == AgentRuntimeKind::Chat {
            let (status, session_id, last_activity_at) = self
                .agent_chats
                .read(cx)
                .session(agent.id)
                .map(|session| {
                    (
                        Some(session.status),
                        session
                            .chat_session_id
                            .clone()
                            .or_else(|| session.cli_session_id.clone()),
                        Some(session.last_activity_at),
                    )
                })
                .unwrap_or((None, None, None));
            return match status {
                Some(AgentChatStatus::Running | AgentChatStatus::Cancelling) => {
                    AgentRuntime::Working
                }
                Some(AgentChatStatus::WaitingForUser) => AgentRuntime::Waiting,
                Some(AgentChatStatus::PlanReady) => AgentRuntime::Waiting,
                Some(AgentChatStatus::Failed) => AgentRuntime::Ended,
                _ if agent.started_at.is_some() => {
                    let manager = self.terminals.read(cx);
                    if let Some(session_id) = session_id
                        .as_deref()
                        .or(agent.chat_session_id.as_deref())
                        .or(agent.cli_session_id.as_deref())
                    {
                        let updated_at = ide_core::agents::chat_updated_at(
                            agent.provider,
                            agent.runtime_path(),
                            session_id,
                        )
                        .or_else(|| {
                            last_activity_at.map(|secs| {
                                std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
                            })
                        });
                        if let Some(updated_at) = updated_at {
                            let working = std::time::SystemTime::now()
                                .duration_since(updated_at)
                                .map(|age| age < ide_core::agents::WORKING_WINDOW)
                                .unwrap_or(false);
                            if working {
                                return AgentRuntime::Working;
                            }
                            if !manager.attention_suppressed(session_id, updated_at) {
                                return AgentRuntime::Waiting;
                            }
                        }
                    }
                    AgentRuntime::Idle
                }
                _ => AgentRuntime::NotStarted,
            };
        }

        let manager = self.terminals.read(cx);
        if let Some(session) = manager.agent_record_session(project, agent.id) {
            if session.exited {
                return AgentRuntime::Ended;
            }
            let session_id = session
                .agent_session_id
                .as_deref()
                .or(agent.cli_session_id.as_deref());
            if let Some(session_id) = session_id {
                if let Some(updated_at) = ide_core::agents::chat_updated_at(
                    agent.provider,
                    agent.runtime_path(),
                    session_id,
                ) {
                    let working = std::time::SystemTime::now()
                        .duration_since(updated_at)
                        .map(|age| age < ide_core::agents::WORKING_WINDOW)
                        .unwrap_or(false);
                    if working {
                        return AgentRuntime::Working;
                    }
                    if !manager.attention_suppressed(session_id, updated_at) {
                        return AgentRuntime::Waiting;
                    }
                }
            }
            return AgentRuntime::Open;
        }
        if agent.started_at.is_some() || agent.cli_session_id.is_some() {
            AgentRuntime::Idle
        } else {
            AgentRuntime::NotStarted
        }
    }

    pub(super) fn agent_notes_input(
        &mut self,
        agent: &AgentRecord,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.agent_notes_inputs.get(&agent.id) {
            return input.clone();
        }

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("markdown")
                .auto_grow(4, 10)
                .placeholder("Add notes for this agent")
                .default_value(agent.notes.clone())
        });
        let input_for_sub = input.clone();
        let agents = self.agents.clone();
        let agent_id = agent.id;
        cx.subscribe(&input, move |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let notes = input_for_sub.read(cx).value().to_string();
                agents.update(cx, |agents, cx| agents.update_notes(agent_id, notes, cx));
            }
        })
        .detach();
        self.agent_notes_inputs.insert(agent.id, input.clone());
        input
    }

    pub(super) fn agent_chat_input(
        &mut self,
        agent: &AgentRecord,
        placeholder: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.agent_chat_inputs.get(&agent.id) {
            return input.clone();
        }

        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 8)
                .placeholder(placeholder)
        });
        let input_for_sub = input.clone();
        let agent_id = agent.id;
        cx.subscribe(&input, move |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.agent_chat_slash_selection.insert(agent_id, 0);
                this.agent_chat_project_selection.insert(agent_id, 0);
                this.agent_chat_agent_selection.insert(agent_id, 0);
                this.agent_chat_doc_selection.insert(agent_id, 0);
                this.agent_chat_file_selection.insert(agent_id, 0);
                let value = input_for_sub.read(cx).value().to_string();
                let active_query = agent_chat_slash_query(&value).map(|query| query.query);
                if this.agent_chat_slash_dismissed_query.get(&agent_id) != active_query.as_ref() {
                    this.agent_chat_slash_dismissed_query.remove(&agent_id);
                }
                let project_query = active_composer_project_mention(&input_for_sub.read(cx))
                    .map(|mention| mention.query);
                if this.agent_chat_project_dismissed_query.get(&agent_id) != project_query.as_ref()
                {
                    this.agent_chat_project_dismissed_query.remove(&agent_id);
                }
                let agent_query = active_composer_agent_mention(&input_for_sub.read(cx))
                    .map(|mention| mention.query);
                if this.agent_chat_agent_dismissed_query.get(&agent_id) != agent_query.as_ref() {
                    this.agent_chat_agent_dismissed_query.remove(&agent_id);
                }
                let doc_query = active_composer_doc_mention(&input_for_sub.read(cx))
                    .map(|mention| mention.query);
                if this.agent_chat_doc_dismissed_query.get(&agent_id) != doc_query.as_ref() {
                    this.agent_chat_doc_dismissed_query.remove(&agent_id);
                }
                let file_query = active_composer_file_mention(&input_for_sub.read(cx))
                    .map(|mention| mention.query);
                if this.agent_chat_file_dismissed_query.get(&agent_id) != file_query.as_ref() {
                    this.agent_chat_file_dismissed_query.remove(&agent_id);
                }
                if this.agent_chat_preview_suggestion_dismissed.get(&agent_id) != Some(&value) {
                    this.agent_chat_preview_suggestion_dismissed
                        .remove(&agent_id);
                }
                let preview_was_dismissed =
                    this.agent_chat_preview_suggestion_dismissed.get(&agent_id) == Some(&value);
                if !preview_was_dismissed
                    && choro_preview_intent(&value) == ChoroPreviewIntent::Automatic
                {
                    this.agent_chat_preview_armed.insert(agent_id);
                }
                cx.notify();
            }
        })
        .detach();
        self.agent_chat_inputs.insert(agent.id, input.clone());
        input
    }

    /// Mark this agent's chat as seen — answering it (sending a message,
    /// answering a question, implementing a plan) counts as acknowledging the
    /// "waiting for you" state, exactly like opening the chat does. Additive
    /// only; mirrors the acknowledge on navigation and changes no other logic.
    pub(super) fn acknowledge_agent_chat_seen(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let session_id = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .and_then(|session| {
                session
                    .chat_session_id
                    .clone()
                    .or_else(|| session.cli_session_id.clone())
            });
        if let Some(session_id) = session_id {
            self.terminals.update(cx, |manager, cx| {
                manager.acknowledge_attention(&session_id, cx)
            });
        }
    }

    /// Ask the active agent to review its uncommitted changes — the same skill
    /// Claude Code / Codex expose, triggered from inside our composer. Sends the
    /// canonical review prompt as a turn; the agent replies with the findings.
    pub(super) fn request_agent_code_review(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        let prompt = {
            let workspace = self.workspace.read(cx);
            format!(
                "{AGENT_CODE_REVIEW_REQUEST_MARKER}\n{}\n\n{}",
                workspace.effective_code_review_prompt(),
                workspace.effective_code_review_output_instructions(),
            )
        };
        self.dispatch_agent_chat_submission(agent_id, prompt, mode, cx);
        self.acknowledge_agent_chat_seen(agent_id, cx);
    }

    /// Ask the agent to apply fixes for a review's findings: the ticked ones when
    /// `only_selected`, otherwise all of them. The chosen findings are listed
    /// back to the agent so it fixes exactly those.
    pub(super) fn request_agent_code_review_fix(
        &mut self,
        agent_id: Uuid,
        review_id: String,
        only_selected: bool,
        cx: &mut Context<Self>,
    ) {
        let lines =
            self.agent_chats
                .read(cx)
                .code_review_fix_lines(agent_id, &review_id, only_selected);
        if lines.is_empty() {
            return;
        }
        let mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        let list = lines
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "{AGENT_CODE_REVIEW_FIX_PREFIX}:\n{list}\n\nFor each one, make the concrete change you suggested earlier. If a finding turns out to be a false positive on closer inspection, skip it and say why. When you're done, give a short summary of what you changed, grouped by file."
        );
        self.agent_chats.update(cx, |chats, cx| {
            chats.mark_code_review_findings_fix_requested(agent_id, &review_id, only_selected, cx);
        });
        self.dispatch_agent_chat_submission(agent_id, prompt, mode, cx);
        self.acknowledge_agent_chat_seen(agent_id, cx);
    }

    /// Ask the agent to verify its finished work against the stated intent —
    /// the source doc, the approved plan, and the linked tasks, all of which
    /// are re-injected verbatim so the check runs against what was actually
    /// asked, not the agent's memory of it. Falls back to the original first
    /// prompt when no more specific written intent is available.
    pub(super) fn request_agent_verification(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> bool {
        let lifecycle = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| verification_lifecycle(&session.timeline))
            .unwrap_or(VerificationLifecycle::NotStarted);
        let verification_closed = self
            .agents
            .read(cx)
            .agent(agent_id)
            .is_some_and(AgentRecord::is_verification_closed);
        if verification_closed
            || !matches!(
                lifecycle,
                VerificationLifecycle::NotStarted | VerificationLifecycle::Fixing
            )
        {
            return false;
        }
        let is_reverification = lifecycle == VerificationLifecycle::Fixing;
        let mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        let prompt = if is_reverification {
            format!(
                "{AGENT_VERIFY_REQUEST_MARKER}\nRe-verify only the clear unmet requirements from the immediately preceding verification-fix request. Check the actual code changed by the fix — not your memory of it.\n\nRespond inside a single <verification>…</verification> block, written as Markdown:\n- Use only `## Met` and `## Missed` headings.\n- Do not include an `Unclear` group or any unclear requirements in this follow-up result.\n- Under each heading, use one list item per requirement starting with `**short requirement**`, then ` — ` and one short line of evidence.\n- If you run a test, report the real result; never invent evidence.\n\nWrite nothing outside the <verification> block."
            )
        } else {
            let Some(bundle) = self.verification_intent_bundle(agent_id, cx) else {
                return false;
            };
            format!(
                "{AGENT_VERIFY_REQUEST_MARKER}\nYou just finished implementing. Verify your work against the original intent below, requirement by requirement. Check the actual state of the code you changed — not your memory of it.\n\n{bundle}\n\nRespond with your verification inside a single <verification>…</verification> block, written as Markdown:\n- Use a `## Met` / `## Unclear` / `## Missed` heading for each group that has requirements.\n- Under each, one list item per requirement starting with `**short requirement**`, then ` — ` and one short line of evidence (what you did and where).\n- Cover every requirement stated in the intent above; do not skip any.\n- If the project has a test command you already know, run it and report the real result — never invent evidence.\n\nWrite nothing outside the <verification> block."
            )
        };
        let sent = self.dispatch_agent_chat_submission(agent_id, prompt, mode, cx);
        if sent {
            self.acknowledge_agent_chat_seen(agent_id, cx);
        }
        sent
    }

    /// The written-intent sections for a verification directive, strongest
    /// first: source doc, approved plan, linked tasks — stacked, each labeled.
    /// Only when none exist does the original first prompt stand in. `None`
    /// when there is no intent of any kind to verify against.
    fn verification_intent_bundle(&self, agent_id: Uuid, cx: &App) -> Option<String> {
        let agent = self.agents.read(cx).agent(agent_id).cloned();
        let session = self.agent_chats.read(cx).session(agent_id);
        let mut sections: Vec<String> = Vec::new();

        if let Some(doc_path) = agent.as_ref().and_then(|agent| agent.source_doc.as_ref()) {
            sections.push(format!(
                "## The source doc (the contract)\nRe-read @@{} now and verify the work against exactly what it specifies.",
                doc_path.to_string_lossy()
            ));
        }

        if let Some(plan) = session
            .and_then(|session| session.proposed_plan.as_ref())
            .filter(|plan| plan.implemented_at.is_some())
        {
            sections.push(format!("## The approved plan\n{}", plan.markdown.trim()));
        }

        if let Some(agent) = agent.as_ref() {
            let mut task_refs: Vec<&TaskRef> = Vec::new();
            for task in agent.source_task.iter().chain(agent.linked_tasks.iter()) {
                if !task_refs.iter().any(|seen| seen.same_issue(task)) {
                    task_refs.push(task);
                }
            }
            let tasks = self.tasks.read(cx);
            let task_sections: Vec<String> = task_refs
                .iter()
                .map(|task| {
                    let description = tasks
                        .detail(task)
                        .map(|detail| detail.description.text)
                        .filter(|text| !text.trim().is_empty());
                    match description {
                        Some(text) => format!(
                            "**{} — {}**\n{}",
                            task.issue_key,
                            task.title,
                            bounded_verification_text(&text, VERIFY_TASK_DESCRIPTION_MAX_CHARS)
                        ),
                        None => format!("{} — {}", task.issue_key, task.title),
                    }
                })
                .collect();
            if !task_sections.is_empty() {
                sections.push(format!(
                    "## The linked task{}\n{}",
                    if task_sections.len() == 1 { "" } else { "s" },
                    task_sections.join("\n\n")
                ));
            }
        }

        if sections.is_empty() {
            let first_prompt = session.and_then(|session| {
                session.timeline.iter().find_map(|item| match item {
                    AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => {
                        let visible = visible_agent_chat_submission_text(text).trim();
                        (!visible.is_empty()).then(|| visible.to_string())
                    }
                    _ => None,
                })
            })?;
            sections.push(format!("## The original request\n{first_prompt}"));
        }

        Some(sections.join("\n\n"))
    }

    /// Close the implementation's verification lifecycle against stated
    /// intent. Eligible work is offered, started immediately, or ignored based
    /// on the user's verification preference. A requested fix can be verified
    /// again until every clear unmet requirement is met.
    pub(super) fn maybe_auto_verify(&mut self, cx: &mut Context<Self>) {
        let mut fire: Vec<Uuid> = Vec::new();
        let mut offer: Vec<Uuid> = Vec::new();
        let mut completed: Vec<Uuid> = Vec::new();
        let mut declined: Vec<Uuid> = Vec::new();
        let verification_mode = self.workspace.read(cx).verification_mode;
        {
            let chats = self.agent_chats.read(cx);
            let agents = self.agents.read(cx);
            self.agent_verify_scan_seen
                .retain(|agent_id, _| chats.sessions.contains_key(agent_id));
            for (agent_id, session) in chats.sessions.iter() {
                let agent = agents.agent(*agent_id);
                // Skip sessions whose decision inputs haven't changed since
                // the last scan: the lifecycle walk below reads the whole
                // timeline, and this observer fires on every event from any
                // chat. Timeline items relevant to the lifecycle (verification
                // cards, marker user turns) are only ever appended — each
                // carries a fresh id — so length + activity stamp track every
                // mutation that could change the outcome.
                let scan_key = (
                    session.timeline.len(),
                    session.last_activity_at,
                    session.status,
                    agent.is_some(),
                    agent.is_some_and(AgentRecord::is_verification_closed),
                    verification_mode,
                );
                if self.agent_verify_scan_seen.get(agent_id) == Some(&scan_key) {
                    continue;
                }
                self.agent_verify_scan_seen.insert(*agent_id, scan_key);
                let lifecycle = verification_lifecycle(&session.timeline);
                if lifecycle == VerificationLifecycle::Declined
                    && agent.is_some_and(|agent| !agent.verification_closed)
                {
                    // Promote dismissals written by older builds from a
                    // timeline-only marker to the authoritative hard gate.
                    declined.push(*agent_id);
                }
                if lifecycle == VerificationLifecycle::Complete
                    && agent.is_some_and(|agent| agent.verification_completed_at.is_none())
                {
                    completed.push(*agent_id);
                }
                let previous = self.agent_status_seen.insert(*agent_id, session.status);
                if previous != Some(AgentChatStatus::Running)
                    || session.status != AgentChatStatus::Idle
                {
                    continue;
                }
                if agent.is_some_and(AgentRecord::is_verification_closed)
                    || lifecycle == VerificationLifecycle::Complete
                {
                    continue;
                }
                match lifecycle {
                    VerificationLifecycle::NotStarted => {
                        if session.changed_files.is_empty() {
                            continue;
                        }
                        let written_intent = agent.is_some_and(|agent| {
                            agent.source_doc.is_some()
                                || !agent.linked_tasks.is_empty()
                                || agent.source_task.is_some()
                        }) || session
                            .proposed_plan
                            .as_ref()
                            .is_some_and(|plan| plan.implemented_at.is_some());
                        if !written_intent {
                            continue;
                        }
                    }
                    // A fix turn is the only thing that reopens verification.
                    VerificationLifecycle::Fixing => {}
                    VerificationLifecycle::Verifying
                    | VerificationLifecycle::NeedsFix
                    | VerificationLifecycle::Declined
                    | VerificationLifecycle::Complete => continue,
                }
                match verification_mode {
                    ide_core::config::VerificationMode::Ask => offer.push(*agent_id),
                    ide_core::config::VerificationMode::Automatic => fire.push(*agent_id),
                    ide_core::config::VerificationMode::Off => {}
                }
            }
        }
        if !declined.is_empty() {
            for agent_id in &declined {
                self.verification_prompt_pending.remove(agent_id);
            }
            self.agents.update(cx, |agents, cx| {
                for agent_id in declined {
                    agents.mark_verification_closed(agent_id, cx);
                }
            });
        }
        if !completed.is_empty() {
            for agent_id in &completed {
                self.verification_prompt_pending.remove(agent_id);
            }
            self.agents.update(cx, |agents, cx| {
                for agent_id in completed {
                    agents.mark_verification_completed(agent_id, cx);
                }
            });
        }
        let mut offered = false;
        for agent_id in offer {
            offered |= self.verification_prompt_pending.insert(agent_id);
        }
        if offered {
            cx.notify();
        }
        for agent_id in fire {
            self.request_agent_verification(agent_id, cx);
        }
    }

    /// Retire the backend processes of chats that have been idle longer than
    /// `AGENT_CHAT_IDLE_RETIRE_AFTER`. The conversation stays exactly as it
    /// is; the next submission restarts the backend and resumes the provider
    /// session from its saved id. The currently selected chat of every
    /// project, hydrating chats, and the dedicated design/doc assistants are
    /// left running, and `AgentChatState::retire_idle_backend` re-checks that
    /// nothing in-flight (turns, questions, approvals) can be lost.
    pub(super) fn retire_idle_agent_chat_backends(&mut self, cx: &mut Context<Self>) {
        let now = unix_now_secs();
        let selected: HashSet<Uuid> = {
            let agents = self.agents.read(cx);
            self.workspace
                .read(cx)
                .projects
                .iter()
                .filter_map(|project| agents.selected_agent_id(project.id))
                .collect()
        };
        let candidates: Vec<Uuid> = {
            let chats = self.agent_chats.read(cx);
            let agents = self.agents.read(cx);
            chats
                .sessions
                .iter()
                .filter_map(|(agent_id, session)| {
                    if !chats.has_backend(*agent_id)
                        || selected.contains(agent_id)
                        || self.agent_chat_hydrating.contains(agent_id)
                    {
                        return None;
                    }
                    let idle_for = now.saturating_sub(session.last_activity_at);
                    if idle_for < AGENT_CHAT_IDLE_RETIRE_AFTER.as_secs() {
                        return None;
                    }
                    // An untouched chat has nothing to resume; retiring it
                    // would only regress the empty view to a resume prompt.
                    if session.messages.is_empty() {
                        return None;
                    }
                    let mut record = agents.agent(*agent_id)?.clone();
                    if record.hidden_doc_assistant || record.design_context.is_some() {
                        return None;
                    }
                    // Use the exact resume predicate the restart path enforces
                    // (it is provider-specific), on the same merged record it
                    // will see. Anything that cannot restart must keep its
                    // backend, or the chat would strand behind the guard.
                    self.apply_live_chat_session_ids(&mut record, cx);
                    if !agent_has_backend_resume_id(&record) {
                        return None;
                    }
                    Some(*agent_id)
                })
                .collect()
        };
        if candidates.is_empty() {
            return;
        }
        self.agent_chats.update(cx, |chats, cx| {
            for agent_id in candidates {
                chats.retire_idle_backend(agent_id, cx);
            }
        });
    }

    /// Apply a Settings change to any verification decisions already waiting
    /// in open chats.
    pub(super) fn reconcile_verification_mode(&mut self, cx: &mut Context<Self>) {
        let verification_mode = self.workspace.read(cx).verification_mode;
        match verification_mode {
            ide_core::config::VerificationMode::Ask => {}
            ide_core::config::VerificationMode::Automatic => {
                let pending = self.verification_prompt_pending.drain().collect::<Vec<_>>();
                for agent_id in pending {
                    self.request_agent_verification(agent_id, cx);
                }
            }
            ide_core::config::VerificationMode::Off => {
                if !self.verification_prompt_pending.is_empty() {
                    self.verification_prompt_pending.clear();
                    cx.notify();
                }
            }
        }
    }

    /// Ask the agent to address a verification's clearly Missed requirements.
    /// Unclear items remain discussion points for the user and are never sent
    /// through the automatic fix path.
    pub(super) fn request_agent_verification_fix(
        &mut self,
        agent_id: Uuid,
        verification_id: String,
        cx: &mut Context<Self>,
    ) {
        let is_current_gap = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .is_some_and(|session| {
                verification_lifecycle(&session.timeline) == VerificationLifecycle::NeedsFix
                    && session.timeline.iter().rev().find_map(|item| match item {
                        AgentChatTimelineItem::Verification(latest) => Some(latest.id.as_str()),
                        _ => None,
                    }) == Some(verification_id.as_str())
            });
        if !is_current_gap {
            return;
        }
        let lines = self
            .agent_chats
            .read(cx)
            .verification_fix_lines(agent_id, &verification_id);
        if lines.is_empty() {
            return;
        }
        let mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        let list = lines
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let prompt = format!(
            "{AGENT_VERIFY_FIX_PREFIX}:\n{list}\n\nFor each one, make the change that satisfies the requirement — or, if on closer inspection the requirement is actually met, say so and point at the evidence. When you're done, give a short summary of what you changed, grouped by file."
        );
        if self.dispatch_agent_chat_submission(agent_id, prompt, mode, cx) {
            self.agent_chats.update(cx, |chats, cx| {
                chats.mark_verification_items_fix_requested(agent_id, &verification_id, cx);
            });
            self.acknowledge_agent_chat_seen(agent_id, cx);
        }
    }

    pub(super) fn submit_agent_chat_message(
        &mut self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        steer_running: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.submit_agent_chat_message_for_surface(
            agent,
            input,
            steer_running,
            &AgentChatSurface::Standard,
            window,
            cx,
        );
    }

    pub(super) fn submit_agent_chat_message_for_surface(
        &mut self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        steer_running: bool,
        surface: &AgentChatSurface,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = input.read(cx).value().trim().to_string();
        let selected_command = self.agent_chat_selected_commands.get(&agent.id).cloned();
        let preview_armed = self.agent_chat_preview_armed.contains(&agent.id);
        let selected_mentions = self
            .agent_chat_selected_mentions
            .get(&agent.id)
            .cloned()
            .unwrap_or_default();
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
        let message_display_text = append_pasted_text_blocks(
            &composer_message_display_text(&draft, selected_command.as_ref(), &selected_mentions),
            &pasted_text_blocks,
        );
        let message_tags =
            composer_message_tags(selected_command.as_ref(), &selected_mentions, preview_armed);
        let projects = self.workspace.read(cx).projects.clone();
        if let Some(target_agent_id) = self
            .agent_chat_selected_agent_targets
            .get(&agent.id)
            .copied()
        {
            let message = message_display_text.trim().to_string();
            if message.is_empty() {
                return;
            }
            let request_kind = self
                .agent_chat_agent_request_kind_overrides
                .remove(&agent.id)
                .unwrap_or_else(|| classify_agent_request(&message));
            input.update(cx, |input, cx| input.set_value("", window, cx));
            self.agent_chat_attached_files.remove(&agent.id);
            self.agent_chat_pasted_text_blocks.remove(&agent.id);
            self.agent_chat_selected_commands.remove(&agent.id);
            self.agent_chat_selected_mentions.remove(&agent.id);
            self.agent_chat_selected_agent_targets.remove(&agent.id);
            self.agent_chat_preview_armed.remove(&agent.id);
            self.queue_composer_agent_message(agent.id, target_agent_id, message, request_kind, cx);
            self.acknowledge_agent_chat_seen(agent.id, cx);
            cx.notify();
            return;
        }
        // Captured before resolution: plan feedback is a decision worth
        // examining for a durable preference once the submission goes through.
        let refine_plan_markdown = self
            .agent_chats
            .read(cx)
            .session(agent.id)
            .and_then(|session| session.proposed_plan.as_ref())
            .filter(|plan| plan.implemented_at.is_none())
            .map(|plan| plan.markdown.clone());
        let Some((submission_text, mode)) = self.agent_chats.update(cx, |chats, cx| {
            let draft_with_pastes = append_pasted_text_blocks(&draft, &pasted_text_blocks);
            let has_actionable_plan = {
                let session = chats.ensure_session(agent.id, agent.title.clone(), cx);
                session
                    .proposed_plan
                    .as_ref()
                    .is_some_and(|plan| plan.implemented_at.is_none())
            };
            if has_actionable_plan {
                chats.resolve_proposed_plan_submission(agent.id, &draft_with_pastes, cx)
            } else if draft.is_empty()
                && attached_files.is_empty()
                && pasted_text_blocks.is_empty()
                && selected_command.is_none()
                && selected_mentions.is_empty()
            {
                None
            } else {
                let session = chats.ensure_session(agent.id, agent.title.clone(), cx);
                let draft =
                    composer_mentions_submission_text(&draft, &selected_mentions, &projects);
                let draft = agent_chat_submission_text(&draft, selected_command.as_ref());
                let draft = preview_submission_text(&draft, preview_armed);
                let draft = memory_save_submission_text(&draft);
                let draft = append_pasted_text_blocks(&draft, &pasted_text_blocks);
                Some((
                    prompt_with_attached_files(&draft, &attached_files),
                    session.interaction_mode,
                ))
            }
        }) else {
            return;
        };
        let auto_name_context =
            self.auto_name_context_for_second_message(agent, &message_display_text, surface, cx);
        // A plan existed, so this submission went through plan resolution;
        // Plan mode back out means the user typed a correction (refine path).
        if let Some(plan_markdown) = refine_plan_markdown {
            if mode == AgentInteractionMode::Plan {
                self.maybe_propose_memory_from_plan_feedback(
                    agent.id,
                    submission_text.clone(),
                    plan_markdown,
                    cx,
                );
            }
        }

        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.agent_chat_attached_files.remove(&agent.id);
        self.agent_chat_pasted_text_blocks.remove(&agent.id);
        self.agent_chat_selected_commands.remove(&agent.id);
        self.agent_chat_selected_mentions.remove(&agent.id);
        self.agent_chat_preview_armed.remove(&agent.id);
        self.agent_chat_preview_suggestion_dismissed
            .remove(&agent.id);
        if steer_running {
            let is_running = self
                .agent_chats
                .read(cx)
                .session(agent.id)
                .is_some_and(|session| {
                    matches!(
                        session.status,
                        AgentChatStatus::Running | AgentChatStatus::Cancelling
                    )
                })
                && self.agent_chats.read(cx).has_backend(agent.id);
            if is_running {
                self.agent_chats
                    .update(cx, |chats, cx| chats.force_stop_backend(agent.id, cx));
            }
        }
        let mode = if surface.allows_plan_mode() {
            mode
        } else {
            AgentInteractionMode::Default
        };
        let submitted = self.dispatch_agent_chat_submission_with_agent(
            agent,
            submission_text,
            Some(message_display_text),
            message_tags,
            mode,
            cx,
        );
        if submitted {
            if let Some((first_message, second_message)) = auto_name_context {
                self.request_agent_auto_name(agent.clone(), first_message, second_message, cx);
            }
        } else {
            // A failed dispatch did not consume the second-message trigger.
            self.agent_auto_names_requested.remove(&agent.id);
        }
        self.acknowledge_agent_chat_seen(agent.id, cx);
    }

    pub(super) fn continue_pending_user_input(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let custom_answer = input.read(cx).value().trim().to_string();
        if !custom_answer.is_empty() {
            self.agent_chats.update(cx, |chats, cx| {
                chats.set_pending_user_input_custom_answer(agent_id, custom_answer, cx);
            });
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }

        let Some(progress) = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .and_then(|session| session.pending_user_input.as_ref())
            .map(|pending| pending.progress())
        else {
            return;
        };

        if progress.is_last_question {
            if progress.is_complete {
                // Clone before submit clears it — typed answers are a decision
                // worth examining for a durable preference.
                let pending = self
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .and_then(|session| session.pending_user_input.clone());
                self.agent_chats.update(cx, |chats, cx| {
                    chats.submit_pending_user_input(agent_id, cx);
                });
                if let Some(pending) = pending {
                    let pairs = pending
                        .questions
                        .iter()
                        .zip(pending.answers.iter())
                        .filter_map(|(question, answer)| {
                            let custom = answer.as_ref()?.custom_answer.clone()?;
                            Some((question.question.clone(), custom))
                        })
                        .collect::<Vec<_>>();
                    self.maybe_propose_memory_from_question_answers(agent_id, pairs, cx);
                }
            }
        } else if progress.can_advance {
            self.agent_chats.update(cx, |chats, cx| {
                chats.next_pending_user_input_question(agent_id, cx);
            });
        }
        self.acknowledge_agent_chat_seen(agent_id, cx);
    }

    pub(super) fn dispatch_agent_chat_submission(
        &mut self,
        agent_id: Uuid,
        submission_text: String,
        fallback_mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) -> bool {
        self.dispatch_agent_chat_submission_inner(
            agent_id,
            submission_text,
            None,
            Vec::new(),
            fallback_mode,
            None,
            cx,
        )
    }

    pub(super) fn enqueue_inline_preview_review(
        &mut self,
        review: ide_core::visual_review::VisualReviewSubmission,
        target_agent: Option<AgentRecord>,
        cx: &mut Context<Self>,
    ) {
        let mut review = review;
        let review_id = review.id;
        let agent_id = review.agent_id;
        if !self.project_preview_review_ids_seen.insert(review_id) {
            return;
        }

        cx.spawn(async move |this, cx| {
            let prepared = cx
                .background_executor()
                .spawn(async move {
                    use base64::Engine as _;

                    let image = base64::engine::general_purpose::STANDARD
                        .decode(review.image_base64.trim())
                        .map_err(|error| {
                            anyhow::anyhow!("invalid Preview review image: {error}")
                        })?;
                    anyhow::ensure!(!image.is_empty(), "Preview review image is empty");
                    anyhow::ensure!(
                        image.len() <= 16 * 1024 * 1024,
                        "Preview review image exceeds 16 MB"
                    );
                    let (mime_type, extension) = visual_review_image_metadata(&image)
                        .ok_or_else(|| anyhow::anyhow!("unsupported Preview review image"))?;
                    let store = ide_core::local_store::LocalStore::open_default()?;
                    let attachment = store.materialize_attachment_bytes(
                        agent_id,
                        format!("visual-review-{review_id}.{extension}"),
                        Some(mime_type.to_string()),
                        extension,
                        &image,
                    )?;
                    review.image_base64.clear();
                    Ok::<_, anyhow::Error>((review, store.root().join(attachment.relative_path)))
                })
                .await;

            this.update(cx, |this, cx| {
                let (review, attachment_path) = match prepared {
                    Ok(prepared) => prepared,
                    Err(error) => {
                        this.project_preview_review_ids_seen.remove(&review_id);
                        eprintln!("failed to prepare visual review {review_id}: {error:#}");
                        return;
                    }
                };
                let agent = target_agent
                    .filter(|agent| agent.id == agent_id && agent.project_id == review.project_id)
                    .or_else(|| this.agents.read(cx).agent(agent_id).cloned());
                let Some(agent) = agent else {
                    this.project_preview_review_ids_seen.remove(&review_id);
                    return;
                };
                let target_label = visual_review_target_label(&review);
                let target_context = visual_review_target_context(&review);
                let display_text = format!("{}\n\n{}", review.comment, target_label);
                let submission = prompt_with_attached_files(
                    &ide_core::penpot_assistant::preview_review_prompt(
                        &review.comment,
                        &review.url,
                        &target_context,
                    ),
                    std::slice::from_ref(&attachment_path),
                );
                let mode = this
                    .agent_chats
                    .read(cx)
                    .session(agent_id)
                    .map(|session| session.interaction_mode)
                    .unwrap_or(AgentInteractionMode::Default);
                let sent = this.dispatch_agent_chat_submission_with_agent(
                    &agent,
                    submission,
                    Some(display_text),
                    vec![AgentChatMessageTag {
                        kind: AgentChatMessageTagKind::Preview,
                        label: "Preview review".to_string(),
                        detail: Some(review.url.clone()),
                    }],
                    mode,
                    cx,
                );
                if !sent {
                    this.project_preview_review_ids_seen.remove(&review_id);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn agent_connected_context_extras(
        &self,
        agent: &AgentRecord,
        cx: &App,
    ) -> AgentConnectedContextExtras {
        let connected_designs = self
            .penpot
            .read(cx)
            .designs_for_agent(&agent)
            .into_iter()
            .map(|design| AgentConnectedDesign {
                design_id: design.id,
                file_id: design.penpot_file_id,
                name: design.name,
                page_id: design.page_id,
            })
            .collect();
        let pull_request =
            self.agent_ship_prs
                .get(&agent.id)
                .map(|pull_request| AgentConnectedPullRequest {
                    repository_path: agent
                        .ship_pr_repo_path
                        .clone()
                        .unwrap_or_else(|| agent.repository_root().to_path_buf()),
                    branch: pull_request.branch.clone(),
                    base_branch: pull_request.base_branch.clone(),
                    number: pull_request.number,
                    title: pull_request.title.clone(),
                    url: pull_request.url.clone(),
                    state: pull_request.state.clone(),
                    is_draft: pull_request.is_draft,
                });
        AgentConnectedContextExtras {
            designs: connected_designs,
            pull_request,
        }
    }

    pub(super) fn dispatch_agent_chat_submission_with_agent(
        &mut self,
        agent: &AgentRecord,
        submission_text: String,
        display_text: Option<String>,
        tags: Vec<AgentChatMessageTag>,
        fallback_mode: AgentInteractionMode,
        cx: &mut Context<Self>,
    ) -> bool {
        let agent = self
            .agents
            .read(cx)
            .agent(agent.id)
            .cloned()
            .unwrap_or_else(|| agent.clone());
        let connected_context = self.agent_connected_context_extras(&agent, cx);
        let submission_text =
            ide_core::prompt_with_connected_context(&submission_text, &agent, &connected_context);
        if let Some(design_context) = agent.design_context {
            let design_id = design_context.design_id;
            let expected_file_id = design_context.file_id;
            let ready = matches!(
                self.design_mcp_readiness.get(&design_id),
                Some(DesignMcpReadiness::Ready { file_id })
                    if *file_id == expected_file_id
            );
            if !ready {
                let pending_id = Uuid::new_v4();
                let queue = self
                    .pending_design_assistant_submissions
                    .entry(design_id)
                    .or_default();
                queue.push_back(PendingDesignAssistantSubmission {
                    id: pending_id,
                    queued_at: Instant::now(),
                    agent: agent.clone(),
                    text: submission_text,
                    display_text,
                    tags,
                    mode: fallback_mode,
                });
                self.agent_start_errors.insert(
                    agent.id,
                    match self.design_mcp_readiness.get(&design_id) {
                        Some(DesignMcpReadiness::Blocked(error)) => error.clone(),
                        _ => {
                            "Connecting the exact design before starting the assistant…".to_string()
                        }
                    },
                );
                self.penpot.update(cx, |penpot, cx| {
                    penpot.set_assistant_busy(agent.project_id, true, cx)
                });
                let agent_id = agent.id;
                let project_id = agent.project_id;
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(DESIGN_MCP_QUEUE_TIMEOUT)
                        .await;
                    let Some(center) = this.upgrade() else {
                        return;
                    };
                    center
                        .update(cx, |this, cx| {
                            let mut removed = false;
                            let mut queue_empty = false;
                            if let Some(queue) =
                                this.pending_design_assistant_submissions.get_mut(&design_id)
                            {
                                if let Some(index) =
                                    queue.iter().position(|submission| submission.id == pending_id)
                                {
                                    queue.remove(index);
                                    removed = true;
                                }
                                queue_empty = queue.is_empty();
                            }
                            if queue_empty {
                                this.pending_design_assistant_submissions.remove(&design_id);
                            }
                            if !removed {
                                return;
                            }
                            this.agent_start_errors.insert(
                                agent_id,
                                "Design connection timed out before the assistant could start. Reopen this design and send the message again."
                                    .to_string(),
                            );
                            if queue_empty {
                                this.penpot.update(cx, |penpot, cx| {
                                    penpot.set_assistant_busy(project_id, false, cx)
                                });
                            }
                            cx.notify();
                        })
                        .ok();
                })
                .detach();
                cx.notify();
                return true;
            }
        }
        self.dispatch_agent_chat_submission_inner(
            agent.id,
            submission_text,
            display_text,
            tags,
            fallback_mode,
            Some(agent),
            cx,
        )
    }

    fn dispatch_agent_chat_submission_inner(
        &mut self,
        agent_id: Uuid,
        mut submission_text: String,
        display_text: Option<String>,
        tags: Vec<AgentChatMessageTag>,
        fallback_mode: AgentInteractionMode,
        fallback_agent: Option<AgentRecord>,
        cx: &mut Context<Self>,
    ) -> bool {
        let has_backend = self.agent_chats.read(cx).has_backend(agent_id);
        let (mode, is_running_status) = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| {
                (
                    session.interaction_mode,
                    matches!(
                        session.status,
                        AgentChatStatus::Running | AgentChatStatus::Cancelling
                    ),
                )
            })
            .unwrap_or((fallback_mode, false));
        let is_running = is_running_status && has_backend;

        if self.agent_chat_hydrating.contains(&agent_id) {
            let has_resume_id = self
                .agents
                .read(cx)
                .agent(agent_id)
                .or(fallback_agent.as_ref())
                .is_some_and(agent_has_backend_resume_id);
            if should_defer_agent_chat_submission_for_resume(
                has_resume_id,
                has_backend,
                true,
                false,
            ) {
                self.agent_chat_post_hydration_submissions
                    .entry(agent_id)
                    .or_default()
                    .push(PostHydrationAgentChatSubmission {
                        text: submission_text,
                        display_text,
                        tags,
                        mode,
                    });
                cx.notify();
                return true;
            }
        }

        if !is_running && !has_backend {
            self.sync_chat_session_ids(cx);
            let Some(mut agent) = self
                .agents
                .read(cx)
                .agent(agent_id)
                .cloned()
                .or(fallback_agent)
            else {
                return false;
            };
            self.apply_live_chat_session_ids(&mut agent, cx);
            if agent_has_backend_resume_id(&agent) {
                submission_text = summary_resume_submission_text(&submission_text, agent_id);
            }
            if !agent_has_backend_resume_id(&agent)
                && (agent.started_at.is_some()
                    || self.agent_chat_has_persisted_history(agent_id, cx))
            {
                self.agent_start_errors.insert(
                    agent_id,
                    "Cannot restart this chat safely because no Claude/Codex resume id was captured. Reset only if you intentionally want a new backend session."
                        .to_string(),
                );
                cx.notify();
                return false;
            }
            let has_loaded_history =
                self.agent_chats
                    .read(cx)
                    .session(agent_id)
                    .is_some_and(|session| {
                        !session.messages.is_empty()
                            || session
                                .timeline
                                .iter()
                                .any(|item| matches!(item, AgentChatTimelineItem::Message(_)))
                    });
            let needs_resume_hydration = should_defer_agent_chat_submission_for_resume(
                agent_has_backend_resume_id(&agent),
                false,
                false,
                has_loaded_history,
            );
            if let Err(error) = self
                .agent_chats
                .update(cx, |chats, cx| chats.start_backend(agent.clone(), mode, cx))
            {
                self.agent_start_errors
                    .insert(agent_id, format!("failed to restart chat: {error:#}"));
                cx.notify();
                return false;
            }
            self.agent_start_errors.remove(&agent_id);
            self.agents
                .update(cx, |agents, cx| agents.mark_started(agent_id, None, cx));
            if needs_resume_hydration {
                self.agent_chat_post_hydration_submissions
                    .entry(agent_id)
                    .or_default()
                    .push(PostHydrationAgentChatSubmission {
                        text: submission_text,
                        display_text,
                        tags,
                        mode,
                    });
                self.schedule_agent_chat_hydration(agent, cx);
                return true;
            }
        }
        self.agent_chats.update(cx, |chats, cx| {
            if is_running {
                chats.queue_turn(
                    agent_id,
                    submission_text.clone(),
                    display_text.clone(),
                    tags.clone(),
                    mode,
                    cx,
                );
            } else {
                chats.append_message(
                    agent_id,
                    AgentChatMessage::User {
                        text: submission_text.clone(),
                        display_text: display_text.clone(),
                        tags: tags.clone(),
                        created_at: unix_now_secs(),
                    },
                    cx,
                );
                chats.send_turn(agent_id, submission_text.clone(), mode, cx);
            }
        });
        cx.notify();
        true
    }

    pub(super) fn apply_live_chat_session_ids(&self, agent: &mut AgentRecord, cx: &App) {
        if let Some(session) = self.agent_chats.read(cx).session(agent.id) {
            if agent.chat_session_id.is_none() {
                agent.chat_session_id = session.chat_session_id.clone();
            }
            if agent.cli_session_id.is_none() {
                agent.cli_session_id = session.cli_session_id.clone();
            }
        }
        if agent.provider == AgentKind::Codex && agent.chat_session_id.is_none() {
            agent.chat_session_id = agent.cli_session_id.clone();
        }
    }

    pub(super) fn agent_chat_has_persisted_history(&self, agent_id: Uuid, cx: &App) -> bool {
        self.agent_chats
            .read(cx)
            .session(agent_id)
            .is_some_and(|session| {
                !session.messages.is_empty()
                    || session
                        .timeline
                        .iter()
                        .any(|item| matches!(item, AgentChatTimelineItem::Message(_)))
            })
    }

    pub(super) fn active_agent_chat_slash_view(
        &self,
        agent: &AgentRecord,
        cx: &App,
    ) -> Option<AgentChatSlashView> {
        let agent_id = agent.id;
        let input = self.agent_chat_inputs.get(&agent_id)?;
        let query = agent_chat_slash_query(&input.read(cx).value())?;
        if self
            .agent_chat_slash_dismissed_query
            .get(&agent_id)
            .is_some_and(|dismissed| dismissed == &query.query)
        {
            return None;
        }
        let commands = agent_chat_slash_capabilities(agent.provider);
        let matches = agent_chat_slash_matches(&commands, &query.query);
        let selected = self
            .agent_chat_slash_selection
            .get(&agent_id)
            .copied()
            .unwrap_or(0)
            .min(matches.len().saturating_sub(1));
        Some(AgentChatSlashView {
            query,
            matches,
            selected,
        })
    }

    pub(super) fn insert_agent_chat_slash_command(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        command: AgentCapability,
        query: AgentChatSlashQuery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
        if query.range.start > query.range.end || query.range.end > current.len() {
            return;
        }
        let (next, cursor) = remove_agent_chat_slash_query(&current, &query);
        let (next, cursor) = insert_agent_chat_command_invocation(&next, cursor, &command);
        if command.is_choro_preview() {
            self.agent_chat_preview_armed.insert(agent_id);
            self.agent_chat_preview_suggestion_dismissed
                .remove(&agent_id);
        } else {
            self.agent_chat_selected_commands.insert(agent_id, command);
        }
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        self.agent_chat_slash_selection.insert(agent_id, 0);
        self.agent_chat_slash_dismissed_query.remove(&agent_id);
        cx.notify();
    }

    pub(super) fn select_agent_chat_command(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        command: AgentCapability,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
        let (next, cursor) = insert_agent_chat_command_invocation(&current, 0, &command);
        self.agent_chat_selected_commands.insert(agent_id, command);
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        self.agent_chat_slash_selection.insert(agent_id, 0);
        self.agent_chat_slash_dismissed_query.remove(&agent_id);
        cx.notify();
    }

    /// Pull a queued turn back into the composer for editing: its visible text
    /// and attachments are restored, the cursor moves to the end, and the turn
    /// is removed from the queue so re-sending it re-queues a fresh copy.
    pub(super) fn edit_queued_turn_into_composer(
        &mut self,
        agent_id: Uuid,
        turn_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.agent_chat_inputs.get(&agent_id).cloned() else {
            return;
        };
        let Some((text, attached_files)) = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .and_then(|session| session.queued_turns.iter().find(|turn| turn.id == turn_id))
            .map(queued_turn_composer_draft)
        else {
            return;
        };
        self.agent_chats.update(cx, |chats, cx| {
            chats.remove_queued_turn(agent_id, turn_id, cx)
        });
        if attached_files.is_empty() {
            self.agent_chat_attached_files.remove(&agent_id);
        } else {
            self.agent_chat_attached_files
                .insert(agent_id, attached_files);
        }
        input.update(cx, |input, cx| {
            input.set_value(text.clone(), window, cx);
            input.set_cursor_position(
                input_position_for_byte_offset(&text, text.len()),
                window,
                cx,
            );
            input.focus(window, cx);
        });
        cx.notify();
    }

    pub(super) fn active_agent_chat_doc_mention_view(
        &self,
        agent_id: Uuid,
        project: ProjectId,
        cx: &App,
    ) -> Option<ComposerDocMentionView> {
        let input = self.agent_chat_inputs.get(&agent_id)?;
        let mention = active_composer_doc_mention(&input.read(cx))?;
        if self
            .agent_chat_doc_dismissed_query
            .get(&agent_id)
            .is_some_and(|dismissed| dismissed == &mention.query)
        {
            return None;
        }
        let docs = self.docs.read(cx).docs_for_project(project);
        let mut matches = composer_doc_mention_matches(&mention, &docs);
        let query = mention.query.to_ascii_lowercase();
        let mut designs: Vec<ProjectReference> = self
            .designs
            .read(cx)
            .references_for_project(project)
            .into_iter()
            .filter(|reference| {
                query.is_empty()
                    || reference.title.to_ascii_lowercase().contains(&query)
                    || reference.source.to_ascii_lowercase().contains(&query)
            })
            .take(COMPOSER_PICKER_VISIBLE_LIMIT / 2)
            .collect();
        {
            let penpot = self.penpot.read(cx);
            designs.extend(
                penpot
                    .designs_for_project(project)
                    .into_iter()
                    .filter_map(|design| {
                        let source = penpot.design_url(&design)?;
                        (query.is_empty()
                            || design.name.to_ascii_lowercase().contains(&query)
                            || source.to_ascii_lowercase().contains(&query))
                        .then(|| penpot_design_reference(&design, source))
                    }),
            );
        }
        designs.truncate(COMPOSER_PICKER_VISIBLE_LIMIT / 2);
        matches.truncate(COMPOSER_PICKER_VISIBLE_LIMIT.saturating_sub(designs.len()));
        let total = matches.len() + designs.len();
        let selected = self
            .agent_chat_doc_selection
            .get(&agent_id)
            .copied()
            .unwrap_or(0)
            .min(total.saturating_sub(1));
        Some(ComposerDocMentionView {
            mention,
            matches,
            designs,
            selected,
        })
    }

    pub(super) fn active_agent_chat_agent_mention_view(
        &self,
        agent: &AgentRecord,
        cx: &App,
    ) -> Option<ComposerAgentMentionView> {
        let input = self.agent_chat_inputs.get(&agent.id)?;
        let mention = active_composer_agent_mention(&input.read(cx))?;
        if self
            .agent_chat_agent_dismissed_query
            .get(&agent.id)
            .is_some_and(|dismissed| dismissed == &mention.query)
        {
            return None;
        }
        let project_name = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == agent.project_id)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Project".to_string());
        let mut matches = self
            .agents
            .read(cx)
            .records_for_project(agent.project_id)
            .into_iter()
            .filter(|candidate| candidate.id != agent.id)
            .map(|candidate| ComposerAgentEntry {
                id: candidate.id,
                title: candidate.title,
                status: candidate.status,
                project_name: project_name.clone(),
                active: self.agent_chats.read(cx).has_backend(candidate.id),
            })
            .filter(|candidate| composer_agent_matches(candidate, &mention.query))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right.active.cmp(&left.active).then_with(|| {
                left.title
                    .to_ascii_lowercase()
                    .cmp(&right.title.to_ascii_lowercase())
            })
        });
        matches.truncate(8);
        let selected = self
            .agent_chat_agent_selection
            .get(&agent.id)
            .copied()
            .unwrap_or_default()
            .min(matches.len().saturating_sub(1));
        Some(ComposerAgentMentionView {
            mention,
            matches,
            selected,
        })
    }

    pub(super) fn active_agent_chat_project_mention_view(
        &self,
        agent: &AgentRecord,
        cx: &App,
    ) -> Option<ComposerProjectMentionView> {
        let input = self.agent_chat_inputs.get(&agent.id)?;
        let mention = active_composer_project_mention(&input.read(cx))?;
        if self
            .agent_chat_project_dismissed_query
            .get(&agent.id)
            .is_some_and(|dismissed| dismissed == &mention.query)
        {
            return None;
        }
        let mut matches = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .filter(|project| project.id != agent.project_id)
            .map(|project| ComposerProjectEntry {
                id: project.id,
                name: project.name.clone(),
                path: project.path.clone(),
                is_favorite: project.is_favorite,
            })
            .filter(|project| composer_project_matches(project, &mention.query))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| {
            right.is_favorite.cmp(&left.is_favorite).then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
        });
        matches.truncate(COMPOSER_PICKER_VISIBLE_LIMIT);
        let selected = self
            .agent_chat_project_selection
            .get(&agent.id)
            .copied()
            .unwrap_or_default()
            .min(matches.len().saturating_sub(1));
        Some(ComposerProjectMentionView {
            mention,
            matches,
            selected,
        })
    }

    pub(super) fn active_agent_chat_file_mention_view(
        &mut self,
        agent: &AgentRecord,
        cx: &App,
    ) -> Option<ComposerFileMentionView> {
        let input = self.agent_chat_inputs.get(&agent.id)?.clone();
        let mention = active_composer_file_mention(&input.read(cx))?;
        if self
            .agent_chat_file_dismissed_query
            .get(&agent.id)
            .is_some_and(|dismissed| dismissed == &mention.query)
        {
            return None;
        }
        let (_, root) = self.project_by_id(agent.project_id, cx)?;
        let files = self.workspace_file_entries(agent.project_id, &root);
        let mut matches = composer_file_mention_matches(&mention, &files);
        matches.truncate(COMPOSER_FILE_MENTION_LIMIT.min(COMPOSER_PICKER_VISIBLE_LIMIT));
        let selected = self
            .agent_chat_file_selection
            .get(&agent.id)
            .copied()
            .unwrap_or(0)
            .min(matches.len().saturating_sub(1));
        Some(ComposerFileMentionView {
            mention,
            matches,
            selected,
        })
    }

    pub(super) fn insert_agent_chat_doc_mention(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        relative_path: PathBuf,
        mention: ComposerDocMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = apply_composer_doc_mention(&current, &mention, &relative_path);
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        let title = relative_path
            .file_stem()
            .and_then(|name| name.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| relative_path.to_string_lossy().to_string());
        let token = ComposerMentionToken::doc(title, &relative_path);
        let mentions = self
            .agent_chat_selected_mentions
            .entry(agent_id)
            .or_default();
        if !mentions.contains(&token) {
            mentions.push(token);
        }
        self.agent_chat_doc_selection.insert(agent_id, 0);
        self.agent_chat_doc_dismissed_query.remove(&agent_id);
        cx.notify();
    }

    /// Pick a design from the `@@` picker: drop the typed `@@query`, then attach
    /// the design's preview image (so the agent can see it) — or, for an
    /// image-less reference, leave its source URL behind as context.
    pub(super) fn insert_agent_chat_design_mention(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        reference: ProjectReference,
        mention: ComposerDocMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
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
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        if let Some(path) = preview {
            self.attach_agent_chat_paths(agent_id, &[path], cx);
        }
        if let Some(token) = penpot_token {
            let mentions = self
                .agent_chat_selected_mentions
                .entry(agent_id)
                .or_default();
            if !mentions.contains(&token) {
                mentions.push(token);
            }
        }
        self.agent_chat_doc_selection.insert(agent_id, 0);
        self.agent_chat_doc_dismissed_query.remove(&agent_id);
        cx.notify();
    }

    pub(super) fn insert_agent_chat_file_mention(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        file: ComposerFileEntry,
        mention: ComposerFileMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = apply_composer_file_mention(&current, &mention, &file.relative_label);
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        let token = ComposerMentionToken::file(&file);
        let mentions = self
            .agent_chat_selected_mentions
            .entry(agent_id)
            .or_default();
        if !mentions.contains(&token) {
            mentions.push(token);
        }
        self.agent_chat_file_selection.insert(agent_id, 0);
        self.agent_chat_file_dismissed_query.remove(&agent_id);
        cx.notify();
    }

    pub(super) fn insert_agent_chat_agent_target(
        &mut self,
        source_agent_id: Uuid,
        input: Entity<InputState>,
        target: ComposerAgentEntry,
        mention: ComposerAgentMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = remove_composer_agent_mention(&current, &mention);
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        self.agent_chat_selected_agent_targets
            .insert(source_agent_id, target.id);
        self.agent_chat_agent_request_kind_overrides
            .remove(&source_agent_id);
        self.agent_chat_agent_selection.insert(source_agent_id, 0);
        self.agent_chat_agent_dismissed_query
            .remove(&source_agent_id);
        cx.notify();
    }

    pub(super) fn insert_agent_chat_project_mention(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        project: ComposerProjectEntry,
        mention: ComposerProjectMention,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = input.read(cx).value().to_string();
        if mention.range.start > mention.range.end || mention.range.end > current.len() {
            return;
        }
        let (next, cursor) = remove_composer_project_mention(&current, &mention);
        input.update(cx, |input, cx| {
            input.set_value(next.clone(), window, cx);
            input.set_cursor_position(input_position_for_byte_offset(&next, cursor), window, cx);
            input.focus(window, cx);
        });
        let token = ComposerMentionToken::project_entry(&project);
        let mentions = self
            .agent_chat_selected_mentions
            .entry(agent_id)
            .or_default();
        if !mentions.iter().any(|selected| {
            selected.kind == ComposerMentionKind::Project && selected.project_id == token.project_id
        }) {
            mentions.push(token);
        }
        self.agent_chat_project_selection.insert(agent_id, 0);
        self.agent_chat_project_dismissed_query.remove(&agent_id);
        cx.notify();
    }

    pub(super) fn move_agent_chat_context_picker(
        &mut self,
        agent: &AgentRecord,
        delta: i32,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(view) = self.active_agent_chat_slash_view(agent, cx) {
            if view.matches.is_empty() {
                return false;
            }
            let len = view.matches.len();
            let next = if delta > 0 {
                if view.selected + 1 >= len {
                    0
                } else {
                    view.selected + 1
                }
            } else if view.selected == 0 {
                len - 1
            } else {
                view.selected - 1
            };
            self.agent_chat_slash_selection.insert(agent.id, next);
            cx.notify();
            return true;
        }

        if let Some(view) = self.active_agent_chat_project_mention_view(agent, cx) {
            if view.matches.is_empty() {
                return false;
            }
            let len = view.matches.len();
            let next = if delta > 0 {
                if view.selected + 1 >= len {
                    0
                } else {
                    view.selected + 1
                }
            } else if view.selected == 0 {
                len - 1
            } else {
                view.selected - 1
            };
            self.agent_chat_project_selection.insert(agent.id, next);
            cx.notify();
            return true;
        }

        if let Some(view) = self.active_agent_chat_agent_mention_view(agent, cx) {
            if view.matches.is_empty() {
                return false;
            }
            let len = view.matches.len();
            let next = if delta > 0 {
                if view.selected + 1 >= len {
                    0
                } else {
                    view.selected + 1
                }
            } else if view.selected == 0 {
                len - 1
            } else {
                view.selected - 1
            };
            self.agent_chat_agent_selection.insert(agent.id, next);
            cx.notify();
            return true;
        }

        if let Some(view) = self.active_agent_chat_doc_mention_view(agent.id, agent.project_id, cx)
        {
            let len = view.total();
            if len == 0 {
                return false;
            }
            let next = if delta > 0 {
                if view.selected + 1 >= len {
                    0
                } else {
                    view.selected + 1
                }
            } else if view.selected == 0 {
                len - 1
            } else {
                view.selected - 1
            };
            self.agent_chat_doc_selection.insert(agent.id, next);
            cx.notify();
            return true;
        }

        if let Some(view) = self.active_agent_chat_file_mention_view(agent, cx) {
            if view.matches.is_empty() {
                return false;
            }
            let len = view.matches.len();
            let next = if delta > 0 {
                if view.selected + 1 >= len {
                    0
                } else {
                    view.selected + 1
                }
            } else if view.selected == 0 {
                len - 1
            } else {
                view.selected - 1
            };
            self.agent_chat_file_selection.insert(agent.id, next);
            cx.notify();
            return true;
        }

        false
    }

    pub(super) fn dismiss_agent_chat_context_picker(
        &mut self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(view) = self.active_agent_chat_slash_view(agent, cx) {
            self.agent_chat_slash_dismissed_query
                .insert(agent.id, view.query.query);
            cx.notify();
            return true;
        }
        if let Some(view) = self.active_agent_chat_project_mention_view(agent, cx) {
            self.agent_chat_project_dismissed_query
                .insert(agent.id, view.mention.query);
            cx.notify();
            return true;
        }
        if let Some(view) = self.active_agent_chat_agent_mention_view(agent, cx) {
            self.agent_chat_agent_dismissed_query
                .insert(agent.id, view.mention.query);
            cx.notify();
            return true;
        }
        if let Some(view) = self.active_agent_chat_doc_mention_view(agent.id, agent.project_id, cx)
        {
            self.agent_chat_doc_dismissed_query
                .insert(agent.id, view.mention.query);
            cx.notify();
            return true;
        }
        if let Some(view) = self.active_agent_chat_file_mention_view(agent, cx) {
            self.agent_chat_file_dismissed_query
                .insert(agent.id, view.mention.query);
            cx.notify();
            return true;
        }
        false
    }

    pub(super) fn accept_agent_chat_context_picker(
        &mut self,
        agent: &AgentRecord,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some(view) = self.active_agent_chat_slash_view(agent, cx) {
            let Some(command) = view.matches.get(view.selected).cloned() else {
                return false;
            };
            self.insert_agent_chat_slash_command(agent.id, input, command, view.query, window, cx);
            return true;
        }
        if let Some(view) = self.active_agent_chat_project_mention_view(agent, cx) {
            let Some(project) = view.matches.get(view.selected).cloned() else {
                return false;
            };
            self.insert_agent_chat_project_mention(
                agent.id,
                input,
                project,
                view.mention,
                window,
                cx,
            );
            return true;
        }
        if let Some(view) = self.active_agent_chat_agent_mention_view(agent, cx) {
            let Some(target) = view.matches.get(view.selected).cloned() else {
                return false;
            };
            self.insert_agent_chat_agent_target(agent.id, input, target, view.mention, window, cx);
            return true;
        }
        if let Some(view) = self.active_agent_chat_doc_mention_view(agent.id, agent.project_id, cx)
        {
            let design_count = view.designs.len();
            if view.selected < design_count {
                let Some(reference) = view.designs.get(view.selected).cloned() else {
                    return false;
                };
                self.insert_agent_chat_design_mention(
                    agent.id,
                    input,
                    reference,
                    view.mention,
                    window,
                    cx,
                );
                return true;
            }
            let doc_index = view.selected - design_count;
            let Some(relative_path) = view
                .matches
                .get(doc_index)
                .map(|doc| doc.relative_path.clone())
            else {
                return false;
            };
            self.insert_agent_chat_doc_mention(
                agent.id,
                input,
                relative_path,
                view.mention,
                window,
                cx,
            );
            return true;
        }
        if let Some(view) = self.active_agent_chat_file_mention_view(agent, cx) {
            let Some(file) = view.matches.get(view.selected).cloned() else {
                return false;
            };
            self.insert_agent_chat_file_mention(agent.id, input, file, view.mention, window, cx);
            return true;
        }
        false
    }

    pub(super) fn continue_proposed_plan(
        &mut self,
        agent_id: Uuid,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let draft = input.read(cx).value().trim().to_string();
        let pasted_text_blocks = self
            .agent_chat_pasted_text_blocks
            .get(&agent_id)
            .cloned()
            .unwrap_or_default();
        let draft = append_pasted_text_blocks(&draft, &pasted_text_blocks);
        let Some((submission_text, mode)) = self.agent_chats.update(cx, |chats, cx| {
            chats.resolve_proposed_plan_submission(agent_id, &draft, cx)
        }) else {
            return;
        };

        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.agent_chat_pasted_text_blocks.remove(&agent_id);
        self.dispatch_agent_chat_submission(agent_id, submission_text, mode, cx);
        self.acknowledge_agent_chat_seen(agent_id, cx);
    }

    pub(super) fn paste_image_into_agent_chat(
        &mut self,
        agent: &AgentRecord,
        announce: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let has_project = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == agent.project_id)
            .is_some();
        if !has_project {
            return false;
        }
        let Some(item) = cx.read_from_clipboard() else {
            if announce {
                self.agent_start_errors
                    .insert(agent.id, "Copy an image first, then attach it here.".into());
                cx.notify();
            }
            return false;
        };
        let Some(image) = clipboard_image_from_item(&item) else {
            if announce {
                self.agent_start_errors
                    .insert(agent.id, "Clipboard does not contain an image.".into());
                cx.notify();
            }
            return false;
        };
        match materialize_agent_clipboard_image(agent.id, &image) {
            Ok(path) => {
                self.agent_chat_attached_files
                    .entry(agent.id)
                    .or_default()
                    .push(path);
                self.agent_start_errors.remove(&agent.id);
                cx.notify();
                true
            }
            Err(error) => {
                if announce {
                    self.agent_start_errors
                        .insert(agent.id, format!("Could not attach image: {error:#}"));
                    cx.notify();
                }
                false
            }
        }
    }

    pub(super) fn paste_long_text_into_agent_chat(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(text) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .filter(|text| text_line_count(text) > LONG_PASTE_LINE_THRESHOLD)
        else {
            return false;
        };
        let line_count = text_line_count(&text);
        self.agent_chat_pasted_text_blocks
            .entry(agent_id)
            .or_default()
            .push(PastedTextBlock {
                id: Uuid::new_v4(),
                text,
                line_count,
                expanded: false,
            });
        cx.notify();
        true
    }

    pub(super) fn attach_agent_chat_paths(
        &mut self,
        agent_id: Uuid,
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

        let attachments = self.agent_chat_attached_files.entry(agent_id).or_default();
        for path in dropped_files {
            if !attachments.iter().any(|existing| existing == &path) {
                attachments.push(path);
            }
        }
        self.agent_start_errors.remove(&agent_id);
        cx.notify();
        true
    }

    pub(super) fn append_agent_chat_prompt_snippet(
        &mut self,
        input: Entity<InputState>,
        snippet: &'static str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.append_agent_chat_prompt_text(input, snippet.to_string(), window, cx);
    }

    pub(super) fn append_agent_chat_prompt_text(
        &mut self,
        input: Entity<InputState>,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        input.update(cx, |input, cx| {
            let mut value = input.value().to_string();
            if !value.trim().is_empty() && !value.ends_with('\n') {
                value.push('\n');
            }
            if !value.is_empty() && !value.ends_with("\n\n") {
                value.push('\n');
            }
            value.push_str(&text);
            input.set_value(value, window, cx);
            input.focus(window, cx);
        });
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VerificationLifecycle {
    NotStarted,
    Verifying,
    NeedsFix,
    Fixing,
    Declined,
    Complete,
}

/// The latest meaningful state in this agent's one verification lifecycle.
/// Ordinary user turns do not reset it: only a fix request advances a failed
/// verification. An all-met card or either declined decision closes it permanently.
pub(super) fn verification_lifecycle(timeline: &[AgentChatTimelineItem]) -> VerificationLifecycle {
    for (index, item) in timeline.iter().enumerate().rev() {
        match item {
            AgentChatTimelineItem::Verification(verification) => {
                let follow_up = is_follow_up_verification(timeline, index);
                let follow_up_has_no_clear_gaps = follow_up
                    && !verification.is_unparsed()
                    && verification
                        .items
                        .iter()
                        .all(|item| item.status != VerificationStatus::Missed);
                return if verification.all_met() || follow_up_has_no_clear_gaps {
                    VerificationLifecycle::Complete
                } else {
                    VerificationLifecycle::NeedsFix
                };
            }
            AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => {
                if text.starts_with(AGENT_VERIFY_DISMISS_MARKER)
                    || text.starts_with(AGENT_REVERIFY_DISMISS_MARKER)
                {
                    return VerificationLifecycle::Declined;
                }
                if text.starts_with(AGENT_VERIFY_REQUEST_MARKER) {
                    return VerificationLifecycle::Verifying;
                }
                if text.starts_with(AGENT_VERIFY_FIX_PREFIX) {
                    return VerificationLifecycle::Fixing;
                }
            }
            _ => {}
        }
    }
    VerificationLifecycle::NotStarted
}

/// A verification card is a follow-up when a verification-fix request already
/// exists earlier in this lifecycle. Follow-up cards intentionally ignore
/// Unclear outcomes: those remain conversational, not auto-fixable gaps.
pub(super) fn is_follow_up_verification(
    timeline: &[AgentChatTimelineItem],
    verification_index: usize,
) -> bool {
    timeline[..verification_index].iter().rev().any(|item| {
        matches!(
            item,
            AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. })
                if text.starts_with(AGENT_VERIFY_FIX_PREFIX)
        )
    })
}

/// Trim re-injected intent text to a character budget on a char boundary,
/// marking the cut so the agent knows the source continues.
fn bounded_verification_text(text: &str, max_chars: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let bounded: String = text.chars().take(max_chars).collect();
    format!("{}…\n[description truncated]", bounded.trim_end())
}

fn visual_review_image_metadata(bytes: &[u8]) -> Option<(&'static str, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some(("image/png", "png"))
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else {
        None
    }
}

fn visual_review_target_label(review: &ide_core::visual_review::VisualReviewSubmission) -> String {
    if let Some(element) = review.element.as_ref() {
        let name = if !element.accessible_name.trim().is_empty() {
            element.accessible_name.trim()
        } else if !element.text.trim().is_empty() {
            element.text.trim()
        } else {
            element.tag_name.trim()
        };
        return format!("Selected element · {} · {name}", element.tag_name);
    }
    if let Some(area) = review.area.as_ref() {
        return format!(
            "Marked image area · {:.0} × {:.0}",
            area.rect.width, area.rect.height
        );
    }
    "Preview visual review".to_string()
}

#[cfg(test)]
mod verification_trigger_tests {
    use super::*;

    #[test]
    fn preview_review_attachments_keep_their_real_image_format() {
        assert_eq!(
            visual_review_image_metadata(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
            Some(("image/png", "png"))
        );
        assert_eq!(
            visual_review_image_metadata(&[0xff, 0xd8, 0xff, 0xe0]),
            Some(("image/jpeg", "jpg"))
        );
        assert_eq!(visual_review_image_metadata(b"not an image"), None);
    }

    fn user_turn(text: &str) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Message(AgentChatMessage::User {
            text: text.to_string(),
            display_text: None,
            tags: Vec::new(),
            created_at: 0,
        })
    }

    fn assistant_turn(text: &str) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
            message_id: None,
            text: text.to_string(),
            created_at: 0,
        })
    }

    fn verification_card(all_met: bool) -> AgentChatTimelineItem {
        AgentChatTimelineItem::Verification(crate::state::agent_chat::Verification::new(
            "v1",
            if all_met {
                "## Met\n- requirement — evidence"
            } else {
                "## Missed\n- requirement — not implemented"
            },
        ))
    }

    #[test]
    fn empty_timeline_has_not_started() {
        assert_eq!(
            verification_lifecycle(&[]),
            VerificationLifecycle::NotStarted
        );
    }

    #[test]
    fn fresh_work_is_not_started() {
        let timeline = vec![user_turn("build the thing"), assistant_turn("done")];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::NotStarted
        );
    }

    #[test]
    fn verify_directive_is_verifying() {
        let timeline = vec![
            user_turn("build the thing"),
            assistant_turn("done"),
            user_turn(&format!("{AGENT_VERIFY_REQUEST_MARKER}\nVerify your work.")),
            assistant_turn("checking…"),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Verifying
        );
    }

    #[test]
    fn unmet_card_waits_for_fix() {
        let timeline = vec![
            user_turn("build the thing"),
            assistant_turn("done"),
            user_turn(&format!("{AGENT_VERIFY_REQUEST_MARKER}\nVerify your work.")),
            verification_card(false),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::NeedsFix
        );
    }

    #[test]
    fn fix_directive_reopens_verification() {
        let timeline = vec![
            user_turn("build the thing"),
            verification_card(false),
            user_turn(&format!("{AGENT_VERIFY_FIX_PREFIX}:\n- missed thing")),
            assistant_turn("fixed"),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Fixing
        );
    }

    #[test]
    fn declining_initial_verification_closes_it_for_later_turns() {
        let timeline = vec![
            user_turn("build the thing"),
            assistant_turn("done"),
            user_turn(AGENT_VERIFY_DISMISS_MARKER),
            user_turn("make one more adjustment"),
            assistant_turn("done"),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Declined
        );
    }

    #[test]
    fn declining_reverification_closes_it_for_later_turns() {
        let timeline = vec![
            user_turn("build the thing"),
            verification_card(false),
            user_turn(&format!("{AGENT_VERIFY_FIX_PREFIX}:\n- missed thing")),
            assistant_turn("fixed"),
            user_turn(AGENT_VERIFY_DISMISS_MARKER),
            user_turn("make one more adjustment"),
            assistant_turn("done"),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Declined
        );
    }

    #[test]
    fn legacy_reverification_dismissal_still_closes_the_lifecycle() {
        let timeline = vec![
            user_turn("build the thing"),
            verification_card(false),
            user_turn(&format!("{AGENT_VERIFY_FIX_PREFIX}:\n- missed thing")),
            assistant_turn("fixed"),
            user_turn(AGENT_REVERIFY_DISMISS_MARKER),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Declined
        );
    }

    #[test]
    fn follow_up_verification_can_complete_the_lifecycle() {
        let timeline = vec![
            user_turn("build the thing"),
            verification_card(false),
            user_turn(&format!("{AGENT_VERIFY_FIX_PREFIX}:\n- missed thing")),
            assistant_turn("fixed"),
            user_turn(&format!("{AGENT_VERIFY_REQUEST_MARKER}\nVerify again.")),
            verification_card(true),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Complete
        );
    }

    #[test]
    fn follow_up_unclear_items_are_not_treated_as_clear_gaps() {
        let timeline = vec![
            user_turn("build the thing"),
            verification_card(false),
            user_turn(&format!("{AGENT_VERIFY_FIX_PREFIX}:\n- missed thing")),
            assistant_turn("fixed"),
            user_turn(&format!("{AGENT_VERIFY_REQUEST_MARKER}\nVerify again.")),
            AgentChatTimelineItem::Verification(crate::state::agent_chat::Verification::new(
                "v2",
                "## Unclear\n- unrelated requirement — needs user clarification",
            )),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Complete
        );
    }

    #[test]
    fn ordinary_work_after_completion_does_not_restart_verification() {
        let timeline = vec![
            user_turn("build the thing"),
            verification_card(true),
            user_turn("now add dark mode"),
            assistant_turn("done"),
        ];
        assert_eq!(
            verification_lifecycle(&timeline),
            VerificationLifecycle::Complete
        );
    }
}

fn visual_review_target_context(
    review: &ide_core::visual_review::VisualReviewSubmission,
) -> String {
    if let Some(element) = review.element.as_ref() {
        return format!(
            "Target: exact DOM element\nTag: {}\nRole: {}\nAccessible name: {}\nSelector: {}\nVisible rect: x {:.0}, y {:.0}, width {:.0}, height {:.0}",
            element.tag_name,
            element.role,
            element.accessible_name,
            element.selector,
            element.rect.x,
            element.rect.y,
            element.rect.width,
            element.rect.height,
        );
    }
    if let Some(area) = review.area.as_ref() {
        return format!(
            "Target: arbitrary image area (visual crop only; this is not a DOM multi-selection)\nVisible rect: x {:.0}, y {:.0}, width {:.0}, height {:.0}",
            area.rect.x, area.rect.y, area.rect.width, area.rect.height,
        );
    }
    "Target: visual crop".to_string()
}
