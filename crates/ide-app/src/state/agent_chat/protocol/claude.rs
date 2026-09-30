use super::*;

impl ClaudeBridgeRuntime {
    pub(super) fn run_loop(&mut self) -> anyhow::Result<()> {
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                self.assistant_stream.flush(&self.events);
                break;
            }
            match next_backend_inbound(
                &self.commands,
                &self.messages,
                self.assistant_stream.has_pending(),
            ) {
                BackendInbound::Command(ChatBackendCommand::Shutdown) => {
                    self.assistant_stream.flush(&self.events);
                    return Ok(());
                }
                BackendInbound::Command(ChatBackendCommand::ForceShutdown) => {
                    self.assistant_stream.flush(&self.events);
                    return Err(anyhow!("Claude bridge force-stopped"));
                }
                BackendInbound::Command(command) => self.handle_command(command)?,
                BackendInbound::CommandsClosed => return Ok(()),
                BackendInbound::Message(message) => {
                    self.handle_message(message)?;
                    self.assistant_stream.flush_due(&self.events);
                }
                BackendInbound::FlushTick => self.assistant_stream.flush_due(&self.events),
                BackendInbound::MessagesClosed => break,
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, command: ChatBackendCommand) -> anyhow::Result<()> {
        match command {
            ChatBackendCommand::Shutdown => return Ok(()),
            ChatBackendCommand::ForceShutdown => {
                return Err(anyhow!("Claude bridge force-stopped"));
            }
            ChatBackendCommand::CancelTurn => {
                self.studio_review.cancel();
                ide_core::studio::revoke_agent_scope(&self.agent);
                self.assistant_stream.flush(&self.events);
                self.events
                    .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Cancelling))
                    .ok();
                self.write_json(&json!({ "type": "cancel_turn" }))?;
            }
            ChatBackendCommand::SendTurn {
                text,
                mode,
                read_only,
                turn_id,
            } => self.send_turn(text, mode, read_only, turn_id)?,
            ChatBackendCommand::UpdateAccessMode { access_mode } => {
                self.access_mode = access_mode;
            }
            ChatBackendCommand::UpdateModelEffort { model, effort, .. } => {
                self.model = model.cli_value().map(str::to_string);
                self.effort = effort.cli_value().to_string();
            }
            ChatBackendCommand::SubmitUserInput {
                request_id,
                answers,
            } => {
                self.write_json(&json!({
                    "type": "submit_user_input",
                    "request_id": request_id,
                    "answers": answers,
                }))?;
            }
            ChatBackendCommand::ResolveApproval {
                request_id,
                approved,
            } => {
                self.write_json(&json!({
                    "type": "resolve_approval",
                    "request_id": request_id,
                    "approved": approved,
                }))?;
            }
        }
        Ok(())
    }

    pub(super) fn send_turn(
        &mut self,
        text: String,
        mode: AgentInteractionMode,
        read_only: bool,
        turn_id: String,
    ) -> anyhow::Result<()> {
        let text = ide_core::studio::attach_request_context(&self.agent, text)?;
        let mode = super::managed::interaction_mode(&self.agent, mode);
        let system_prompt = if self.agent.hidden_doc_assistant {
            format!(
                "{}\n\n{}",
                self.agent.doc,
                super::CHORO_NATIVE_TOOL_INSTRUCTIONS
            )
        } else {
            super::CHORO_NATIVE_TOOL_INSTRUCTIONS.to_string()
        };
        let system_prompt = super::managed::instructions(system_prompt, &self.agent)?;
        let system_prompt = super::append_choro_visualization_instructions(
            system_prompt,
            self.visualization_dir.as_deref(),
        );
        self.studio_review
            .begin(self.agent.studio_context.is_some());
        self.assistant_stream.reset(&self.events);
        self.assistant_buffer.clear();
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .ok();
        self.write_json(&json!({
            "type": "send_turn",
            "turn_id": turn_id,
            "text": text,
            "cwd": self.agent.runtime_path(),
            "sessionId": self.agent.cli_session_id,
            "mode": match mode {
                AgentInteractionMode::Default => "default",
                AgentInteractionMode::Plan => "plan",
            },
            "model": self.model,
            "effort": self.effort,
            "accessMode": self.access_mode.claude_permission_mode(),
            "systemPrompt": system_prompt,
            "managedDelegation": self.agent.delegation.is_some(),
            "managedChild": super::managed::is_child(&self.agent),
            "managedConsultation": super::managed::consultation(&self.agent),
            "visualizationDir": self.visualization_dir,
            "claudePath": self.claude_path.display().to_string(),
            "mcpServers": choro_mcp_servers_json(&self.agent),
            "studioAssistant": self.agent.studio_context.is_some(),
            "readOnly": read_only,
        }))
    }

    fn handle_message(&mut self, message: impl Into<ProviderMessage>) -> anyhow::Result<()> {
        let ProviderMessage { value: message, _reservation } = message.into();
        if message.get("_choro_evidence_incomplete").and_then(Value::as_bool) == Some(true) { self.events.mark_evidence_overflow(); }
        let Some(event_type) = message.get("type").and_then(Value::as_str) else {
            return Ok(());
        };
        if event_type != "assistant_chunk" {
            self.assistant_stream.flush(&self.events);
        }
        match event_type {
            "assistant_chunk" => {
                if let Some(text) = message.get("text").and_then(Value::as_str) {
                    self.assistant_buffer.push_str(text);
                    self.assistant_stream.push(
                        message
                            .get("message_id")
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        text,
                        &self.events,
                    );
                }
            }
            "session_ready" => {
                if let Some(session_id) = message.get("session_id").and_then(Value::as_str) {
                    self.events
                        .send_blocking(ChatBackendEvent::SessionReady {
                            session_id: session_id.to_string(),
                        })
                        .ok();
                }
            }
            "thought_chunk" => {
                if let Some(text) = message.get("text").and_then(Value::as_str) {
                    self.events
                        .send_blocking(ChatBackendEvent::ThoughtChunk {
                            message_id: message
                                .get("message_id")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            text: text.to_string(),
                        })
                        .ok();
                }
            }
            "work_log" => {
                if let Some(entry) = work_log_from_bridge_event(&message) {
                    self.events
                        .send_blocking(ChatBackendEvent::WorkLog(entry))
                        .ok();
                }
            }
            "file_change_activity" => {
                let activity = message
                    .get("file")
                    .and_then(|file| file_change_stat_from_bridge(file, false))
                    .map(|file| {
                        FileChangeActivity::new(
                            message
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("claude-file-change"),
                            message
                                .get("turn_id")
                                .and_then(Value::as_str)
                                .unwrap_or("claude-turn"),
                            file.with_count_projection(
                                message
                                    .get("observed")
                                    .and_then(Value::as_bool)
                                    .unwrap_or(false),
                            ),
                            message
                                .get("observed")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                            unix_now(),
                        )
                    });
                if let Some(activity) = activity {
                    self.events
                        .send_blocking(ChatBackendEvent::FileChangeActivity(activity))
                        .ok();
                }
            }
            "pending_user_input" => {
                let pending = pending_user_input_from_bridge_event(&message);
                self.events
                    .send_blocking(ChatBackendEvent::PendingUserInput(pending))
                    .ok();
            }
            "pending_approval" => {
                let request_id = message
                    .get("request_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(next_request_id);
                let kind = match message.get("kind").and_then(Value::as_str) {
                    Some("command") => PendingApprovalKind::Command,
                    Some("file_change") => PendingApprovalKind::FileChange,
                    _ => PendingApprovalKind::Permissions,
                };
                let title = message
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Allow this action?");
                let detail = message
                    .get("detail")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|detail| !detail.is_empty())
                    .map(str::to_string);
                self.events
                    .send_blocking(ChatBackendEvent::PendingApproval(PendingApproval::new(
                        request_id, kind, title, detail,
                    )))
                    .ok();
            }
            "proposed_plan" => {
                if super::managed::is_child(&self.agent) {
                    return Ok(());
                }
                if let Some(markdown) = message
                    .get("markdown")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|markdown| !markdown.is_empty())
                {
                    let id = message
                        .get("id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(next_request_id);
                    self.events
                        .send_blocking(ChatBackendEvent::ProposedPlan(ProposedPlan::new(
                            id, markdown,
                        )))
                        .ok();
                }
            }
            "evidence_overflow" => self.events.mark_evidence_overflow(),
            "changed_files" => {
                let turn = message.get("turn_id").and_then(Value::as_str).unwrap_or("claude-turn");
                self.events.send_blocking(ChatBackendEvent::ChangedFiles(ChangedFilesSummary::attributed(turn, vec![], vec![]))).ok();
            }
            "usage" => {
                if let Some(usage) = conversation_usage_from_bridge_message(message) {
                    self.events
                        .send_blocking(ChatBackendEvent::Usage(usage))
                        .ok();
                }
            }
            "compaction" => {
                if let Some(active) = message.get("active").and_then(Value::as_bool) {
                    self.events
                        .send_blocking(ChatBackendEvent::Compaction(active))
                        .ok();
                }
            }
            "status" => {
                let mut status = match message.get("status").and_then(Value::as_str) {
                    Some("running") => AgentChatStatus::Running,
                    Some("cancelling") => AgentChatStatus::Cancelling,
                    Some("idle") => AgentChatStatus::Idle,
                    Some("waiting_for_user") => AgentChatStatus::WaitingForUser,
                    Some("plan_ready") if super::managed::is_child(&self.agent) => {
                        AgentChatStatus::Idle
                    }
                    Some("plan_ready") => AgentChatStatus::PlanReady,
                    Some("failed") => AgentChatStatus::Failed,
                    _ => return Ok(()),
                };
                if status == AgentChatStatus::Idle && self.studio_review.complete() {
                    if let Err(error) = ide_core::studio::verify_agent_completion(&self.agent) {
                        self.events
                            .send_blocking(ChatBackendEvent::Error(format!("{error:#}")))
                            .ok();
                        status = AgentChatStatus::Failed;
                    }
                }
                if matches!(status, AgentChatStatus::Idle | AgentChatStatus::Failed) {
                    self.studio_review.cancel();
                    ide_core::studio::revoke_agent_scope(&self.agent);
                }
                if status == AgentChatStatus::Idle {
                    self.emit_code_review_from_buffer();
                }
                self.events
                    .send_blocking(ChatBackendEvent::Status(status))
                    .ok();
            }
            "error" => {
                let error = message
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("Claude bridge returned an error.")
                    .to_string();
                self.events
                    .send_blocking(ChatBackendEvent::Error(error))
                    .ok();
            }
            _ => {}
        }
        Ok(())
    }

    fn emit_code_review_from_buffer(&mut self) {
        if let Some(review) = extract_code_review(&self.assistant_buffer) {
            self.events
                .send_blocking(ChatBackendEvent::CodeReview(CodeReview::new(
                    next_request_id(),
                    review,
                )))
                .ok();
        }
        if let Some(verification) = extract_verification(&self.assistant_buffer) {
            self.events
                .send_blocking(ChatBackendEvent::Verification(Verification::new(
                    next_request_id(),
                    verification,
                )))
                .ok();
        }
        if let Some(checklist) = extract_review_checklist(&self.assistant_buffer) {
            self.events
                .send_blocking(ChatBackendEvent::ReviewChecklist(ReviewChecklist::ready(
                    "",
                    &checklist,
                    unix_now(),
                )))
                .ok();
        }
        self.assistant_buffer.clear();
    }

    fn write_json(&self, value: &Value) -> anyhow::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("Claude bridge stdin lock poisoned"))?;
        writeln!(stdin, "{}", serde_json::to_string(value)?)?;
        stdin.flush()?;
        Ok(())
    }
}

fn file_change_stat_from_bridge(file: &Value, projection: bool) -> Option<FileChangeStat> {
    let path = file.get("path").and_then(Value::as_str)?;
    let additions = file.get("additions").and_then(Value::as_u64).unwrap_or(0) as usize;
    let deletions = file.get("deletions").and_then(Value::as_u64).unwrap_or(0) as usize;
    let mut stat = FileChangeStat::new(path, additions, deletions)
            .with_count_projection(projection)
            .with_cleared_projection(
                file.get("clears_projection")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            )
            .with_content_hashes(
                file.get("baseline_hash")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                file.get("result_hash")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            )
            .with_content_projection(
                file.get("baseline_content")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                file.get("result_content")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            );
    stat.counts_unavailable = true;
    Some(stat)
}

fn conversation_usage_from_bridge_message(message: Value) -> Option<ConversationUsage> {
    serde_json::from_value(message).ok()
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    #[test]
    fn parses_claude_bridge_usage_snapshot() {
        let usage = conversation_usage_from_bridge_message(json!({
            "type": "usage",
            "session_id": "claude-session",
            "totals": {
                "reported_total_tokens": 1_650,
                "input_tokens": 1_000,
                "output_tokens": 250,
                "reasoning_tokens": 0,
                "cache_read_tokens": 350,
                "cache_write_tokens": 50,
                "cost_usd": 0
            },
            "latest_turn": {
                "reported_total_tokens": 650,
                "input_tokens": 300,
                "output_tokens": 100,
                "reasoning_tokens": 0,
                "cache_read_tokens": 200,
                "cache_write_tokens": 50,
                "cost_usd": 0
            },
            "models": [{
                "provider_id": "Claude",
                "model_id": "claude-sonnet-4-6",
                "totals": {
                    "reported_total_tokens": 1_650,
                    "input_tokens": 1_000,
                    "output_tokens": 250,
                    "reasoning_tokens": 0,
                    "cache_read_tokens": 350,
                    "cache_write_tokens": 50,
                    "cost_usd": 0
                }
            }]
        }))
        .expect("usage");

        assert_eq!(usage.session_id, "claude-session");
        assert_eq!(usage.totals.total_tokens(), 1_650);
        assert_eq!(usage.latest_turn.as_ref().unwrap().total_tokens(), 650);
        assert_eq!(usage.models[0].model_id, "claude-sonnet-4-6");
        assert_eq!(usage.totals.cost_usd, 0.0);
    }
}

impl Drop for ClaudeBridgeRuntime {
    fn drop(&mut self) {
        ide_core::studio::revoke_agent_scope(&self.agent);
        let _ = self.write_json(&json!({ "type": "shutdown" }));
        terminate_child_process(&mut self.child);
    }
}
