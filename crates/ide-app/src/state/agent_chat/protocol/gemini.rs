//! Gemini subscription access through Google's official Antigravity ACP server.
use super::*;

mod google_runtime;

const SIGN_IN_HELP: &str = "Start a Gemini chat in Choro and approve Sign in with Google using the account linked to your Google AI subscription.";

pub(crate) fn provider_available() -> bool {
    google_runtime::cached_executable().is_some()
}

pub(super) fn spawn_gemini_acp(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    previous_stop: Option<ChatBackendStopSignal>,
) -> anyhow::Result<()> {
    thread::Builder::new()
        .name("choro-gemini-acp".into())
        .spawn(move || {
            let _stopped = BackendStoppedOnDrop(stopped);
            if !wait_for_previous_backend(previous_stop, &shutdown) {
                return;
            }
            if let Err(error) = run(agent, initial_mode, commands, events.clone(), shutdown) {
                let _ = events.send_blocking(ChatBackendEvent::Error(format!(
                    "Google Gemini failed: {error:#}"
                )));
            }
        })?;
    Ok(())
}

struct GeminiPermission {
    jsonrpc_id: Value,
    allow_option: Option<String>,
    reject_option: Option<String>,
}

struct GeminiRuntime {
    child: Child,
    stdin: ChildStdin,
    messages: Receiver<ProviderMessage>,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    agent: AgentRecord,
    session_id: Option<String>,
    access_mode: AgentAccessMode,
    interaction_mode: AgentInteractionMode,
    read_only_turn: bool,
    active_turn_id: String,
    assistant_buffer: String,
    assistant_stream: StreamChunkBuffer,
    pending_permissions: std::collections::HashMap<String, GeminiPermission>,
    deferred_turns: VecDeque<(String, AgentInteractionMode, bool, String)>,
    loading_history: bool,
    tools: ide_core::agent_changes::PendingEvidence<Value>,
    session_modes: Value,
    deadline: Option<Instant>,
    deny_all_tools: bool,
}

fn start_runtime(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<GeminiRuntime> {
    let executable =
        google_runtime::executable().context("Could not prepare Google's Gemini provider")?;
    anyhow::ensure!(!shutdown.load(Ordering::SeqCst), "Gemini stopped");
    let mut command = google_runtime::command(&executable)?;
    command
        .current_dir(agent.runtime_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command.spawn().context("Could not start Google Gemini")?;
    let stdin = child.stdin.take().context("Gemini stdin unavailable")?;
    let stdout = child.stdout.take().context("Gemini stdout unavailable")?;
    let stderr = child.stderr.take().context("Gemini stderr unavailable")?;
    let (tx, messages) = crossbeam_channel::bounded(1);
    spawn_json_reader(stdout, tx);
    spawn_stderr_reader(stderr, events.clone(), "Google Gemini");
    Ok(GeminiRuntime {
        child,
        stdin,
        messages,
        commands,
        events,
        shutdown,
        session_id: None,
        access_mode: agent.access_mode,
        interaction_mode: initial_mode,
        read_only_turn: false,
        active_turn_id: next_request_id(),
        assistant_buffer: String::new(),
        assistant_stream: StreamChunkBuffer::new(),
        pending_permissions: Default::default(),
        deferred_turns: VecDeque::new(),
        loading_history: false,
        tools: Default::default(),
        agent,
        session_modes: Value::Null,
        deadline: None,
        deny_all_tools: false,
    })
}

fn run(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut runtime = start_runtime(agent, initial_mode, commands, events, shutdown)?;
    let resumed = runtime.agent.cli_session_id.is_some();
    runtime.initialize_session(true, choro_acp_mcp_servers_json(&runtime.agent))?;
    let session_id = runtime
        .session_id
        .clone()
        .context("Google did not return a session id")?;
    let _ = runtime
        .events
        .send_blocking(ChatBackendEvent::SessionReady { session_id });
    if resumed {
        let _ = runtime
            .events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle));
    } else {
        runtime.send_turn(
            runtime.agent.doc.clone(),
            initial_mode,
            false,
            runtime.events.initial_turn_id.clone(),
        )?;
    }
    loop {
        if runtime.shutdown.load(Ordering::SeqCst) {
            return Ok(());
        }
        if let Some((text, mode, read_only, turn_id)) = runtime.deferred_turns.pop_front() {
            runtime.send_turn(text, mode, read_only, turn_id)?;
            continue;
        }
        match next_backend_inbound(
            &runtime.commands,
            &runtime.messages,
            runtime.assistant_stream.has_pending(),
        ) {
            BackendInbound::Command(ChatBackendCommand::SendTurn {
                text,
                mode,
                read_only,
                turn_id,
            }) => runtime.send_turn(text, mode, read_only, turn_id)?,
            BackendInbound::Command(command) => runtime.handle_command(command)?,
            BackendInbound::Message(message) => runtime.handle_message(message)?,
            BackendInbound::FlushTick => runtime.assistant_stream.flush_due(&runtime.events),
            BackendInbound::CommandsClosed => return Ok(()),
            BackendInbound::MessagesClosed => return Err(anyhow!("Google Gemini disconnected")),
        }
    }
}

impl GeminiRuntime {
    fn initialize_session(
        &mut self,
        allow_sign_in: bool,
        mcp_servers: Value,
    ) -> anyhow::Result<()> {
        self.request("initialize", json!({"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"Choro","version":env!("CARGO_PKG_VERSION")}}))?;
        let existing = self.agent.cli_session_id.clone();
        let method = if existing.is_some() {
            "session/load"
        } else {
            "session/new"
        };
        let mut params = json!({"cwd":self.agent.runtime_path(),"mcpServers":mcp_servers});
        if let Some(id) = &existing {
            params["sessionId"] = json!(id);
            self.loading_history = true;
        }
        let session = match self.request(method, params.clone()) {
            Ok(session) => session,
            Err(error) if is_authentication_error(&format!("{error:#}")) => {
                anyhow::ensure!(allow_sign_in, "Gemini needs a Google login. {SIGN_IN_HELP}");
                self.approve_google_sign_in()?;
                self.request("authenticate", json!({"methodId":"oauth-personal"}))?;
                self.request(method, params)?
            }
            Err(error) => return Err(error),
        };
        self.loading_history = false;
        self.session_modes = session.get("modes").cloned().unwrap_or(Value::Null);
        self.session_id = existing.or_else(|| {
            session
                .get("sessionId")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
        anyhow::ensure!(
            self.session_id.is_some(),
            "Google did not return a session id"
        );
        Ok(())
    }

    fn approve_google_sign_in(&mut self) -> anyhow::Result<()> {
        const ID: &str = "gemini-google-sign-in";
        let _ = self.events.send_blocking(ChatBackendEvent::PendingApproval(PendingApproval::new(
            ID, PendingApprovalKind::Permissions, "Sign in with Google",
            Some("Google will open a browser to sign in. Use the account linked to your Google AI subscription. Choro uses Google's official Antigravity agent and does not need an API key.".into()),
        )));
        let _ = self
            .events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::WaitingForUser));
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return Err(anyhow!("Google sign-in cancelled"));
            }
            match self.commands.recv_timeout(Duration::from_millis(40)) {
                Ok(ChatBackendCommand::ResolveApproval {
                    request_id,
                    approved,
                }) if request_id == ID => {
                    anyhow::ensure!(approved, "Google sign-in cancelled");
                    let _ = self
                        .events
                        .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running));
                    return Ok(());
                }
                Ok(
                    ChatBackendCommand::CancelTurn
                    | ChatBackendCommand::Shutdown
                    | ChatBackendCommand::ForceShutdown,
                ) => return Err(anyhow!("Google sign-in cancelled")),
                Ok(command) => self.handle_command(command)?,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(anyhow!("Google sign-in cancelled")),
            }
        }
    }

    fn write_json(&mut self, value: &Value) -> anyhow::Result<()> {
        writeln!(self.stdin, "{}", serde_json::to_string(value)?)?;
        self.stdin.flush()?;
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = next_request_id();
        self.write_json(&json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}))?;
        let deadline =
            (method != "session/prompt").then(|| Instant::now() + Duration::from_secs(180));
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return Err(anyhow!("Gemini stopped"));
            }
            if deadline
                .into_iter()
                .chain(self.deadline)
                .min()
                .is_some_and(|deadline| Instant::now() >= deadline)
            {
                return Err(anyhow!("Gemini {method} timed out. {SIGN_IN_HELP}"));
            }
            while let Ok(command) = self.commands.try_recv() {
                self.handle_command(command)?;
            }
            match self.messages.recv_timeout(Duration::from_millis(40)) {
                Ok(message) => {
                    if message.get("id").and_then(Value::as_str) == Some(&id)
                        && message.get("method").is_none()
                    {
                        if let Some(error) = message.get("error") {
                            return Err(anyhow!("Gemini {method}: {error}"));
                        }
                        return Ok(message.get("result").cloned().unwrap_or(Value::Null));
                    }
                    self.handle_message(message)?;
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    self.assistant_stream.flush_due(&self.events)
                }
                Err(_) => return Err(anyhow!("Google Gemini disconnected")),
            }
        }
    }

    fn send_turn(
        &mut self,
        text: String,
        mode: AgentInteractionMode,
        read_only: bool,
        turn_id: String,
    ) -> anyhow::Result<()> {
        let session = self
            .session_id
            .clone()
            .context("Gemini session unavailable")?;
        self.interaction_mode = mode;
        self.read_only_turn = read_only || mode == AgentInteractionMode::Plan;
        self.active_turn_id = turn_id;
        self.assistant_buffer.clear();
        self.assistant_stream.reset(&self.events);
        self.tools.clear();
        self.request(
            "session/set_model",
            json!({"sessionId":session, "modelId":self.agent.model.cli_value().unwrap_or("auto")}),
        )?;
        // Never let the CLI's auto-approval modes bypass Choro's controls.
        self.request(
            "session/set_mode",
            json!({"sessionId":session, "modeId":session_mode(&self.session_modes, self.read_only_turn)?}),
        )?;
        let _ = self
            .events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running));
        let result = self.request(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":format!("{}\n\n{text}", ide_core::agent_changes::AGENT_CHANGE_INSTRUCTIONS)}]}),
        );
        self.assistant_stream.flush(&self.events);
        let _ = self.events.send_blocking(ChatBackendEvent::ChangedFiles(
            ChangedFilesSummary::attributed(self.active_turn_id.clone(), vec![], vec![]),
        ));
        let result = result?;
        match result.get("stopReason").and_then(Value::as_str) {
            Some("refusal") => {
                let _ = self.events.send_blocking(ChatBackendEvent::Error(
                    "Gemini declined this request.".into(),
                ));
            }
            Some("end_turn")
                if mode == AgentInteractionMode::Plan
                    && !self.assistant_buffer.trim().is_empty() =>
            {
                let _ =
                    self.events
                        .send_blocking(ChatBackendEvent::ProposedPlan(ProposedPlan::new(
                            next_request_id(),
                            &self.assistant_buffer,
                        )));
            }
            _ => {
                if let Some(review) = extract_code_review(&self.assistant_buffer) {
                    let _ =
                        self.events
                            .send_blocking(ChatBackendEvent::CodeReview(CodeReview::new(
                                next_request_id(),
                                review,
                            )));
                }
                if let Some(verification) = extract_verification(&self.assistant_buffer) {
                    let _ = self.events.send_blocking(ChatBackendEvent::Verification(
                        Verification::new(next_request_id(), verification),
                    ));
                }
                if let Some(checklist) = extract_review_checklist(&self.assistant_buffer) {
                    let _ = self.events.send_blocking(ChatBackendEvent::ReviewChecklist(
                        ReviewChecklist::ready("", &checklist, unix_now()),
                    ));
                }
                let _ = self
                    .events
                    .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle));
            }
        }
        Ok(())
    }

    fn handle_command(&mut self, command: ChatBackendCommand) -> anyhow::Result<()> {
        match command {
            ChatBackendCommand::Shutdown | ChatBackendCommand::ForceShutdown => {
                return Err(anyhow!("Gemini stopped"))
            }
            ChatBackendCommand::CancelTurn => {
                let pending = std::mem::take(&mut self.pending_permissions);
                for (_, permission) in pending {
                    self.respond_permission(permission.jsonrpc_id, None)?;
                }
                self.write_json(&json!({"jsonrpc":"2.0","method":"session/cancel","params":{"sessionId":self.session_id}}))?;
            }
            ChatBackendCommand::UpdateAccessMode { access_mode } => self.access_mode = access_mode,
            ChatBackendCommand::UpdateModelEffort { model, .. } => self.agent.model = model,
            ChatBackendCommand::SendTurn {
                text,
                mode,
                read_only,
                turn_id,
            } => self
                .deferred_turns
                .push_back((text, mode, read_only, turn_id)),
            ChatBackendCommand::SendStudioTurn { .. } => {
                return Err(anyhow!("Studio requires a Claude or Codex backend"));
            }
            ChatBackendCommand::ResolveApproval {
                request_id,
                approved,
            } => self.resolve_approval(request_id, approved)?,
            ChatBackendCommand::SubmitUserInput { .. } => {}
        }
        Ok(())
    }

    fn handle_message(&mut self, message: ProviderMessage) -> anyhow::Result<()> {
        let ProviderMessage {
            value: message,
            _reservation,
        } = message;
        if message
            .get("_choro_evidence_incomplete")
            .and_then(Value::as_bool)
            == Some(true)
        {
            self.events.mark_evidence_overflow();
        }
        if message.get("id").is_some() && message.get("method").is_some() {
            return self.handle_server_request(message);
        }
        if message.get("method").and_then(Value::as_str) != Some("session/update")
            || self.loading_history
        {
            return Ok(());
        }
        let Some(update) = message.pointer("/params/update") else {
            return Ok(());
        };
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("agent_message_chunk") => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    self.assistant_buffer.push_str(text);
                    if self.interaction_mode != AgentInteractionMode::Plan {
                        self.assistant_stream.push(None, text, &self.events);
                    }
                }
            }
            Some("agent_thought_chunk") => {
                if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                    let _ = self.events.send_blocking(ChatBackendEvent::ThoughtChunk {
                        message_id: None,
                        text: text.into(),
                    });
                }
            }
            Some("tool_call" | "tool_call_update") => self.handle_tool_update(update),
            _ => {}
        }
        Ok(())
    }

    fn handle_tool_update(&mut self, update: &Value) {
        let Some(id) = update.get("toolCallId").and_then(Value::as_str) else {
            return;
        };
        let mut merged = self.tools.remove(id).unwrap_or_else(|| json!({}));
        if let (Some(target), Some(fields)) = (merged.as_object_mut(), update.as_object()) {
            for (key, value) in fields {
                target.insert(key.clone(), value.clone());
            }
        }
        if merged.get("status").and_then(Value::as_str) == Some("completed") {
            // Only a completed tool with an actual diff is evidence of a write.
            for file in open_code::open_code_confirmed_diffs(&merged, self.agent.runtime_path()) {
                let _ = self
                    .events
                    .send_blocking(ChatBackendEvent::FileChangeActivity(
                        FileChangeActivity::new(
                            format!("gemini:{id}:{}", file.path.display()),
                            self.active_turn_id.clone(),
                            file,
                            false,
                            unix_now(),
                        ),
                    ));
            }
        }
        let _ = self
            .events
            .send_blocking(ChatBackendEvent::WorkLog(open_code::acp_tool_entry(
                &merged,
                "Gemini tool",
            )));
        if !matches!(
            merged.get("status").and_then(Value::as_str),
            Some("completed" | "failed")
        ) {
            let bytes = serde_json::to_vec(&merged).map_or(usize::MAX, |value| value.len());
            if !self.tools.insert(id.to_string(), merged, bytes) {
                self.events.mark_evidence_overflow();
            }
        }
    }
}

impl Drop for GeminiRuntime {
    fn drop(&mut self) {
        terminate_child_process(&mut self.child);
    }
}

impl GeminiRuntime {
    fn handle_server_request(&mut self, message: Value) -> anyhow::Result<()> {
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        if method != "session/request_permission" {
            return self.write_json(&json!({
                "jsonrpc": "2.0",
                "id": message.get("id").cloned().unwrap_or(Value::Null),
                "error": { "code": -32601, "message": format!("Unsupported client method: {method}") }
            }));
        }
        let jsonrpc_id = message.get("id").cloned().unwrap_or(Value::Null);
        let request_id = jsonrpc_id_key(&jsonrpc_id);
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let tool_call = params.get("toolCall").cloned().unwrap_or(Value::Null);
        let tool_kind = tool_call
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("other");
        let title = ide_core::redact_sensitive_text(
            tool_call
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Allow this Gemini action?"),
        );
        let detail = tool_call
            .get("rawInput")
            .filter(|value| !value.is_null())
            .and_then(|value| serde_json::to_string_pretty(value).ok())
            .map(|detail| ide_core::redact_sensitive_text(&detail));
        let options = params
            .get("options")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let option = |kinds: &[&str]| {
            options.iter().find_map(|option| {
                kinds
                    .contains(&option.get("kind").and_then(Value::as_str).unwrap_or(""))
                    .then(|| {
                        option
                            .get("optionId")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .flatten()
            })
        };
        let allow_once = option(&["allow_once"]);
        let allow_always = option(&["allow_always"]);
        let reject = option(&["reject_once", "reject_always"]);
        let automatic = if self.deny_all_tools
            || (self.read_only_turn && !matches!(tool_kind, "read" | "search" | "think"))
        {
            return self.respond_permission(jsonrpc_id, reject);
        } else if matches!(tool_kind, "read" | "search" | "think") {
            // The provider asks even for workspace reads, so tool-free
            // generation can reject them. Ordinary reads need no approval.
            return self.respond_permission(jsonrpc_id, allow_once);
        } else {
            match self.access_mode {
                AgentAccessMode::FullAccess => allow_always.clone().or_else(|| allow_once.clone()),
                AgentAccessMode::AutoAcceptEdits
                    if matches!(tool_kind, "edit" | "delete" | "move") =>
                {
                    allow_once.clone()
                }
                _ => None,
            }
        };
        if let Some(option_id) = automatic {
            return self.respond_permission(jsonrpc_id, Some(option_id));
        }

        self.pending_permissions.insert(
            request_id.clone(),
            GeminiPermission {
                jsonrpc_id,
                allow_option: allow_once.or(allow_always),
                reject_option: reject,
            },
        );
        let kind = match tool_kind {
            "execute" => PendingApprovalKind::Command,
            "edit" | "delete" | "move" => PendingApprovalKind::FileChange,
            _ => PendingApprovalKind::Permissions,
        };
        self.events
            .send_blocking(ChatBackendEvent::PendingApproval(PendingApproval::new(
                request_id, kind, title, detail,
            )))
            .ok();
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::WaitingForUser))
            .ok();
        Ok(())
    }

    fn resolve_approval(&mut self, request_id: String, approved: bool) -> anyhow::Result<()> {
        let Some(permission) = self.pending_permissions.remove(&request_id) else {
            return Ok(());
        };
        let option = if approved {
            permission.allow_option
        } else {
            permission.reject_option
        };
        self.respond_permission(permission.jsonrpc_id, option)?;
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .ok();
        Ok(())
    }

    fn respond_permission(&mut self, id: Value, option_id: Option<String>) -> anyhow::Result<()> {
        let outcome = option_id.map_or_else(
            || json!({ "outcome": "cancelled" }),
            |option_id| json!({ "outcome": "selected", "optionId": option_id }),
        );
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "outcome": outcome }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gemini_execution_modes_use_provider_ids_and_require_read_only_support() {
        let modes = json!({"availableModes":[{"id":"agent"},{"id":"plan"},{"id":"turbo"}]});
        assert_eq!(session_mode(&modes, false).unwrap(), "agent");
        assert_eq!(session_mode(&modes, true).unwrap(), "plan");
        assert!(session_mode(&json!({"availableModes":[{"id":"agent"}]}), true).is_err());
        assert!(session_mode(
            &json!({"availableModes":[{"id":"accept-edits"},{"id":"auto"}]}),
            false
        )
        .is_err());
    }

    #[test]
    fn gemini_only_authentication_errors_trigger_sign_in() {
        assert!(is_authentication_error("Authentication required"));
        assert!(is_authentication_error("auth_required"));
        assert!(!is_authentication_error("Quota exhausted"));
        assert!(!is_authentication_error("Model not available"));
    }

    #[test]
    fn gemini_google_sign_in_requires_explicit_approval() {
        for approved in [false, true] {
            let (mut runtime, _, events) = runtime(AgentAccessMode::FullAccess);
            let (commands, receiver) = crossbeam_channel::unbounded();
            runtime.commands = receiver;
            commands
                .send(ChatBackendCommand::ResolveApproval {
                    request_id: "gemini-google-sign-in".into(),
                    approved,
                })
                .unwrap();
            assert_eq!(runtime.approve_google_sign_in().is_ok(), approved);
            match events.recv_blocking().unwrap() {
                ChatBackendEvent::PendingApproval(approval) => {
                    assert_eq!(approval.title, "Sign in with Google");
                }
                other => panic!("Expected Google sign-in approval, got {other:?}"),
            }
        }
    }

    #[test]
    fn gemini_session_authentication_retries_only_after_consent() {
        for approved in [false, true] {
            let (mut runtime, mut reader, events) = runtime(AgentAccessMode::FullAccess);
            runtime.agent.cli_session_id = None;
            runtime.session_id = None;
            runtime.deadline = Some(Instant::now() + Duration::from_secs(5));
            let (messages, receiver) = crossbeam_channel::unbounded();
            runtime.messages = receiver;
            let (commands, receiver) = crossbeam_channel::unbounded();
            runtime.commands = receiver;
            let server = thread::spawn(move || {
                let expected = if approved {
                    vec!["initialize", "session/new", "authenticate", "session/new"]
                } else {
                    vec!["initialize", "session/new"]
                };
                for (index, method) in expected.iter().enumerate() {
                    let request = response(&mut reader);
                    assert_eq!(request["method"], *method);
                    let reply = if index == 1 {
                        messages.send(json!({"id":request["id"],"error":{"code":-32000,"message":"Authentication required"}}).into()).unwrap();
                        assert!(matches!(
                            events.recv_blocking().unwrap(),
                            ChatBackendEvent::PendingApproval(_)
                        ));
                        commands
                            .send(ChatBackendCommand::ResolveApproval {
                                request_id: "gemini-google-sign-in".into(),
                                approved,
                            })
                            .unwrap();
                        continue;
                    } else if *method == "authenticate" {
                        assert_eq!(request["params"]["methodId"], "oauth-personal");
                        json!({"result":{}})
                    } else if *method == "session/new" {
                        json!({"result":{"sessionId":"google-session", "modes":{"availableModes":[{"id":"agent"},{"id":"plan"}]}}})
                    } else {
                        json!({"result":{"protocolVersion":1}})
                    };
                    let mut reply = reply;
                    reply["id"] = request["id"].clone();
                    messages.send(reply.into()).unwrap();
                }
            });
            assert_eq!(
                runtime.initialize_session(true, json!([])).is_ok(),
                approved
            );
            server.join().unwrap();
            if approved {
                assert_eq!(runtime.session_id.as_deref(), Some("google-session"));
                assert_eq!(session_mode(&runtime.session_modes, true).unwrap(), "plan");
            }
        }
    }

    fn runtime(
        access: AgentAccessMode,
    ) -> (
        GeminiRuntime,
        BufReader<std::process::ChildStdout>,
        async_channel::Receiver<ChatBackendEvent>,
    ) {
        let mut child = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let (_, messages) = crossbeam_channel::unbounded();
        let (_, commands) = crossbeam_channel::unbounded();
        let (events, output) = event_channel();
        let agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            PathBuf::from("/tmp/choro-gemini-test"),
            "Gemini",
            "Hello",
            AgentKind::Gemini,
            AgentModel::Gemini38FlashMedium,
            AgentEffort::Medium,
            access,
        );
        (
            GeminiRuntime {
                child,
                stdin,
                messages,
                commands,
                events,
                shutdown: Arc::new(AtomicBool::new(false)),
                agent,
                session_id: Some("session-test".into()),
                access_mode: access,
                interaction_mode: AgentInteractionMode::Default,
                read_only_turn: false,
                active_turn_id: "turn-test".into(),
                assistant_buffer: String::new(),
                assistant_stream: StreamChunkBuffer::new(),
                pending_permissions: Default::default(),
                deferred_turns: VecDeque::new(),
                loading_history: false,
                tools: Default::default(),
                session_modes: Value::Null,
                deadline: None,
                deny_all_tools: false,
            },
            stdout,
            output,
        )
    }

    fn permission(kind: &str) -> Value {
        json!({"jsonrpc":"2.0", "id":17, "method":"session/request_permission", "params":{
            "toolCall":{"kind":kind,"title":"Do work"},
            "options":[{"kind":"allow_once","optionId":"allow"},{"kind":"reject_once","optionId":"deny"}]
        }})
    }

    fn response(reader: &mut impl BufRead) -> Value {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }

    #[test]
    fn gemini_supervised_permissions_wait_for_the_user() {
        let (mut runtime, mut reader, events) = runtime(AgentAccessMode::AskForApproval);
        runtime
            .handle_server_request(permission("execute"))
            .unwrap();
        assert!(matches!(
            events.recv_blocking().unwrap(),
            ChatBackendEvent::PendingApproval(_)
        ));
        assert_eq!(runtime.pending_permissions.len(), 1);
        runtime.resolve_approval("17".into(), false).unwrap();
        assert_eq!(
            response(&mut reader).pointer("/result/outcome/optionId"),
            Some(&json!("deny"))
        );
    }

    #[test]
    fn gemini_read_only_denies_commands_even_with_full_access() {
        for kind in ["execute", "edit", "delete", "move"] {
            let (mut runtime, mut reader, _) = runtime(AgentAccessMode::FullAccess);
            runtime.read_only_turn = true;
            runtime.handle_server_request(permission(kind)).unwrap();
            assert_eq!(
                response(&mut reader).pointer("/result/outcome/optionId"),
                Some(&json!("deny"))
            );
            assert!(runtime.pending_permissions.is_empty());
        }
    }

    #[test]
    fn gemini_read_only_allows_reads_but_tool_free_denies_every_tool() {
        let (mut runtime, mut reader, _) = runtime(AgentAccessMode::AskForApproval);
        runtime.read_only_turn = true;
        runtime.handle_server_request(permission("read")).unwrap();
        assert_eq!(
            response(&mut reader).pointer("/result/outcome/optionId"),
            Some(&json!("allow"))
        );
        runtime.deny_all_tools = true;
        for kind in ["read", "search", "think", "execute", "edit", "other"] {
            runtime.handle_server_request(permission(kind)).unwrap();
            assert_eq!(
                response(&mut reader).pointer("/result/outcome/optionId"),
                Some(&json!("deny"))
            );
        }
        assert!(runtime.pending_permissions.is_empty());
    }

    #[test]
    fn gemini_auto_edit_approves_edits_and_keeps_command_approval() {
        let (mut runtime, mut reader, _) = runtime(AgentAccessMode::AutoAcceptEdits);
        runtime.handle_server_request(permission("edit")).unwrap();
        assert_eq!(
            response(&mut reader).pointer("/result/outcome/optionId"),
            Some(&json!("allow"))
        );
        runtime
            .handle_server_request(permission("execute"))
            .unwrap();
        assert_eq!(runtime.pending_permissions.len(), 1);
    }

    #[test]
    fn gemini_cancel_resolves_pending_permissions_then_cancels_session() {
        let (mut runtime, mut reader, _) = runtime(AgentAccessMode::AskForApproval);
        runtime.handle_server_request(permission("edit")).unwrap();
        runtime
            .handle_command(ChatBackendCommand::CancelTurn)
            .unwrap();
        assert_eq!(
            response(&mut reader).pointer("/result/outcome/outcome"),
            Some(&json!("cancelled"))
        );
        assert_eq!(
            response(&mut reader).get("method"),
            Some(&json!("session/cancel"))
        );
        assert!(runtime.pending_permissions.is_empty());
    }

    #[test]
    fn gemini_resume_does_not_duplicate_replayed_messages() {
        let (mut runtime, _, events) = runtime(AgentAccessMode::AskForApproval);
        runtime.loading_history = true;
        runtime.handle_message(json!({"method":"session/update","params":{"update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Old message"}}}}).into()).unwrap();
        assert!(runtime.assistant_buffer.is_empty());
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn gemini_only_completed_diffs_are_attributed() {
        let (mut runtime, _, events) = runtime(AgentAccessMode::AskForApproval);
        runtime.handle_tool_update(&json!({"sessionUpdate":"tool_call","toolCallId":"edit-1","status":"in_progress","kind":"edit", "content":[{"type":"diff","path":"/tmp/choro-gemini-test/a.txt","oldText":"before","newText":"after"}]}));
        assert!(matches!(
            unpack_event(events.recv_blocking().unwrap()),
            ChatBackendEvent::WorkLog(_)
        ));
        assert!(events.try_recv().is_err());
        runtime.handle_tool_update(
            &json!({"sessionUpdate":"tool_call_update","toolCallId":"edit-1","status":"completed"}),
        );
        match unpack_event(events.recv_blocking().unwrap()) {
            ChatBackendEvent::FileChangeActivity(activity) => {
                assert_eq!(activity.turn_id, "turn-test");
                assert_eq!(activity.file.path, PathBuf::from("a.txt"));
                assert_eq!(activity.file.result_content.as_deref(), Some("after"));
            }
            other => panic!("Expected file activity, got {other:?}"),
        }
    }
}

fn is_authentication_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("authentication required")
        || message.contains("auth_required")
        || message.contains("not authenticated")
}

fn session_mode(modes: &Value, read_only: bool) -> anyhow::Result<String> {
    let available = modes
        .get("availableModes")
        .and_then(Value::as_array)
        .context("Google did not advertise execution modes")?;
    let selected = available
        .iter()
        .find(|mode| {
            let id = mode
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            if read_only {
                id == "plan" || id == "planning"
            } else {
                matches!(id.as_str(), "default" | "agent")
            }
        })
        .and_then(|mode| mode.get("id"))
        .and_then(Value::as_str)
        .context("Google does not offer the required execution mode")?;
    Ok(selected.to_string())
}

pub(crate) fn one_shot_generation(
    root: &Path,
    model: AgentModel,
    prompt: String,
    images: &[PathBuf],
    timeout: Duration,
    tool_free: bool,
) -> anyhow::Result<String> {
    use base64::Engine as _;
    let agent = AgentRecord::new(
        ide_core::ProjectId(uuid::Uuid::new_v4()),
        root.to_path_buf(),
        "Text generation",
        "",
        AgentKind::Gemini,
        model,
        AgentEffort::Medium,
        AgentAccessMode::AskForApproval,
    );
    let (_commands, rx) = crossbeam_channel::unbounded();
    let (events, _output) = event_channel();
    let mut runtime = start_runtime(
        agent,
        AgentInteractionMode::Default,
        rx,
        events,
        Arc::new(AtomicBool::new(false)),
    )?;
    runtime.deadline = Some(Instant::now() + timeout);
    runtime.initialize_session(false, json!([]))?;
    runtime.read_only_turn = true;
    runtime.deny_all_tools = tool_free;
    let session = runtime
        .session_id
        .clone()
        .context("Google session unavailable")?;
    runtime.request(
        "session/set_model",
        json!({"sessionId":session,"modelId":model.cli_value()}),
    )?;
    runtime.request(
        "session/set_mode",
        json!({"sessionId":session,"modeId":session_mode(&runtime.session_modes, true)?}),
    )?;
    let mut content = vec![json!({"type":"text","text":prompt})];
    for image in images {
        let mime = match image
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            _ => "image/jpeg",
        };
        content.push(json!({"type":"image","mimeType":mime,"data":base64::engine::general_purpose::STANDARD.encode(fs::read(image)?)}));
    }
    let result = runtime.request(
        "session/prompt",
        json!({"sessionId":session,"prompt":content}),
    )?;
    anyhow::ensure!(
        result.get("stopReason").and_then(Value::as_str) == Some("end_turn"),
        "Google did not complete text generation"
    );
    anyhow::ensure!(
        !runtime.assistant_buffer.trim().is_empty(),
        "Google returned no answer"
    );
    Ok(runtime.assistant_buffer.clone())
}
