use super::*;

impl CodexRuntime {
    pub(super) fn run_loop(&mut self) -> anyhow::Result<()> {
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                self.assistant_stream.flush(&self.events);
                break;
            }
            if let Some((text, mode)) = self.deferred_turns.pop_front() {
                self.send_turn(text, mode)?;
                continue;
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
                    return Err(anyhow!("Codex app-server force-stopped"));
                }
                BackendInbound::Command(command) => self.handle_command(command)?,
                BackendInbound::CommandsClosed => return Ok(()),
                BackendInbound::Message(message) => {
                    self.handle_message(message)?;
                    self.assistant_stream.flush_due(&self.events);
                }
                BackendInbound::FlushTick => self.assistant_stream.flush_due(&self.events),
                BackendInbound::MessagesClosed => {
                    let detail = match self.child.try_wait() {
                        Ok(Some(status)) => format!("Codex app-server exited with status {status}"),
                        Ok(None) => {
                            "Codex app-server stdout closed while the process was still running"
                                .to_string()
                        }
                        Err(error) => format!("Codex app-server stdout closed: {error}"),
                    };
                    return Err(anyhow!(detail));
                }
            }
        }
        Ok(())
    }

    pub(super) fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.request_with_timeout(method, params, Duration::from_secs(120))
    }

    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> anyhow::Result<Value> {
        let id = next_request_id();
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;

        let deadline = Instant::now() + timeout;
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return Err(anyhow!("Codex app-server is shutting down"));
            }
            if Instant::now() >= deadline {
                return Err(anyhow!("timed out waiting for {method} response"));
            }
            self.handle_commands_while_blocked()?;
            let message = match self.messages.recv_timeout(Duration::from_millis(40)) {
                Ok(message) => message,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    self.assistant_stream.flush_due(&self.events);
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("Codex app-server stdout closed"));
                }
            };
            if message.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                if let Some(error) = message.get("error") {
                    self.assistant_stream.flush(&self.events);
                    return Err(anyhow!("{method} returned error: {error}"));
                }
                self.assistant_stream.flush(&self.events);
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            self.handle_message(message)?;
            self.assistant_stream.flush_due(&self.events);
        }
    }

    pub(super) fn notify(&mut self, method: &str, params: Value) -> anyhow::Result<()> {
        let mut message = json!({
            "jsonrpc": "2.0",
            "method": method,
        });
        if !params.is_null() {
            message["params"] = params;
        }
        self.write_json(&message)
    }

    pub(super) fn send_turn(
        &mut self,
        text: String,
        mode: AgentInteractionMode,
    ) -> anyhow::Result<()> {
        let Some(thread_id) = self.thread_id.clone() else {
            return Err(anyhow!("Codex thread is not started"));
        };
        self.finish_reconnect(
            WorkLogStatus::Completed,
            "Reconnect superseded by a new turn",
            None,
        );
        self.assistant_stream.reset(&self.events);
        self.assistant_buffer.clear();
        self.plan_buffer.clear();
        self.pending_changed_files = None;
        self.pending_observed_files.clear();
        self.active_turn_id = next_request_id();
        self.command_ran_this_turn = false;
        self.active_command_item_id = None;
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .ok();
        let mode_name = match mode {
            AgentInteractionMode::Default => "default",
            AgentInteractionMode::Plan => "plan",
        };
        let mode_instructions = match mode {
            AgentInteractionMode::Default => CODEX_DEFAULT_MODE_DEVELOPER_INSTRUCTIONS,
            AgentInteractionMode::Plan => CODEX_PLAN_MODE_DEVELOPER_INSTRUCTIONS,
        };
        let developer_instructions = codex_developer_instructions(
            mode_instructions,
            self.visualization_dir.as_deref(),
            self.agent
                .hidden_doc_assistant
                .then_some(self.agent.doc.as_str()),
        );
        let design_assistant = is_design_assistant(&self.agent);
        let design_preview_review = ide_core::penpot_assistant::is_preview_review_prompt(&text);
        let sandbox_policy = if design_assistant && !design_preview_review {
            json!({ "type": "readOnly" })
        } else if design_assistant {
            // Compare Review writes implementation code, but remains confined
            // to the project even when the saved access mode is Full Access.
            codex_turn_workspace_sandbox_policy(self.visualization_dir.as_deref())
        } else {
            codex_turn_sandbox_policy(self.access_mode, self.visualization_dir.as_deref())
        };
        let approval_policy = if design_assistant && !design_preview_review {
            "never"
        } else if design_assistant {
            self.access_mode.codex_approval_policy()
        } else {
            self.access_mode.codex_approval_policy()
        };
        let _ = self.request(
            "turn/start",
            json!({
                "threadId": thread_id,
                "input": [{"type": "text", "text": text}],
                "approvalPolicy": approval_policy,
                "sandboxPolicy": sandbox_policy,
                "model": self.model,
                "effort": self.effort,
                "collaborationMode": {
                    "mode": mode_name,
                    "settings": {
                        "model": self.model.clone().unwrap_or_else(|| "gpt-5.5".to_string()),
                        "reasoning_effort": self.effort,
                        "developer_instructions": developer_instructions
                    }
                }
            }),
        )?;
        Ok(())
    }

    fn handle_command(&mut self, command: ChatBackendCommand) -> anyhow::Result<()> {
        match command {
            ChatBackendCommand::Shutdown => return Ok(()),
            ChatBackendCommand::ForceShutdown => {
                return Err(anyhow!("Codex app-server force-stopped"));
            }
            ChatBackendCommand::CancelTurn => self.cancel_turn()?,
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
            } => self.submit_user_input(request_id, answers)?,
            ChatBackendCommand::ResolveApproval {
                request_id,
                approved,
            } => self.resolve_approval(request_id, approved)?,
        }
        Ok(())
    }

    fn handle_commands_while_blocked(&mut self) -> anyhow::Result<()> {
        loop {
            match self.commands.try_recv() {
                Ok(ChatBackendCommand::Shutdown) => {
                    self.assistant_stream.flush(&self.events);
                    return Err(anyhow!("Codex app-server is shutting down"));
                }
                Ok(ChatBackendCommand::ForceShutdown) => {
                    self.assistant_stream.flush(&self.events);
                    return Err(anyhow!("Codex app-server force-stopped"));
                }
                Ok(ChatBackendCommand::CancelTurn) => self.cancel_turn()?,
                Ok(ChatBackendCommand::UpdateAccessMode { access_mode }) => {
                    self.access_mode = access_mode;
                }
                Ok(ChatBackendCommand::UpdateModelEffort { model, effort }) => {
                    self.model = model.cli_value().map(str::to_string);
                    self.effort = effort.cli_value().to_string();
                }
                Ok(ChatBackendCommand::SubmitUserInput {
                    request_id,
                    answers,
                }) => self.submit_user_input(request_id, answers)?,
                Ok(ChatBackendCommand::ResolveApproval {
                    request_id,
                    approved,
                }) => self.resolve_approval(request_id, approved)?,
                Ok(ChatBackendCommand::SendTurn { text, mode }) => {
                    self.deferred_turns.push_back((text, mode));
                    self.events
                        .send_blocking(ChatBackendEvent::WorkLog(
                            WorkLogEntry::new(
                                next_request_id(),
                                "codex-startup-queued",
                                WorkLogEntryKind::System,
                                "Message queued while Codex starts",
                                WorkLogStatus::Pending,
                            )
                            .detail(Some(
                                "The turn will run after the Codex backend thread is ready."
                                    .to_string(),
                            )),
                        ))
                        .ok();
                }
                Err(crossbeam_channel::TryRecvError::Empty) => return Ok(()),
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    return Err(anyhow!("chat command channel closed"));
                }
            }
        }
    }

    fn submit_user_input(
        &mut self,
        request_id: String,
        answers: Vec<String>,
    ) -> anyhow::Result<()> {
        if let Some(pending) = self.pending_user_inputs.remove(&request_id) {
            let answer_map = pending
                .question_ids
                .into_iter()
                .zip(answers)
                .map(|(question_id, answer)| (question_id, json!({ "answers": [answer] })))
                .collect::<serde_json::Map<_, _>>();
            self.write_json(&json!({
                "jsonrpc": "2.0",
                "id": pending.jsonrpc_id,
                "result": { "answers": answer_map }
            }))?;
        } else {
            self.events
                .send_blocking(ChatBackendEvent::WorkLog(
                    WorkLogEntry::new(
                        next_request_id(),
                        "codex-user-input-missing",
                        WorkLogEntryKind::UserInput,
                        "Question answer could not be matched to an active request",
                        WorkLogStatus::Failed,
                    )
                    .detail(Some(request_id)),
                ))
                .ok();
        }
        Ok(())
    }

    fn resolve_approval(&mut self, request_id: String, approved: bool) -> anyhow::Result<()> {
        let Some(pending) = self.pending_approvals.remove(&request_id) else {
            self.events
                .send_blocking(ChatBackendEvent::WorkLog(
                    WorkLogEntry::new(
                        next_request_id(),
                        "codex-approval-missing",
                        WorkLogEntryKind::System,
                        "Approval could not be matched to an active request",
                        WorkLogStatus::Failed,
                    )
                    .detail(Some(request_id)),
                ))
                .ok();
            return Ok(());
        };
        let result = match pending.response_kind {
            PendingApprovalResponseKind::Decision => {
                json!({ "decision": if approved { "accept" } else { "decline" } })
            }
            PendingApprovalResponseKind::Permissions(requested) => json!({
                "permissions": if approved { requested } else { json!({}) },
                "scope": "turn"
            }),
        };
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": pending.jsonrpc_id,
            "result": result
        }))
    }

    fn cancel_turn(&mut self) -> anyhow::Result<()> {
        self.assistant_stream.flush(&self.events);
        self.finish_reconnect(WorkLogStatus::Completed, "Reconnect stopped", None);
        self.deny_all_pending_approvals()?;
        if let Some(thread_id) = self.thread_id.clone() {
            self.notify("turn/cancel", json!({ "threadId": thread_id }))?;
        }
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Cancelling))
            .ok();
        Ok(())
    }

    fn deny_all_pending_approvals(&mut self) -> anyhow::Result<()> {
        let request_ids = self.pending_approvals.keys().cloned().collect::<Vec<_>>();
        for request_id in request_ids {
            self.resolve_approval(request_id, false)?;
        }
        Ok(())
    }

    fn handle_message(&mut self, message: Value) -> anyhow::Result<()> {
        if message.get("method").is_some() && message.get("id").is_some() {
            if message
                .get("method")
                .and_then(Value::as_str)
                .is_some_and(codex_event_confirms_recovery)
            {
                self.finish_reconnect(WorkLogStatus::Completed, "Reconnected to Codex", None);
            }
            return self.handle_server_request(message);
        }

        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(());
        };
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        if codex_event_confirms_recovery(method) {
            self.finish_reconnect(WorkLogStatus::Completed, "Reconnected to Codex", None);
        }
        if method != "item/agentMessage/delta" {
            self.assistant_stream.flush(&self.events);
        }
        match method {
            "item/agentMessage/delta" => {
                if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                    self.assistant_buffer.push_str(delta);
                    self.assistant_stream
                        .push(item_id_from_params(&params), delta, &self.events);
                }
            }
            "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                    self.events
                        .send_blocking(ChatBackendEvent::ThoughtChunk {
                            message_id: item_id_from_params(&params),
                            text: delta.to_string(),
                        })
                        .ok();
                }
            }
            "turn/plan/updated" => {
                let entry = WorkLogEntry::new(
                    "codex-plan",
                    "codex-plan",
                    WorkLogEntryKind::Plan,
                    format_plan_summary(&params),
                    WorkLogStatus::InProgress,
                )
                .detail(format_plan_detail(&params));
                self.events
                    .send_blocking(ChatBackendEvent::WorkLog(entry))
                    .ok();
            }
            "item/started" => {
                if item_type_from_params(&params) == Some("commandExecution") {
                    self.command_ran_this_turn = true;
                    self.active_command_item_id =
                        item_id_from_params(&params).or_else(|| Some(next_request_id()));
                }
                if params
                    .get("item")
                    .and_then(|item| item.get("type"))
                    .and_then(Value::as_str)
                    == Some("plan")
                {
                    self.plan_buffer.clear();
                }
                if let Some(entry) = work_log_from_item(&params, WorkLogStatus::InProgress) {
                    self.events
                        .send_blocking(ChatBackendEvent::WorkLog(entry))
                        .ok();
                }
            }
            "item/completed" => {
                if let Some(plan) = plan_text_from_completed_item(&params).or_else(|| {
                    (!self.plan_buffer.trim().is_empty()).then(|| self.plan_buffer.clone())
                }) {
                    self.events
                        .send_blocking(ChatBackendEvent::ProposedPlan(ProposedPlan::new(
                            next_request_id(),
                            plan,
                        )))
                        .ok();
                    return Ok(());
                }
                if let Some(markdown) = generated_image_markdown_from_item(&params) {
                    let message_id = item_id_from_params(&params)
                        .map(|id| format!("generated-image-{id}"))
                        .unwrap_or_else(|| format!("generated-image-{}", next_request_id()));
                    self.events
                        .send_blocking(ChatBackendEvent::AssistantChunk {
                            message_id: Some(message_id),
                            text: markdown,
                        })
                        .ok();
                }
                if let Some(entry) = work_log_from_item(&params, WorkLogStatus::Completed) {
                    self.events
                        .send_blocking(ChatBackendEvent::WorkLog(entry))
                        .ok();
                }
            }
            "item/plan/delta" => {
                if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                    self.plan_buffer.push_str(delta);
                }
            }
            "item/fileChange/patchUpdated" => {
                let action_id = item_id_from_params(&params)
                    .unwrap_or_else(|| format!("file-change-{}", self.active_turn_id));
                let files = params
                    .get("changes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(file_stat_from_patch_change)
                    .collect::<Vec<_>>();
                if !files.is_empty() {
                    for file in &files {
                        let activity_id =
                            format!("codex:{action_id}:{}", file.path.to_string_lossy());
                        self.events
                            .send_blocking(ChatBackendEvent::FileChangeActivity(
                                FileChangeActivity::new(
                                    activity_id,
                                    self.active_turn_id.clone(),
                                    file.clone(),
                                    false,
                                    unix_now(),
                                ),
                            ))
                            .ok();
                    }
                    let summary = self
                        .pending_changed_files
                        .get_or_insert_with(ChangedFilesSummary::default);
                    upsert_file_change_stats(&mut summary.files, files);
                }
            }
            "turn/diff/updated" => {
                if let Some(diff) = self
                    .command_ran_this_turn
                    .then(|| params.get("diff").and_then(Value::as_str))
                    .flatten()
                {
                    let files = changed_files_from_unified_diff(diff);
                    let exact_paths = self
                        .pending_changed_files
                        .as_ref()
                        .into_iter()
                        .flat_map(|summary| &summary.files)
                        .map(|file| file.path.clone())
                        .collect::<HashSet<_>>();
                    let observed = files
                        .into_iter()
                        .filter(|file| !exact_paths.contains(&file.path))
                        .map(FileChangeStat::as_count_projection)
                        .collect::<Vec<_>>();
                    let action_id = self.active_command_item_id.as_deref().unwrap_or("command");
                    for file in &observed {
                        let activity_id =
                            format!("codex:{action_id}:{}", file.path.to_string_lossy());
                        self.events
                            .send_blocking(ChatBackendEvent::FileChangeActivity(
                                FileChangeActivity::new(
                                    activity_id,
                                    self.active_turn_id.clone(),
                                    file.clone(),
                                    true,
                                    unix_now(),
                                ),
                            ))
                            .ok();
                    }
                    upsert_file_change_stats(&mut self.pending_observed_files, observed);
                }
            }
            "thread/tokenUsage/updated" => {
                if let Some(usage) = codex_conversation_usage(&params, self.model.as_deref()) {
                    self.events
                        .send_blocking(ChatBackendEvent::Usage(usage))
                        .ok();
                }
            }
            "turn/completed" => {
                self.pending_approvals.clear();
                self.emit_pending_changed_files();
                let review = extract_code_review(&self.assistant_buffer);
                let verification = extract_verification(&self.assistant_buffer);
                if review.is_some() || verification.is_some() {
                    if let Some(review) = review {
                        self.events
                            .send_blocking(ChatBackendEvent::CodeReview(CodeReview::new(
                                next_request_id(),
                                review,
                            )))
                            .ok();
                    }
                    if let Some(verification) = verification {
                        self.events
                            .send_blocking(ChatBackendEvent::Verification(Verification::new(
                                next_request_id(),
                                verification,
                            )))
                            .ok();
                    }
                    self.events
                        .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle))
                        .ok();
                } else if let Some(plan) = extract_proposed_plan(&self.assistant_buffer) {
                    self.events
                        .send_blocking(ChatBackendEvent::ProposedPlan(ProposedPlan::new(
                            next_request_id(),
                            plan,
                        )))
                        .ok();
                } else {
                    self.events
                        .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle))
                        .ok();
                }
            }
            "error" => {
                match codex_error_disposition(&params) {
                    CodexErrorDisposition::Retry {
                        message,
                        detail,
                        turn_id,
                    } => {
                        let id = self
                            .active_reconnect_work_log_id
                            .get_or_insert_with(|| {
                                turn_id
                                    .map(|turn_id| format!("codex-reconnect-{turn_id}"))
                                    .unwrap_or_else(|| {
                                        format!("codex-reconnect-{}", next_request_id())
                                    })
                            })
                            .clone();
                        self.events
                            .send_blocking(ChatBackendEvent::WorkLog(
                                WorkLogEntry::new(
                                    id.clone(),
                                    id,
                                    WorkLogEntryKind::System,
                                    message,
                                    WorkLogStatus::InProgress,
                                )
                                .detail(detail),
                            ))
                            .ok();
                        // A retry notification is progress within the active turn,
                        // not a terminal backend failure. Keeping Running makes new
                        // composer submissions queue until this turn really ends.
                        self.events
                            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
                            .ok();
                    }
                    CodexErrorDisposition::Terminal { message } => {
                        self.finish_reconnect(
                            WorkLogStatus::Failed,
                            "Could not reconnect to Codex",
                            Some(message.clone()),
                        );
                        self.pending_approvals.clear();
                        self.events
                            .send_blocking(ChatBackendEvent::Error(message))
                            .ok();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn finish_reconnect(&mut self, status: WorkLogStatus, title: &str, detail: Option<String>) {
        let Some(id) = self.active_reconnect_work_log_id.take() else {
            return;
        };
        self.events
            .send_blocking(ChatBackendEvent::WorkLog(
                WorkLogEntry::new(id.clone(), id, WorkLogEntryKind::System, title, status)
                    .detail(detail),
            ))
            .ok();
    }

    fn emit_pending_changed_files(&mut self) {
        let mut summary = self.pending_changed_files.take().unwrap_or_default();
        summary.observed_files = std::mem::take(&mut self.pending_observed_files);
        summary.turn_id = Some(self.active_turn_id.clone());
        summary.attribution_version = 1;
        if !summary.is_empty() {
            let summary =
                capture_changed_files_snapshot(&self.agent, summary, "agent_changed_files");
            if !summary.is_empty() {
                self.events
                    .send_blocking(ChatBackendEvent::ChangedFiles(summary))
                    .ok();
            }
        }
    }

    fn handle_server_request(&mut self, message: Value) -> anyhow::Result<()> {
        let id_value = message.get("id").cloned().unwrap_or(Value::Null);
        let request_key = jsonrpc_id_key(&id_value);
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        match method {
            "item/tool/requestUserInput" => {
                let pending = pending_user_input_from_codex_request(request_key, &params);
                let question_ids = pending
                    .questions
                    .iter()
                    .map(|question| question.id.clone())
                    .collect::<Vec<_>>();
                let request_id = pending.request_id.clone();
                self.pending_user_inputs.insert(
                    request_id,
                    PendingRequest {
                        jsonrpc_id: id_value,
                        question_ids,
                    },
                );
                self.events
                    .send_blocking(ChatBackendEvent::PendingUserInput(pending))
                    .ok();
            }
            "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
                let pending =
                    pending_approval_from_codex_request(request_key.clone(), method, &params);
                self.pending_approvals.insert(
                    request_key,
                    PendingApprovalRequest {
                        jsonrpc_id: id_value,
                        response_kind: PendingApprovalResponseKind::Decision,
                    },
                );
                self.events
                    .send_blocking(ChatBackendEvent::PendingApproval(pending))
                    .ok();
            }
            "item/permissions/requestApproval" => {
                let pending =
                    pending_approval_from_codex_request(request_key.clone(), method, &params);
                let requested = params
                    .get("permissions")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.pending_approvals.insert(
                    request_key,
                    PendingApprovalRequest {
                        jsonrpc_id: id_value,
                        response_kind: PendingApprovalResponseKind::Permissions(requested),
                    },
                );
                self.events
                    .send_blocking(ChatBackendEvent::PendingApproval(pending))
                    .ok();
            }
            _ => {
                self.write_json(&json!({
                    "jsonrpc": "2.0",
                    "id": id_value,
                    "result": {}
                }))?;
            }
        }
        Ok(())
    }

    fn write_json(&self, value: &Value) -> anyhow::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("stdin lock poisoned"))?;
        writeln!(stdin, "{}", serde_json::to_string(value)?)?;
        stdin.flush()?;
        Ok(())
    }
}

fn codex_conversation_usage(params: &Value, model: Option<&str>) -> Option<ConversationUsage> {
    let session_id = params.get("threadId").and_then(Value::as_str)?;
    let token_usage = params.get("tokenUsage")?;
    let totals = codex_usage_totals(token_usage.get("total")?);
    if totals.total_tokens() == 0 {
        return None;
    }
    let latest_turn = token_usage
        .get("last")
        .map(codex_usage_totals)
        .filter(|usage| usage.total_tokens() > 0);
    let models = model
        .map(|model_id| {
            vec![ModelUsage {
                provider_id: "Codex".to_string(),
                model_id: model_id.to_string(),
                totals: totals.clone(),
            }]
        })
        .unwrap_or_default();

    Some(ConversationUsage {
        session_id: session_id.to_string(),
        totals,
        latest_turn,
        models,
    })
}

fn codex_usage_totals(value: &Value) -> UsageTotals {
    UsageTotals {
        reported_total_tokens: value
            .get("totalTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        input_tokens: value
            .get("inputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: value
            .get("outputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        reasoning_tokens: value
            .get("reasoningOutputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: value
            .get("cachedInputTokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        ..Default::default()
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    #[test]
    fn parses_codex_thread_token_usage_notification() {
        let usage = codex_conversation_usage(
            &json!({
                "threadId": "codex-thread",
                "turnId": "turn-2",
                "tokenUsage": {
                    "total": {
                        "totalTokens": 4_200,
                        "inputTokens": 3_200,
                        "cachedInputTokens": 1_100,
                        "outputTokens": 1_000,
                        "reasoningOutputTokens": 600
                    },
                    "last": {
                        "totalTokens": 1_500,
                        "inputTokens": 1_100,
                        "cachedInputTokens": 400,
                        "outputTokens": 400,
                        "reasoningOutputTokens": 250
                    }
                }
            }),
            Some("gpt-5.6"),
        )
        .expect("usage");

        assert_eq!(usage.session_id, "codex-thread");
        assert_eq!(usage.totals.total_tokens(), 4_200);
        assert_eq!(usage.totals.input_tokens, 3_200);
        assert_eq!(usage.totals.cache_read_tokens, 1_100);
        assert_eq!(usage.totals.reasoning_tokens, 600);
        assert_eq!(usage.latest_turn.as_ref().unwrap().total_tokens(), 1_500);
        assert_eq!(usage.models[0].model_id, "gpt-5.6");
        assert_eq!(usage.totals.cost_usd, 0.0);
    }

    #[test]
    fn compare_review_workspace_policy_never_becomes_danger_full_access() {
        let policy = codex_turn_workspace_sandbox_policy(None);

        assert_eq!(
            policy.get("type").and_then(Value::as_str),
            Some("workspaceWrite")
        );
    }
}

fn codex_turn_sandbox_policy(
    access_mode: AgentAccessMode,
    visualization_dir: Option<&Path>,
) -> Value {
    if access_mode == AgentAccessMode::FullAccess {
        return json!({ "type": "dangerFullAccess" });
    }
    let writable_roots = visualization_dir
        .map(|path| vec![path.to_path_buf()])
        .unwrap_or_default();
    json!({
        "type": "workspaceWrite",
        "writableRoots": writable_roots
    })
}

fn codex_turn_workspace_sandbox_policy(visualization_dir: Option<&Path>) -> Value {
    let writable_roots = visualization_dir
        .map(|path| vec![path.to_path_buf()])
        .unwrap_or_default();
    json!({
        "type": "workspaceWrite",
        "writableRoots": writable_roots
    })
}

fn codex_developer_instructions(
    base: &str,
    visualization_dir: Option<&Path>,
    document_instructions: Option<&str>,
) -> String {
    let mut instructions = base.to_string();
    if let Some(document_instructions) = document_instructions
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        instructions.push_str("\n\n<document_assistant>\n");
        instructions.push_str(document_instructions);
        instructions.push_str("\n</document_assistant>");
    }
    let Some(visualization_dir) = visualization_dir else {
        return instructions;
    };
    format!(
        r#"{instructions}

<choro_visualizations>
Choro can render one interactive visualization at a time inside the conversation. When a visualization materially improves the answer:
- Write one self-contained HTML document or fragment under this exact directory: {visualization_dir}
- Keep it under 2 MB. Do not use fetch, XHR, WebSocket, or other live API calls.
- Do not write visualization HTML into the project or include it as a project change.
- In the final response, put this exact directive on its own line where the visualization belongs: ::codex-inline-vis{{file="<absolute-file-path>"}}
- Keep any necessary explanation outside the directive. Do not link to the HTML file.
</choro_visualizations>"#,
        visualization_dir = visualization_dir.display(),
    )
}

impl Drop for CodexRuntime {
    fn drop(&mut self) {
        terminate_child_process(&mut self.child);
    }
}

pub(super) fn capture_changed_files_snapshot(
    agent: &AgentRecord,
    mut summary: ChangedFilesSummary,
    source: &str,
) -> ChangedFilesSummary {
    let repo_path = agent.runtime_path().to_path_buf();
    summary.remove_visualization_artifacts(agent.id, &repo_path);
    if summary.snapshot_id.is_some() || summary.files.is_empty() {
        return summary;
    }

    let wanted_paths = summary
        .files
        .iter()
        .map(|file| normalize_repo_path(&repo_path, &file.path))
        .collect::<HashSet<_>>();
    if wanted_paths.is_empty() {
        return summary;
    }

    let diffs_result = if agent.repository_path.is_none() && !agent.is_active_solo() {
        ide_core::git::workspace_worktree_diffs(&agent.project_path)
    } else {
        ide_core::git::worktree_diffs(&repo_path)
    };
    let diffs = match diffs_result {
        Ok(diffs) => diffs
            .into_iter()
            .filter(|diff| wanted_paths.contains(&normalize_repo_path(&repo_path, &diff.path)))
            .collect::<Vec<_>>(),
        Err(error) => {
            eprintln!("failed to capture changed-files diff snapshot: {error:#}");
            Vec::new()
        }
    };
    if diffs.is_empty() {
        return summary;
    }

    match ide_core::local_store::LocalStore::open_default().and_then(|store| {
        store.create_agent_diff_snapshot(
            agent.id,
            agent.project_id,
            repo_path.clone(),
            source,
            git_head_sha(&repo_path),
            None,
            None,
            diffs,
        )
    }) {
        Ok(snapshot) => {
            summary.snapshot_id = Some(snapshot.id);
        }
        Err(error) => {
            eprintln!("failed to save changed-files diff snapshot: {error:#}");
        }
    }

    summary
}

fn normalize_repo_path(repo_path: &Path, path: &Path) -> PathBuf {
    let relative = path.strip_prefix(repo_path).unwrap_or(path);
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn git_head_sha(repo_path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}
