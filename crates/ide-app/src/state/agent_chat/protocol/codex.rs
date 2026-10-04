use super::*;

#[derive(Default)]
pub(super) struct CodexTurnControl {
    starting: bool,
    active_id: Option<String>,
    cancel_requested: bool,
    interrupt_request_id: Option<String>,
}

impl CodexTurnControl {
    fn begin(&mut self) {
        *self = Self {
            starting: true,
            ..Self::default()
        };
    }

    fn started(&mut self, turn_id: &str) {
        self.starting = false;
        self.active_id = Some(turn_id.to_owned());
    }

    fn completed(&mut self) {
        self.starting = false;
        self.active_id = None;
    }

    fn interrupt_request(&mut self, thread_id: &str) -> Option<Value> {
        if !self.cancel_requested || self.interrupt_request_id.is_some() {
            return None;
        }
        let turn_id = self.active_id.as_ref()?;
        let id = next_request_id();
        self.interrupt_request_id = Some(id.clone());
        Some(json!({
            "jsonrpc": "2.0", "id": id, "method": "turn/interrupt",
            "params": { "threadId": thread_id, "turnId": turn_id }
        }))
    }
}

impl CodexRuntime {
    pub(super) fn run_loop(&mut self) -> anyhow::Result<()> {
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                self.assistant_stream.flush(&self.events);
                break;
            }
            if let Some(command) = self.deferred_turns.pop_front() {
                self.handle_command(command)?;
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
        read_only: bool,
        turn_id: String,
    ) -> anyhow::Result<()> {
        let text = ide_core::studio::attach_request_context(&self.agent, text)?;
        let mode = super::managed::interaction_mode(&self.agent, mode);
        let consultation = super::managed::consultation(&self.agent);
        let read_only = read_only
            || self.agent.review_run_id.is_some()
            || consultation
            || (self.agent.delegation.is_some() && mode == AgentInteractionMode::Plan);
        let Some(thread_id) = self.thread_id.clone() else {
            return Err(anyhow!("Codex thread is not started"));
        };
        self.finish_reconnect(
            WorkLogStatus::Completed,
            "Reconnect superseded by a new turn",
            None,
        );
        self.studio_review
            .begin(self.agent.studio_context.is_some());
        self.assistant_stream.reset(&self.events);
        self.assistant_buffer.clear();
        self.plan_buffer.clear();
        self.pending_changed_files = None;

        self.pending_file_previews.clear();
        self.pending_observed_files.clear();
        self.active_turn_id = turn_id;
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
        let developer_instructions = if self.agent.review_run_id.is_some() {
            ide_core::code_review::REVIEW_INSTRUCTIONS.to_string()
        } else { codex_developer_instructions(
            mode_instructions,
            self.visualization_dir.as_deref(),
            self.agent
                .hidden_doc_assistant
                .then_some(self.agent.doc.as_str()),
        ) };
        let developer_instructions =
            super::managed::instructions(developer_instructions, &self.agent)?;
        let mut sandbox_policy = if read_only || self.agent.studio_context.is_some() {
            json!({ "type": "readOnly" })
        } else {
            codex_turn_sandbox_policy(self.access_mode, self.visualization_dir.as_deref())
        };
        // PocketComet's per-turn history/memory CLI uses a loopback HTTP bridge.
        // Match its other teammates' network-enabled sandbox without changing
        // filesystem confinement or the user's approval policy. The bridge
        // authorizes each call against this run's conversation and memory scope.
        allow_pocketcomet_chat_network(&mut sandbox_policy, self.agent.origin.as_ref());
        let approval_policy = if self.agent.studio_context.is_some()
            || self.agent.review_run_id.is_some()
            || consultation
        {
            "never"
        } else {
            self.access_mode.codex_approval_policy()
        };
        self.turn_control.begin();
        let mut params = json!({
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
            });
        if self.agent.review_run_id.is_some() { params["environments"] = json!([]); }
        let result = self.request("turn/start", params)?;
        // Normally turn/started supplies the id while the request is pending.
        // Also accept the response, without resurrecting an already completed turn.
        if self.turn_control.starting {
            if let Some(id) = result.pointer("/turn/id").and_then(Value::as_str) {
                self.turn_control.started(id);
                self.send_pending_interrupt()?;
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, command: ChatBackendCommand) -> anyhow::Result<()> {
        match command {
            ChatBackendCommand::Shutdown => return Ok(()),
            ChatBackendCommand::ForceShutdown => {
                return Err(anyhow!("Codex app-server force-stopped"));
            }
            ChatBackendCommand::CancelTurn => self.cancel_turn()?,
            ChatBackendCommand::SendTurn {
                text,
                mode,
                read_only,
                turn_id,
            } => self.send_turn(text, mode, read_only, turn_id)?,
            ChatBackendCommand::SendStudioTurn { text, mode, turn_id, title, request } => {
                request.activate(self.agent.id, &title)?;
                self.send_turn(text, mode, false, turn_id)?;
            }
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
                Ok(ChatBackendCommand::UpdateModelEffort { model, effort, .. }) => {
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
                Ok(command @ (ChatBackendCommand::SendTurn { .. } | ChatBackendCommand::SendStudioTurn { .. })) => {
                    self.deferred_turns.push_back(command);
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
        self.studio_review.cancel();
        self.deferred_turns.clear();
        self.turn_control.cancel_requested = true;
        ide_core::studio::revoke_agent_scope(&self.agent);
        self.assistant_stream.flush(&self.events);
        self.finish_reconnect(WorkLogStatus::Completed, "Reconnect stopped", None);
        self.deny_all_pending_approvals()?;
        self.send_pending_interrupt()?;
        let status = if self.turn_control.starting || self.turn_control.active_id.is_some() {
            AgentChatStatus::Cancelling
        } else {
            AgentChatStatus::Idle
        };
        self.events
            .send_blocking(ChatBackendEvent::Status(status))
            .ok();
        Ok(())
    }

    fn send_pending_interrupt(&mut self) -> anyhow::Result<()> {
        if let Some(request) = self
            .thread_id
            .as_deref()
            .and_then(|id| self.turn_control.interrupt_request(id))
        {
            // Do not nest a blocking request inside turn/start: its response
            // could otherwise be consumed by the interrupt's response loop.
            self.write_json(&request)?;
        }
        Ok(())
    }

    fn deny_all_pending_approvals(&mut self) -> anyhow::Result<()> {
        let request_ids = self.pending_approvals.keys().cloned().collect::<Vec<_>>();
        for request_id in request_ids {
            self.resolve_approval(request_id, false)?;
        }
        Ok(())
    }

    fn handle_message(&mut self, message: impl Into<ProviderMessage>) -> anyhow::Result<()> {
        let ProviderMessage { value: message, _reservation } = message.into();
        if message.get("_choro_evidence_incomplete").and_then(Value::as_bool) == Some(true) { self.events.mark_evidence_overflow(); }
        if message.get("method").is_none()
            && message
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| self.turn_control.interrupt_request_id.as_deref() == Some(id))
        {
            if let Some(error) = message.get("error") {
                if self.turn_control.active_id.is_some() {
                    return Err(anyhow!("turn/interrupt returned error: {error}"));
                }
            }
            return Ok(());
        }
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
        // An app-server connection also receives notifications from delegated
        // threads. Their deltas, usage and completion must never mutate the
        // parent conversation. Keep server requests above this guard so child
        // tool/approval requests can still receive their required response.
        if !codex_notification_for_thread(&params, self.thread_id.as_deref()) {
            return Ok(());
        }
        if codex_event_confirms_recovery(method) {
            self.finish_reconnect(WorkLogStatus::Completed, "Reconnected to Codex", None);
        }
        if method != "item/agentMessage/delta" {
            self.assistant_stream.flush(&self.events);
        }
        if let Some(active) = codex_compaction_activity(method, &params) {
            self.events
                .send_blocking(ChatBackendEvent::Compaction(active))
                .ok();
            return Ok(());
        }
        match method {
            "turn/started" => {
                if let Some(id) = params.pointer("/turn/id").and_then(Value::as_str) {
                    self.turn_control.started(id);
                    self.send_pending_interrupt()?;
                }
            }
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
                if item_type_from_params(&params) == Some("fileChange") {
                    let action_id = item_id_from_params(&params)
                        .unwrap_or_else(|| format!("file-change-{}", self.active_turn_id));
                    let preview = self
                        .pending_file_previews
                        .remove(&action_id)
                        .unwrap_or_default();
                    if params.pointer("/item/status").and_then(Value::as_str) == Some("completed") {
                        let files = completed_file_change_stats(&params);
                        self.record_exact_file_changes(
                            action_id,
                            if files.is_empty() { preview } else { files },
                        );
                    }
                }
                if let Some(plan) = plan_text_from_completed_item(&params)
                    .or_else(|| {
                        (!self.plan_buffer.trim().is_empty()).then(|| self.plan_buffer.clone())
                    })
                    .filter(|_| !super::managed::is_child(&self.agent))
                {
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
                // A preview may still fail or be declined. Attribute it only
                // when completion confirms success; show real writes as Git
                // observations in the meantime.
                let bytes = files.iter().map(change_tracking::file_payload_bytes).sum();
                if !self.pending_file_previews.insert(action_id, files, bytes) {
                    self.events.mark_evidence_overflow();
                }
            }
            "turn/diff/updated" => {
                // Workspace observations come from the shared observer. A turn
                // diff cannot establish authorship and needs no provider work.
            }
            "thread/tokenUsage/updated" => {
                if let Some(usage) = codex_conversation_usage(&params, self.model.as_deref()) {
                    self.events
                        .send_blocking(ChatBackendEvent::Usage(usage))
                        .ok();
                }
            }
            "turn/completed" => {
                self.turn_control.completed();
                let was_cancelled = self.studio_review.cancelled;
                let interrupted = was_cancelled
                    || params
                        .pointer("/turn/status")
                        .and_then(Value::as_str)
                        .is_some_and(|status| {
                            matches!(status, "interrupted" | "failed" | "cancelled")
                        });
                if interrupted {
                    self.studio_review.cancel();
                }
                if self.studio_review.complete() {
                    if let Err(error) = ide_core::studio::verify_agent_completion(&self.agent) {
                        ide_core::studio::revoke_agent_scope(&self.agent);
                        self.pending_approvals.clear();
                        self.events
                            .send_blocking(ChatBackendEvent::Error(format!("{error:#}")))
                            .ok();
                        self.events
                            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Failed))
                            .ok();
                        return Ok(());
                    }
                }
                ide_core::studio::revoke_agent_scope(&self.agent);
                self.pending_approvals.clear();
                self.emit_pending_changed_files();
                if interrupted {
                    let failed = !was_cancelled
                        && params.pointer("/turn/status").and_then(Value::as_str) == Some("failed");
                    self.events
                        .send_blocking(ChatBackendEvent::Status(if failed {
                            AgentChatStatus::Failed
                        } else {
                            AgentChatStatus::Idle
                        }))
                        .ok();
                    return Ok(());
                }
                let review = extract_code_review(&self.assistant_buffer);
                let verification = extract_verification(&self.assistant_buffer);
                let checklist = extract_review_checklist(&self.assistant_buffer);
                if review.is_some() || verification.is_some() || checklist.is_some() {
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
                    if let Some(checklist) = checklist {
                        self.events
                            .send_blocking(ChatBackendEvent::ReviewChecklist(
                                ReviewChecklist::ready("", &checklist, unix_now()),
                            ))
                            .ok();
                    }
                    self.events
                        .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle))
                        .ok();
                } else if let Some(plan) = extract_proposed_plan(&self.assistant_buffer)
                    .filter(|_| !super::managed::is_child(&self.agent))
                {
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
                        self.emit_pending_changed_files();
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
        self.events
            .send_blocking(ChatBackendEvent::ChangedFiles(summary))
            .ok();
    }

    fn record_exact_file_changes(&mut self, action_id: String, mut files: Vec<FileChangeStat>) {
        if files.is_empty() {
            return;
        }
        for file in &mut files {
            file.path = normalize_repo_path(self.agent.runtime_path(), &file.path);
        }
        for file in &files {
            let activity_id = format!("codex:{action_id}:{}", file.path.to_string_lossy());
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
    }

    fn handle_server_request(&mut self, message: Value) -> anyhow::Result<()> {
        let id_value = message.get("id").cloned().unwrap_or(Value::Null);
        let request_key = jsonrpc_id_key(&id_value);
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        if self.agent.review_run_id.is_some() {
            let result = match method {
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => json!({"decision":"decline"}),
                "item/permissions/requestApproval" => json!({"permissions":{},"scope":"turn"}),
                "item/tool/requestUserInput" => json!({"answers":{}}),
                _ => return self.write_json(&json!({"jsonrpc":"2.0","id":id_value,"error":{"code":-32601,"message":"Internal reviewer has no native tool authority"}})),
            };
            return self.write_json(&json!({"jsonrpc":"2.0","id":id_value,"result":result}));
        }
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

#[cfg(test)]
mod interruption_tests {
    use super::*;

    #[cfg(unix)]
    fn echo_runtime() -> (CodexRuntime, async_channel::Receiver<ChatBackendEvent>) {
        // A local pipe echoes client requests; no provider, credentials, or
        // conversation store is involved in this protocol regression test.
        let mut child = Command::new("/bin/cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .process_group(0)
            .spawn()
            .unwrap();
        let stdin = Arc::new(Mutex::new(child.stdin.take().unwrap()));
        let (message_tx, messages) = crossbeam_channel::unbounded();
        spawn_json_reader(child.stdout.take().unwrap(), message_tx);
        let (_, commands) = crossbeam_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        let runtime = CodexRuntime {
            child,
            stdin,
            messages,
            commands,
            events: events.into(),
            shutdown: Arc::new(AtomicBool::new(false)),
            agent: AgentRecord::new(
                ide_core::ProjectId(uuid::Uuid::new_v4()),
                PathBuf::from("/tmp"),
                "Stop test",
                "",
                AgentKind::Codex,
                AgentModel::default_for(AgentKind::Codex),
                AgentEffort::default(),
                AgentAccessMode::FullAccess,
            ),
            thread_id: Some("test-thread".into()),
            turn_control: CodexTurnControl::default(),
            assistant_buffer: String::new(),
            assistant_stream: StreamChunkBuffer::new(),
            studio_review: StudioReviewGate::default(),
            plan_buffer: String::new(),
            pending_changed_files: None,
            pending_file_previews: Default::default(),
            pending_observed_files: Vec::new(),
            active_turn_id: "local-receipt-id".into(),
            command_ran_this_turn: false,
            active_command_item_id: None,
            pending_user_inputs: Default::default(),
            pending_approvals: Default::default(),
            deferred_turns: VecDeque::new(),
            active_reconnect_work_log_id: None,
            model: None,
            effort: "medium".into(),
            access_mode: AgentAccessMode::FullAccess,
            visualization_dir: None,
        };
        (runtime, event_rx)
    }

    #[test]
    #[cfg(unix)]
    fn review_denies_native_approval_requests_without_asking_the_user() {
        let (mut runtime, events) = echo_runtime();
        runtime.agent.review_run_id = Some(uuid::Uuid::new_v4());
        for method in ["item/commandExecution/requestApproval", "item/fileChange/requestApproval", "item/permissions/requestApproval", "item/tool/requestUserInput", "item/tool/call"] {
            runtime.handle_server_request(json!({"jsonrpc":"2.0","id":17,"method":method,"params":{}})).unwrap();
            let reply = runtime.messages.recv_timeout(Duration::from_secs(1)).unwrap();
            assert!(reply.get("error").is_some() || reply["result"]["decision"] == "decline"
                || reply["result"]["permissions"] == json!({}) || reply["result"]["answers"] == json!({}));
        }
        assert!(runtime.pending_approvals.is_empty() && runtime.pending_user_inputs.is_empty());
        assert!(events.try_recv().is_err());
    }

    #[test]
    #[cfg(unix)]
    fn studio_startup_defers_frozen_target_without_replacing_scope() {
        let (mut runtime, _events) = echo_runtime();
        let (store, design, agent, request) = crate::state::agent_chat::studio_request::tests::fixture();
        let active = ide_core::studio::scope_for_request(&design, None, None);
        store.save_scope(agent, &active).unwrap();
        let (commands, receiver) = crossbeam_channel::unbounded();
        runtime.commands = receiver;
        commands.send(ChatBackendCommand::SendStudioTurn {
            text: "Transport prompt".into(), mode: AgentInteractionMode::Default,
            turn_id: "queued-studio".into(), title: "Visible user message".into(), request: request.clone(),
        }).unwrap();
        runtime.handle_commands_while_blocked().unwrap();
        assert_eq!(store.scope(agent).unwrap().id, active.id);
        match runtime.deferred_turns.pop_front().unwrap() {
            ChatBackendCommand::SendStudioTurn { request: frozen, title, turn_id, .. } => {
                assert_eq!(frozen, request);
                assert_eq!(title, "Visible user message");
                assert_eq!(turn_id, "queued-studio");
            }
            _ => panic!("Studio startup lost the frozen target"),
        }
        let _ = runtime.child.kill();
        let _ = runtime.child.wait();
    }

    #[test]
    #[cfg(unix)]
    fn runtime_interrupts_the_provider_turn_and_waits_for_completion() {
        let (mut runtime, events) = echo_runtime();
        runtime.turn_control.begin();
        runtime.cancel_turn().unwrap();
        assert!(runtime.messages.try_recv().is_err());
        assert!(matches!(
            events.try_recv().unwrap(),
            ChatBackendEvent::Status(AgentChatStatus::Cancelling)
        ));

        runtime
            .handle_message(json!({
                "method": "turn/started", "params": {
                    "threadId": "test-thread", "turn": { "id": "provider-turn" }
                }
            }))
            .unwrap();
        let request = runtime
            .messages
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        assert_eq!(request["method"], "turn/interrupt");
        assert_eq!(request["params"]["turnId"], "provider-turn");
        runtime
            .handle_message(json!({"id": request["id"], "result": {}}))
            .unwrap();
        assert!(
            events.try_recv().is_err(),
            "An interrupt ack is not completion"
        );
        runtime
            .handle_message(json!({
                "method": "turn/completed", "params": {
                    "threadId": "test-thread",
                    "turn": { "id": "provider-turn", "status": "interrupted" }
                }
            }))
            .unwrap();
        assert!(matches!(
            events.try_recv().unwrap(),
            ChatBackendEvent::ChangedFiles(_)
        ));
        assert!(matches!(
            events.try_recv().unwrap(),
            ChatBackendEvent::Status(AgentChatStatus::Idle)
        ));
    }

    #[test]
    #[cfg(unix)]
    fn blocked_tracker_cannot_delay_prompt_or_interrupt_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().join("state")).unwrap();
        let tracker = ide_core::agent_changes::ChangeTracker::with_store(store);
        let (release, blocked) = std::sync::mpsc::channel();
        assert!(tracker.enqueue(0, move |_| {
            let _ = blocked.recv();
        }));
        let (mut runtime, events) = echo_runtime();
        let (_command_tx, command_rx) = crossbeam_channel::unbounded();
        runtime.commands = command_rx;
        runtime.agent.project_path = dir.path().to_path_buf();
        fs::File::create(dir.path().join("asset.zip"))
            .unwrap()
            .set_len(3_000_000_000)
            .unwrap();
        let (output, received) = async_channel::unbounded();
        super::super::change_tracking::route_with_tracker(
            runtime.agent.clone(),
            events,
            output.into(),
            tracker,
            Arc::new(Default::default()),
        );
        let (reply_tx, reply_rx) = crossbeam_channel::unbounded();
        let requests = std::mem::replace(&mut runtime.messages, reply_rx);
        let (sent_tx, sent_rx) = crossbeam_channel::unbounded();
        thread::spawn(move || {
            while let Ok(request) = requests.recv() {
                let method = request["method"].as_str().unwrap_or_default().to_string();
                let result = if method == "turn/start" {
                    json!({"turn":{"id":"provider-turn"}})
                } else {
                    json!({})
                };
                let _ = sent_tx.send(method);
                if reply_tx
                    .send(json!({"id":request["id"],"result":result}).into())
                    .is_err()
                {
                    break;
                }
            }
        });
        let started = Instant::now();
        runtime
            .send_turn("hello".into(), AgentInteractionMode::Default, false, "test-turn".into())
            .unwrap();
        assert_eq!(
            sent_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "turn/start"
        );
        let prompt_delay = started.elapsed();
        runtime.record_exact_file_changes(
            "edit".into(),
            vec![FileChangeStat::new("shared.rs", 1, 1)
                .with_content_projection(Some("before\n".into()), Some("after\n".into()))],
        );
        let started = Instant::now();
        runtime.cancel_turn().unwrap();
        assert_eq!(
            sent_rx.recv_timeout(Duration::from_secs(2)).unwrap(),
            "turn/interrupt"
        );
        let interrupt_delay = started.elapsed();
        assert!(prompt_delay < Duration::from_secs(2));
        assert!(interrupt_delay < Duration::from_secs(2));
        while let Ok(event) = received.try_recv() {
            assert!(!matches!(
                event,
                ChatBackendEvent::ChangeReceiptReady { .. }
            ));
        }
        eprintln!("blocked tracking: prompt dispatch {prompt_delay:?}, interrupt dispatch {interrupt_delay:?}");
        runtime.emit_pending_changed_files();
        release.send(()).unwrap();
        loop {
            if let ChatBackendEvent::ChangeReceiptReady { state, .. } =
                unpack_event(received.recv_blocking().unwrap())
            {
                assert_eq!(state, ide_core::agent_changes::ChangeReceiptState::Ready);
                break;
            }
        }
    }

    #[test]
    fn stop_during_start_waits_for_the_provider_turn_id() {
        let mut control = CodexTurnControl::default();
        control.begin();
        control.cancel_requested = true;
        assert!(control.interrupt_request("thread-1").is_none());

        control.started("provider-turn-1");
        let request = control.interrupt_request("thread-1").unwrap();
        assert_eq!(request["method"], "turn/interrupt");
        assert!(request["id"].is_string());
        assert_eq!(
            request["params"],
            json!({
                "threadId": "thread-1", "turnId": "provider-turn-1"
            })
        );
        assert!(control.interrupt_request("thread-1").is_none());
    }

    #[test]
    fn stop_completion_allows_an_independent_next_turn() {
        let mut control = CodexTurnControl::default();
        control.begin();
        control.started("first");
        control.cancel_requested = true;
        let first = control.interrupt_request("thread-1").unwrap();
        control.completed();
        assert!(!control.starting);
        assert!(control.active_id.is_none());

        control.begin();
        control.started("second");
        assert!(control.interrupt_request("thread-1").is_none());
        control.cancel_requested = true;
        let second = control.interrupt_request("thread-1").unwrap();
        assert_eq!(second["params"]["turnId"], "second");
        assert_ne!(first["id"], second["id"]);
    }

    #[test]
    fn completed_turn_is_not_resurrected_by_late_start_response() {
        let mut control = CodexTurnControl::default();
        control.begin();
        control.started("finished");
        control.completed();
        assert!(!control.starting);
        control.cancel_requested = true;
        assert!(control.interrupt_request("thread-1").is_none());
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
    fn pocketcomet_chat_network_keeps_filesystem_confinement() {
        let origin: ide_core::agents::AgentOrigin = serde_json::from_value(json!({
            "kind": "pocket_comet_chat", "workspace_id": "workspace", "workspace_name": "Workspace",
            "project_id": "project", "project_name": "Project", "teammate_id": "choro", "teammate_name": "Choro",
            "conversation_id": "conversation", "conversation_name": "Chat", "thread_id": "thread", "thread_title": "Question"
        })).unwrap();
        for mut policy in [
            codex_turn_sandbox_policy(
                AgentAccessMode::AskForApproval,
                Some(Path::new("/tmp/visualizations")),
            ),
            json!({ "type": "readOnly" }),
        ] {
            let original = policy.clone();
            allow_pocketcomet_chat_network(&mut policy, Some(&origin));
            assert_eq!(policy["networkAccess"], true);
            policy.as_object_mut().unwrap().remove("networkAccess");
            assert_eq!(policy, original);
        }
        let mut ordinary = codex_turn_sandbox_policy(AgentAccessMode::AskForApproval, None);
        let original = ordinary.clone();
        allow_pocketcomet_chat_network(&mut ordinary, None);
        assert_eq!(ordinary, original);
    }

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

    #[test]
    fn developer_instructions_scope_choro_native_creation_tools() {
        let instructions = codex_developer_instructions("base", None, None);

        assert!(instructions.contains("create_choro_doc"));
        assert!(instructions.contains("create_choro_script"));
        assert!(instructions.contains("Do not use these tools for ordinary repository"));
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

fn allow_pocketcomet_chat_network(
    policy: &mut Value,
    origin: Option<&ide_core::agents::AgentOrigin>,
) {
    if origin.is_some_and(ide_core::agents::AgentOrigin::is_pocketcomet_chat)
        && matches!(policy["type"].as_str(), Some("workspaceWrite" | "readOnly"))
    {
        policy["networkAccess"] = json!(true);
    }
}

fn codex_developer_instructions(
    base: &str,
    visualization_dir: Option<&Path>,
    document_instructions: Option<&str>,
) -> String {
    let mut instructions = base.to_string();
    instructions.push_str("\n\n<choro_native_tools>\n");
    instructions.push_str(super::CHORO_NATIVE_TOOL_INSTRUCTIONS);
    instructions.push_str("\n</choro_native_tools>");
    if let Some(document_instructions) = document_instructions
        .map(str::trim)
        .filter(|instructions| !instructions.is_empty())
    {
        instructions.push_str("\n\n<document_assistant>\n");
        instructions.push_str(document_instructions);
        instructions.push_str("\n</document_assistant>");
    }
    super::append_choro_visualization_instructions(instructions, visualization_dir)
}

impl Drop for CodexRuntime {
    fn drop(&mut self) {
        ide_core::studio::revoke_agent_scope(&self.agent);
        terminate_child_process(&mut self.child);
    }
}

pub(super) fn capture_changed_files_snapshot(
    agent: &AgentRecord,
    mut summary: ChangedFilesSummary,
    source: &str,
    store: &LocalStore,
) -> ChangedFilesSummary {
    let repo_path = agent.runtime_path().to_path_buf();
    summary.reconcile_final_files(&repo_path);
    summary.remove_visualization_artifacts(agent.id, &repo_path);
    summary.remove_provider_private_artifacts();
    if summary.snapshot_id.is_some() || summary.is_empty() {
        return summary;
    }

    let owned_diffs = summary
        .conversation_files()
        .filter_map(|file| attributed_file_diff(file, &repo_path))
        .collect::<Vec<_>>();
    let diffs = owned_diffs;
    if diffs.is_empty() {
        return summary;
    }

    match store.create_agent_diff_snapshot(
        agent.id,
        agent.project_id,
        repo_path.clone(),
        source,
        None,
        None,
        None,
        diffs,
    ) {
        Ok(snapshot) => {
            summary.snapshot_id = Some(snapshot.id);
        }
        Err(error) => {
            eprintln!("failed to save changed-files diff snapshot: {error:#}");
        }
    }

    summary
}

fn attributed_file_diff(
    file: &FileChangeStat,
    repo_path: &Path,
) -> Option<ide_core::git::FileDiff> {
    if !file.prior_segments.is_empty() {
        let mut last = file.clone();
        last.prior_segments.clear();
        let mut result = ide_core::git::FileDiff { path: normalize_repo_path(repo_path, &file.path), ..Default::default() };
        for (index, segment) in file.prior_segments.iter().chain(std::iter::once(&last)).enumerate() {
            let diff = attributed_file_diff(segment, repo_path)?;
            result.is_binary |= diff.is_binary;
            result.hunks.push(ide_core::git::DiffHunk { header: format!("Edit segment {} (separate evidence chain)", index + 1), lines: vec![] });
            result.hunks.extend(diff.hunks);
        }
        return Some(result);
    }
    let mut diff = if let Some((before, after)) = file.baseline_content.as_deref().zip(file.result_content.as_deref()) {
        ide_core::git::diff::diff_from_contents(&file.path, before, after).ok()?
    } else if let Some(diff) = &file.attributed_diff {
        diff.clone()
    } else {
        let (before, after) = file
            .baseline_content
            .as_deref()
            .zip(file.result_content.as_deref())?;
        ide_core::git::diff::diff_from_contents(&file.path, before, after).unwrap_or_else(|error| {
            eprintln!("failed to render attributed file contents: {error:#}");
            ide_core::git::FileDiff::default()
        })
    };
    diff.path = normalize_repo_path(repo_path, &file.path);
    Some(diff)
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use ide_core::git::LineOrigin;

    #[test]
    fn captured_contents_do_not_attribute_preexisting_lines_to_this_turn() {
        let file = FileChangeStat::new("/project/shared.txt", 1, 0).with_content_projection(
            Some("other-agent-first\nother-agent-second\n".into()),
            Some("other-agent-first\nother-agent-second\nthis-agent-third\n".into()),
        );
        let diff = attributed_file_diff(&file, Path::new("/project")).unwrap();
        assert_eq!(diff.path, Path::new("shared.txt"));
        let additions = diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.origin == LineOrigin::Add)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(additions, vec!["this-agent-third"]);
    }

    #[test]
    fn multiple_owned_actions_survive_turn_receipt_but_not_ledger_patch_history() {
        let first = render_patch_change(&json!({"path": "/project/shared.txt",
            "kind": {"type": "update"}, "diff": "@@ -1 +1,2 @@\n other-agent\n+first-edit\n"}))
        .unwrap();
        let second = render_patch_change(&json!({"path": "/project/shared.txt",
            "kind": {"type": "update"}, "diff": "@@ -1,2 +1,3 @@\n other-agent\n first-edit\n+second-edit\n"})).unwrap();
        let activities = [
            FileChangeActivity::new("one", "turn", first, false, 1),
            FileChangeActivity::new("two", "turn", second, false, 2),
        ];
        let summary = ChangedFilesSummary::from_activities("turn", &activities);
        assert_eq!(summary.files.len(), 1);
        assert_eq!(summary.total_additions(), 2);
        let diff = attributed_file_diff(&summary.files[0], Path::new("/project")).unwrap();
        let additions = diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.origin == LineOrigin::Add)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(additions, vec!["first-edit", "second-edit"]);
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&summary);
        assert!(ledger.files[0].attributed_diff.is_none());
        assert!(summary.files[0].attributed_diff.is_some());
    }
}
