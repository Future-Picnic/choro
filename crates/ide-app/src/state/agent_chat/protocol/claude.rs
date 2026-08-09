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
                self.assistant_stream.flush(&self.events);
                self.events
                    .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Cancelling))
                    .ok();
                self.write_json(&json!({ "type": "cancel_turn" }))?;
            }
            ChatBackendCommand::SendTurn { text, mode } => self.send_turn(text, mode)?,
            ChatBackendCommand::UpdateAccessMode { access_mode } => {
                self.access_mode = access_mode;
            }
            ChatBackendCommand::UpdateModelEffort { model, effort } => {
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
    ) -> anyhow::Result<()> {
        self.assistant_stream.reset(&self.events);
        self.assistant_buffer.clear();
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .ok();
        let design_preview_review = ide_core::penpot_assistant::is_preview_review_prompt(&text);
        self.write_json(&json!({
            "type": "send_turn",
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
            "systemPrompt": self.agent.hidden_doc_assistant.then_some(self.agent.doc.as_str()),
            "claudePath": self.claude_path.display().to_string(),
            "mcpServers": choro_mcp_servers_json(&self.agent),
            "designAssistant": is_design_assistant(&self.agent),
            "designPreviewReview": design_preview_review,
        }))
    }

    fn handle_message(&mut self, message: Value) -> anyhow::Result<()> {
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
            "changed_files" => {
                let files = message
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|file| {
                        let path = file.get("path").and_then(Value::as_str)?;
                        let additions =
                            file.get("additions").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let deletions =
                            file.get("deletions").and_then(Value::as_u64).unwrap_or(0) as usize;
                        Some(FileChangeStat::new(path, additions, deletions))
                    })
                    .collect::<Vec<_>>();
                if !files.is_empty() {
                    let summary = capture_changed_files_snapshot(
                        &self.agent,
                        ChangedFilesSummary {
                            files,
                            ..Default::default()
                        },
                        "agent_changed_files",
                    );
                    if !summary.files.is_empty() {
                        self.events
                            .send_blocking(ChatBackendEvent::ChangedFiles(summary))
                            .ok();
                    }
                }
            }
            "usage" => {
                if let Some(usage) = conversation_usage_from_bridge_message(message) {
                    self.events
                        .send_blocking(ChatBackendEvent::Usage(usage))
                        .ok();
                }
            }
            "status" => {
                let status = match message.get("status").and_then(Value::as_str) {
                    Some("running") => AgentChatStatus::Running,
                    Some("cancelling") => AgentChatStatus::Cancelling,
                    Some("idle") => AgentChatStatus::Idle,
                    Some("waiting_for_user") => AgentChatStatus::WaitingForUser,
                    Some("plan_ready") => AgentChatStatus::PlanReady,
                    Some("failed") => AgentChatStatus::Failed,
                    _ => return Ok(()),
                };
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
        let _ = self.write_json(&json!({ "type": "shutdown" }));
        terminate_child_process(&mut self.child);
    }
}
