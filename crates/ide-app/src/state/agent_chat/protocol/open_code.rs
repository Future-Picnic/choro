use super::*;
use std::collections::HashMap;

struct OpenCodePermission {
    jsonrpc_id: Value,
    allow_option: Option<String>,
    reject_option: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenCodePathAttribution {
    Exact,
    Observed,
}

fn open_code_path_attribution(kind: Option<&str>) -> Option<OpenCodePathAttribution> {
    match kind {
        Some("edit" | "delete" | "move") => Some(OpenCodePathAttribution::Exact),
        Some("execute") => Some(OpenCodePathAttribution::Observed),
        _ => None,
    }
}

/// The next thing the OpenCode run loop should react to.
enum OpenCodeInbound {
    Command(ChatBackendCommand),
    CommandsClosed,
    Message(Value),
    MessagesClosed,
    Question(Value),
    QuestionsClosed,
    FlushTick,
}

#[derive(Clone)]
struct OpenCodeQuestionBridge {
    client: reqwest::blocking::Client,
    base_url: String,
    project_path: PathBuf,
    username: String,
    password: String,
}

struct OpenCodeRuntime {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    messages: Receiver<Value>,
    questions: Receiver<Value>,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    agent: AgentRecord,
    session_id: Option<String>,
    model_id: String,
    effort: String,
    access_mode: AgentAccessMode,
    interaction_mode: AgentInteractionMode,
    assistant_buffer: String,
    assistant_stream: StreamChunkBuffer,
    pending_permissions: std::collections::HashMap<String, OpenCodePermission>,
    question_bridge: OpenCodeQuestionBridge,
    known_question_ids: HashSet<String>,
    active_question: Option<Value>,
    queued_questions: VecDeque<Value>,
    changed_paths: HashSet<PathBuf>,
    observed_changed_paths: HashSet<PathBuf>,
    tool_changed_paths: HashMap<String, Vec<(PathBuf, bool)>>,
    active_turn_id: String,
    worktree_baseline: Option<super::worktree_changes::WorktreeChanges>,
    deferred_turns: VecDeque<(String, AgentInteractionMode, bool)>,
    read_only_turn: bool,
    /// True while a `session/prompt` is in flight. The question poller only
    /// needs its fast cadence during a turn — questions are asked by a running
    /// prompt — so an idle chat drops to a slow safety poll.
    turn_active: Arc<AtomicBool>,
}

pub(super) fn spawn_open_code_acp(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    command_rx: Receiver<ChatBackendCommand>,
    event_tx: EventSender,
    shutdown: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    thread::Builder::new()
        .name("choro-opencode-acp".into())
        .spawn(move || {
            let _stopped = super::BackendStoppedOnDrop(stopped);
            if let Err(error) =
                run_open_code_acp(agent, initial_mode, command_rx, event_tx.clone(), shutdown)
            {
                let _ = event_tx.send_blocking(ChatBackendEvent::Error(format!(
                    "OpenCode ACP failed: {error:#}"
                )));
            }
        })?;
    Ok(())
}

fn run_open_code_acp(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    command_rx: Receiver<ChatBackendCommand>,
    event_tx: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let executable = find_opencode_executable().ok_or_else(|| {
        anyhow!(
            "OpenCode is not installed. Install OpenCode and configure models there, then refresh Choro."
        )
    })?;
    let model_id = agent
        .external_model_id
        .clone()
        .context("this OpenCode agent has no provider-qualified model id")?;
    let server_port = available_local_port()?;
    let server_username = "choro".to_string();
    let server_password = uuid::Uuid::new_v4().simple().to_string();
    let question_bridge = OpenCodeQuestionBridge::new(
        server_port,
        agent.runtime_path().to_path_buf(),
        server_username.clone(),
        server_password.clone(),
    )?;

    let mut command = Command::new(&executable);
    command
        .arg("acp")
        .arg("--hostname")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(server_port.to_string())
        .env("PATH", command_path_env())
        .env("OPENCODE_ENABLE_QUESTION_TOOL", "1")
        .env("OPENCODE_SERVER_USERNAME", &server_username)
        .env("OPENCODE_SERVER_PASSWORD", &server_password)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(agent.runtime_path());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().context("failed to start OpenCode ACP")?;
    let stdin = Arc::new(Mutex::new(
        child
            .stdin
            .take()
            .context("OpenCode ACP stdin unavailable")?,
    ));
    let stdout = child
        .stdout
        .take()
        .context("OpenCode ACP stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("OpenCode ACP stderr unavailable")?;
    let (message_tx, message_rx) = crossbeam_channel::unbounded();
    spawn_json_reader(stdout, message_tx);
    spawn_stderr_reader(stderr, event_tx.clone(), "OpenCode ACP");
    let (question_tx, question_rx) = crossbeam_channel::unbounded();

    let mut runtime = OpenCodeRuntime {
        child,
        stdin,
        messages: message_rx,
        questions: question_rx,
        commands: command_rx,
        events: event_tx,
        shutdown,
        session_id: None,
        effort: agent.effort.cli_value().to_string(),
        access_mode: agent.access_mode,
        interaction_mode: initial_mode,
        model_id,
        assistant_buffer: String::new(),
        assistant_stream: StreamChunkBuffer::new(),
        pending_permissions: std::collections::HashMap::new(),
        question_bridge,
        known_question_ids: HashSet::new(),
        active_question: None,
        queued_questions: VecDeque::new(),
        changed_paths: HashSet::new(),
        observed_changed_paths: HashSet::new(),
        tool_changed_paths: HashMap::new(),
        active_turn_id: next_request_id(),
        worktree_baseline: None,
        deferred_turns: VecDeque::new(),
        read_only_turn: false,
        turn_active: Arc::new(AtomicBool::new(false)),
        agent,
    };

    runtime.request(
        "initialize",
        json!({
            "protocolVersion": 1,
            "clientCapabilities": {},
            "clientInfo": { "name": "Choro", "version": env!("CARGO_PKG_VERSION") }
        }),
    )?;

    let existing_session_id = runtime.agent.cli_session_id.clone();
    let was_resumed = existing_session_id.is_some();
    let session_id = if let Some(session_id) = existing_session_id {
        runtime.request(
            "session/resume",
            json!({
                "sessionId": session_id.clone(),
                "cwd": runtime.agent.runtime_path(),
                "mcpServers": choro_acp_mcp_servers_json(&runtime.agent)
            }),
        )?;
        session_id
    } else {
        let session = runtime.request(
            "session/new",
            json!({
                "cwd": runtime.agent.runtime_path(),
                "mcpServers": choro_acp_mcp_servers_json(&runtime.agent)
            }),
        )?;
        session
            .get("sessionId")
            .and_then(Value::as_str)
            .context("OpenCode ACP did not return a session id")?
            .to_string()
    };
    runtime.session_id = Some(session_id.clone());
    // Start polling only after the ACP session id is known. Otherwise a
    // question observed during startup can be marked as seen before it can be
    // matched to this runtime, which would make it disappear from Choro.
    spawn_open_code_question_poller(
        runtime.question_bridge.clone(),
        question_tx,
        runtime.shutdown.clone(),
        runtime.turn_active.clone(),
    );
    runtime
        .events
        .send_blocking(ChatBackendEvent::SessionReady {
            session_id: session_id.clone(),
        })
        .ok();
    // OpenCode is the source of truth for usage. Reading the whole provider
    // session here preserves the accumulated total when an agent is resumed.
    if let Err(error) = runtime.refresh_usage() {
        eprintln!("failed to read initial OpenCode usage: {error:#}");
    }

    runtime.set_config_option("model", runtime.model_id.clone())?;
    if runtime
        .agent
        .external_model_variants
        .iter()
        .any(|variant| variant == &runtime.effort)
    {
        runtime.set_config_option("effort", runtime.effort.clone())?;
    }

    if !was_resumed && !runtime.agent.hidden_doc_assistant && !runtime.agent.doc.trim().is_empty() {
        runtime.send_turn(runtime.agent.doc.clone(), initial_mode, false)?;
    }
    runtime.run_loop()
}

impl OpenCodeRuntime {
    fn run_loop(&mut self) -> anyhow::Result<()> {
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                self.assistant_stream.flush(&self.events);
                return Ok(());
            }
            self.drain_questions()?;
            if let Some((text, mode, read_only)) = self.deferred_turns.pop_front() {
                self.send_turn(text, mode, read_only)?;
                continue;
            }
            match self.next_inbound() {
                OpenCodeInbound::Command(ChatBackendCommand::Shutdown) => return Ok(()),
                OpenCodeInbound::Command(ChatBackendCommand::ForceShutdown) => {
                    return Err(anyhow!("OpenCode ACP force-stopped"));
                }
                OpenCodeInbound::Command(command) => self.handle_command(command)?,
                OpenCodeInbound::CommandsClosed => return Ok(()),
                OpenCodeInbound::Message(message) => self.handle_message(message)?,
                OpenCodeInbound::Question(question) => self.enqueue_question(question)?,
                OpenCodeInbound::QuestionsClosed => {
                    // The poller thread is gone; stop selecting on its closed
                    // channel so the loop can keep blocking instead of spinning.
                    self.questions = crossbeam_channel::never();
                }
                OpenCodeInbound::FlushTick => self.assistant_stream.flush_due(&self.events),
                OpenCodeInbound::MessagesClosed => {
                    let detail = match self.child.try_wait() {
                        Ok(Some(status)) => format!("OpenCode ACP exited with status {status}"),
                        Ok(None) => "OpenCode ACP stdout closed unexpectedly".to_string(),
                        Err(error) => format!("OpenCode ACP stdout closed: {error}"),
                    };
                    return Err(anyhow!(detail));
                }
            }
        }
    }

    /// Block on commands, server messages, and bridged questions at once;
    /// commands drain with priority and a timed tick only exists while
    /// streamed text waits on a flush. An idle OpenCode chat parks here.
    fn next_inbound(&self) -> OpenCodeInbound {
        match self.commands.try_recv() {
            Ok(command) => return OpenCodeInbound::Command(command),
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                return OpenCodeInbound::CommandsClosed;
            }
        }
        if self.assistant_stream.has_pending() {
            crossbeam_channel::select! {
                recv(self.commands) -> command => match command {
                    Ok(command) => OpenCodeInbound::Command(command),
                    Err(_) => OpenCodeInbound::CommandsClosed,
                },
                recv(self.messages) -> message => match message {
                    Ok(message) => OpenCodeInbound::Message(message),
                    Err(_) => OpenCodeInbound::MessagesClosed,
                },
                recv(self.questions) -> question => match question {
                    Ok(question) => OpenCodeInbound::Question(question),
                    Err(_) => OpenCodeInbound::QuestionsClosed,
                },
                default(CHAT_STREAM_FLUSH_INTERVAL) => OpenCodeInbound::FlushTick,
            }
        } else {
            crossbeam_channel::select! {
                recv(self.commands) -> command => match command {
                    Ok(command) => OpenCodeInbound::Command(command),
                    Err(_) => OpenCodeInbound::CommandsClosed,
                },
                recv(self.messages) -> message => match message {
                    Ok(message) => OpenCodeInbound::Message(message),
                    Err(_) => OpenCodeInbound::MessagesClosed,
                },
                recv(self.questions) -> question => match question {
                    Ok(question) => OpenCodeInbound::Question(question),
                    Err(_) => OpenCodeInbound::QuestionsClosed,
                },
            }
        }
    }

    fn request(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        let id = next_request_id();
        self.write_json(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        // A model turn can legitimately run longer than three minutes. Keep
        // startup/configuration requests bounded, while prompt cancellation is
        // handled explicitly through session/cancel and ForceShutdown.
        let deadline =
            (method != "session/prompt").then(|| Instant::now() + Duration::from_secs(180));
        loop {
            if self.shutdown.load(Ordering::SeqCst) {
                return Err(anyhow!("OpenCode ACP is shutting down"));
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(anyhow!("timed out waiting for OpenCode {method}"));
            }
            self.drain_questions()?;
            self.handle_commands_while_blocked()?;
            let message = match self.messages.recv_timeout(Duration::from_millis(40)) {
                Ok(message) => message,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    self.assistant_stream.flush_due(&self.events);
                    continue;
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("OpenCode ACP stdout closed"));
                }
            };
            if message.get("id").and_then(Value::as_str) == Some(id.as_str()) {
                if let Some(error) = message.get("error") {
                    return Err(anyhow!("OpenCode {method} returned error: {error}"));
                }
                return Ok(message.get("result").cloned().unwrap_or(Value::Null));
            }
            self.handle_message(message)?;
        }
    }

    fn notify(&self, method: &str, params: Value) -> anyhow::Result<()> {
        self.write_json(&json!({ "jsonrpc": "2.0", "method": method, "params": params }))
    }

    fn set_config_option(&mut self, config_id: &str, value: String) -> anyhow::Result<()> {
        let Some(session_id) = self.session_id.clone() else {
            return Err(anyhow!("OpenCode session is not ready"));
        };
        self.request(
            "session/set_config_option",
            json!({ "sessionId": session_id, "configId": config_id, "value": value }),
        )?;
        Ok(())
    }

    fn send_turn(
        &mut self,
        text: String,
        mode: AgentInteractionMode,
        read_only: bool,
    ) -> anyhow::Result<()> {
        let Some(session_id) = self.session_id.clone() else {
            return Err(anyhow!("OpenCode session is not ready"));
        };
        self.set_config_option("model", self.model_id.clone())?;
        self.assistant_stream.reset(&self.events);
        self.assistant_buffer.clear();
        self.active_turn_id = next_request_id();
        self.read_only_turn = read_only;
        self.tool_changed_paths.clear();
        self.worktree_baseline = super::worktree_changes::WorktreeChanges::capture(&self.agent)
            .map_err(|error| eprintln!("failed to capture OpenCode file baseline: {error:#}"))
            .ok();
        self.interaction_mode = mode;
        let is_plan = mode == AgentInteractionMode::Plan;
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .ok();
        let mode = match mode {
            AgentInteractionMode::Default => "build",
            AgentInteractionMode::Plan => "plan",
        };
        // OpenCode exposes Build/Plan as a session config option. Older ACP
        // builds may not expose it, so a failed mode switch must not block chat.
        let _ = self.set_config_option("mode", mode.to_string());
        self.turn_active.store(true, Ordering::SeqCst);
        let result = self.request(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }),
        );
        self.turn_active.store(false, Ordering::SeqCst);
        if result.is_err() {
            self.emit_changed_files_receipt();
        }
        let result = result?;
        self.assistant_stream.flush(&self.events);
        let stop_reason = result
            .get("stopReason")
            .and_then(Value::as_str)
            .unwrap_or("end_turn");
        let proposed_plan = if open_code_should_emit_plan(is_plan, stop_reason) {
            open_code_proposed_plan(&self.assistant_buffer)
        } else {
            None
        };
        if let Some(markdown) = proposed_plan.as_deref() {
            self.events
                .send_blocking(ChatBackendEvent::ProposedPlan(ProposedPlan::new(
                    next_request_id(),
                    markdown,
                )))
                .ok();
        } else {
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
        }
        self.emit_changed_files_receipt();
        if stop_reason == "refusal" {
            self.events
                .send_blocking(ChatBackendEvent::Error(
                    "OpenCode declined to continue this turn.".to_string(),
                ))
                .ok();
        } else if proposed_plan.is_none() {
            self.events
                .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Idle))
                .ok();
        }
        // Usage is supplementary and the provider endpoint returns the full
        // session. Never keep turn completion or a queued prompt waiting on
        // that potentially slow request.
        self.refresh_usage_async();
        Ok(())
    }

    fn refresh_usage_async(&self) {
        let Some(session_id) = self.session_id.clone() else {
            return;
        };
        let bridge = self.question_bridge.clone();
        let events = self.events.clone();
        if let Err(error) = thread::Builder::new()
            .name("choro-opencode-usage".into())
            .spawn(move || match bridge.session_messages(&session_id) {
                Ok(messages) => {
                    if let Some(usage) = open_code_usage_from_messages(&session_id, &messages) {
                        events.send_blocking(ChatBackendEvent::Usage(usage)).ok();
                    }
                }
                Err(error) => {
                    eprintln!("failed to refresh OpenCode usage: {error:#}");
                }
            })
        {
            eprintln!("failed to start OpenCode usage refresh: {error}");
        }
    }

    fn refresh_usage(&self) -> anyhow::Result<()> {
        let Some(session_id) = self.session_id.as_deref() else {
            return Ok(());
        };
        let messages = self.question_bridge.session_messages(session_id)?;
        if let Some(usage) = open_code_usage_from_messages(session_id, &messages) {
            self.events
                .send_blocking(ChatBackendEvent::Usage(usage))
                .ok();
        }
        Ok(())
    }

    fn handle_command(&mut self, command: ChatBackendCommand) -> anyhow::Result<()> {
        match command {
            ChatBackendCommand::Shutdown => return Ok(()),
            ChatBackendCommand::ForceShutdown => {
                return Err(anyhow!("OpenCode ACP force-stopped"));
            }
            ChatBackendCommand::CancelTurn => self.cancel_turn()?,
            ChatBackendCommand::SendTurn {
                text,
                mode,
                read_only,
            } => self.send_turn(text, mode, read_only)?,
            ChatBackendCommand::UpdateAccessMode { access_mode } => self.access_mode = access_mode,
            ChatBackendCommand::UpdateModelEffort { effort, external_model, .. } => {
                if let Some((id,variants))=external_model {self.model_id=id;self.agent.external_model_variants=variants;}
                self.effort = effort.cli_value().to_string();
                if self
                    .agent
                    .external_model_variants
                    .iter()
                    .any(|variant| variant == &self.effort)
                {
                    self.set_config_option("effort", self.effort.clone())?;
                }
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
                    return Err(anyhow!("OpenCode ACP is shutting down"));
                }
                Ok(ChatBackendCommand::ForceShutdown) => {
                    return Err(anyhow!("OpenCode ACP force-stopped"));
                }
                Ok(ChatBackendCommand::CancelTurn) => self.cancel_turn()?,
                Ok(ChatBackendCommand::UpdateAccessMode { access_mode }) => {
                    self.access_mode = access_mode
                }
                Ok(ChatBackendCommand::ResolveApproval {
                    request_id,
                    approved,
                }) => self.resolve_approval(request_id, approved)?,
                Ok(ChatBackendCommand::SendTurn {
                    text,
                    mode,
                    read_only,
                }) => {
                    self.deferred_turns.push_back((text, mode, read_only));
                }
                Ok(ChatBackendCommand::UpdateModelEffort { effort, external_model, .. }) => {
                    if let Some((id,variants))=external_model {self.model_id=id;self.agent.external_model_variants=variants;}
                self.effort = effort.cli_value().to_string();
                }
                Ok(ChatBackendCommand::SubmitUserInput {
                    request_id,
                    answers,
                }) => self.submit_user_input(request_id, answers)?,
                Err(crossbeam_channel::TryRecvError::Empty) => return Ok(()),
                Err(crossbeam_channel::TryRecvError::Disconnected) => {
                    return Err(anyhow!("chat command channel closed"));
                }
            }
        }
    }

    fn drain_questions(&mut self) -> anyhow::Result<()> {
        loop {
            match self.questions.try_recv() {
                Ok(question) => self.enqueue_question(question)?,
                Err(crossbeam_channel::TryRecvError::Empty) => return Ok(()),
                Err(crossbeam_channel::TryRecvError::Disconnected) => return Ok(()),
            }
        }
    }

    fn enqueue_question(&mut self, question: Value) -> anyhow::Result<()> {
        let Some(request_id) = question.get("id").and_then(Value::as_str) else {
            return Ok(());
        };
        if question.get("sessionID").and_then(Value::as_str) != self.session_id.as_deref() {
            return Ok(());
        }
        if !self.known_question_ids.insert(request_id.to_string()) {
            return Ok(());
        }
        self.queued_questions.push_back(question);
        self.show_next_question()
    }

    fn show_next_question(&mut self) -> anyhow::Result<()> {
        if self.active_question.is_some() {
            return Ok(());
        }
        while let Some(question) = self.queued_questions.pop_front() {
            let Some(pending) = pending_user_input_from_open_code(&question) else {
                continue;
            };
            self.active_question = Some(question);
            self.events
                .send_blocking(ChatBackendEvent::PendingUserInput(pending))
                .ok();
            self.events
                .send_blocking(ChatBackendEvent::Status(AgentChatStatus::WaitingForUser))
                .ok();
            break;
        }
        Ok(())
    }

    fn submit_user_input(
        &mut self,
        request_id: String,
        answers: Vec<String>,
    ) -> anyhow::Result<()> {
        let Some(question) = self.active_question.as_ref() else {
            return Ok(());
        };
        if question.get("id").and_then(Value::as_str) != Some(request_id.as_str()) {
            return Ok(());
        }
        let answers = open_code_answer_groups(question, &answers);
        self.question_bridge.reply(&request_id, &answers)?;
        self.active_question = None;
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Running))
            .ok();
        self.show_next_question()
    }

    fn reject_pending_questions(&mut self) {
        if let Some(question) = self.active_question.take() {
            if let Some(request_id) = question.get("id").and_then(Value::as_str) {
                let _ = self.question_bridge.reject(request_id);
            }
        }
        for question in self.queued_questions.drain(..) {
            if let Some(request_id) = question.get("id").and_then(Value::as_str) {
                let _ = self.question_bridge.reject(request_id);
            }
        }
    }

    fn cancel_turn(&mut self) -> anyhow::Result<()> {
        self.assistant_stream.flush(&self.events);
        self.reject_pending_questions();
        let pending = self
            .pending_permissions
            .drain()
            .map(|(_, permission)| permission)
            .collect::<Vec<_>>();
        for permission in pending {
            self.respond_permission(permission.jsonrpc_id, permission.reject_option)?;
        }
        if let Some(session_id) = self.session_id.clone() {
            self.notify("session/cancel", json!({ "sessionId": session_id }))?;
        }
        self.events
            .send_blocking(ChatBackendEvent::Status(AgentChatStatus::Cancelling))
            .ok();
        Ok(())
    }

    fn handle_message(&mut self, message: Value) -> anyhow::Result<()> {
        if message.get("method").is_some() && message.get("id").is_some() {
            return self.handle_server_request(message);
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            return Ok(());
        };
        if method != "session/update" {
            return Ok(());
        }
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let update = params.get("update").unwrap_or(&Value::Null);
        let Some(update_type) = update.get("sessionUpdate").and_then(Value::as_str) else {
            return Ok(());
        };
        if update_type != "agent_message_chunk" {
            self.assistant_stream.flush(&self.events);
        }
        match update_type {
            "agent_message_chunk" => {
                if let Some(text) = update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str)
                {
                    self.assistant_buffer.push_str(text);
                    if self.interaction_mode != AgentInteractionMode::Plan {
                        self.assistant_stream.push(
                            update
                                .get("messageId")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            text,
                            &self.events,
                        );
                    }
                }
            }
            "agent_thought_chunk" => {
                if let Some(text) = update
                    .get("content")
                    .and_then(|content| content.get("text"))
                    .and_then(Value::as_str)
                {
                    self.events
                        .send_blocking(ChatBackendEvent::ThoughtChunk {
                            message_id: update
                                .get("messageId")
                                .and_then(Value::as_str)
                                .map(str::to_string),
                            text: text.to_string(),
                        })
                        .ok();
                }
            }
            "tool_call" | "tool_call_update" => {
                let action_id = update
                    .get("toolCallId")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(next_request_id);
                let paths = self.track_changed_paths(update);
                if !paths.is_empty() {
                    let tracked = self
                        .tool_changed_paths
                        .entry(action_id.clone())
                        .or_default();
                    for path in paths {
                        if !tracked.contains(&path) {
                            tracked.push(path);
                        }
                    }
                }
                if matches!(
                    update.get("status").and_then(Value::as_str),
                    Some("completed" | "failed")
                ) {
                    let paths = self
                        .tool_changed_paths
                        .remove(&action_id)
                        .unwrap_or_default();
                    if update.get("status").and_then(Value::as_str) == Some("completed") {
                        for (path, observed) in &paths {
                            if *observed {
                                self.observed_changed_paths.insert(path.clone());
                            } else {
                                self.changed_paths.insert(path.clone());
                            }
                        }
                        self.emit_file_change_activities(&action_id, &paths);
                    } else {
                        // A failed mutation may still have written files. Only
                        // disk evidence establishes that; its requested path is
                        // not proof that an already-dirty file was edited.
                        self.emit_file_change_activities(&action_id, &[]);
                    }
                }
                self.events
                    .send_blocking(ChatBackendEvent::WorkLog(open_code_tool_entry(update)))
                    .ok();
            }
            "plan" => {
                let entries = update
                    .get("entries")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                let detail = entries
                    .iter()
                    .filter_map(|entry| {
                        Some(format!(
                            "{}\t{}",
                            entry.get("status")?.as_str()?,
                            entry.get("content")?.as_str()?
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let all_done = !entries.is_empty()
                    && entries.iter().all(|entry| {
                        entry.get("status").and_then(Value::as_str) == Some("completed")
                    });
                self.events
                    .send_blocking(ChatBackendEvent::WorkLog(
                        WorkLogEntry::new(
                            "opencode-plan",
                            "opencode-plan",
                            WorkLogEntryKind::Plan,
                            "OpenCode plan",
                            if all_done {
                                WorkLogStatus::Completed
                            } else {
                                WorkLogStatus::InProgress
                            },
                        )
                        .detail(Some(detail)),
                    ))
                    .ok();
            }
            _ => {}
        }
        Ok(())
    }

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
                .unwrap_or("Allow this OpenCode action?"),
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
        let automatic = if self.read_only_turn
            && matches!(tool_kind, "execute" | "edit" | "delete" | "move")
        {
            return self.respond_permission(jsonrpc_id, reject);
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
            OpenCodePermission {
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

    fn respond_permission(&self, id: Value, option_id: Option<String>) -> anyhow::Result<()> {
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

    fn write_json(&self, value: &Value) -> anyhow::Result<()> {
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("OpenCode ACP stdin lock poisoned"))?;
        writeln!(stdin, "{}", serde_json::to_string(value)?)?;
        stdin.flush()?;
        Ok(())
    }

    fn track_changed_paths(&mut self, update: &Value) -> Vec<(PathBuf, bool)> {
        let kind = update.get("kind").and_then(Value::as_str);
        let Some(attribution) = open_code_path_attribution(kind) else {
            // Reads often carry `locations`; treating those as mutations was
            // another route for already-dirty files to leak into a chat.
            return Vec::new();
        };
        let exact = attribution == OpenCodePathAttribution::Exact;
        let mut paths = Vec::new();
        for location in update
            .get("locations")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(path) = location.get("path").and_then(Value::as_str) else {
                continue;
            };
            let path = project_relative_path(self.agent.runtime_path(), Path::new(path));
            if !paths.iter().any(|(existing, _)| existing == &path) {
                paths.push((path, !exact));
            }
        }

        // OpenCode's resumed ACP stream may omit `locations` from completed
        // tool calls. Recover mutation paths from raw input, but only for
        // mutation kinds so a read of an already-dirty file is not attributed
        // to the agent.
        if matches!(kind, Some("edit" | "delete" | "move")) {
            let Some(raw_input) = update.get("rawInput").and_then(Value::as_object) else {
                return paths;
            };
            for key in ["filePath", "filepath", "path", "oldPath", "newPath"] {
                let Some(path) = raw_input.get(key).and_then(Value::as_str) else {
                    continue;
                };
                let path = project_relative_path(self.agent.runtime_path(), Path::new(path));
                if !paths.iter().any(|(existing, _)| existing == &path) {
                    paths.push((path, false));
                }
            }
        }
        paths
    }

    fn emit_file_change_activities(&self, action_id: &str, paths: &[(PathBuf, bool)]) {
        let summary = self.changed_files_summary();
        let mut paths = paths.to_vec();
        for file in &summary.observed_files {
            if !paths.iter().any(|(path, _)| *path == file.path) {
                paths.push((file.path.clone(), true));
            }
        }
        for (path, observed) in &paths {
            let file = summary
                .files
                .iter()
                .chain(&summary.observed_files)
                .find(|file| project_relative_path(self.agent.runtime_path(), &file.path) == *path)
                .cloned();
            let Some(file) = file else {
                continue;
            };
            let activity_id = format!("opencode:{action_id}:{}", path.to_string_lossy());
            self.events
                .send_blocking(ChatBackendEvent::FileChangeActivity(
                    FileChangeActivity::new(
                        activity_id,
                        self.active_turn_id.clone(),
                        file.as_count_projection(),
                        *observed,
                        unix_now(),
                    ),
                ))
                .ok();
        }
    }

    fn emit_changed_files_receipt(&mut self) {
        let changed = self.changed_files_summary();
        self.changed_paths.clear();
        self.observed_changed_paths.clear();
        self.worktree_baseline = None;
        if !changed.is_empty() {
            let changed = capture_changed_files_snapshot(&self.agent, changed, "opencode-acp");
            self.events
                .send_blocking(ChatBackendEvent::ChangedFiles(changed))
                .ok();
        }
    }

    fn changed_files_summary(&self) -> ChangedFilesSummary {
        let diffs = if self.agent.repository_path.is_none() && !self.agent.is_active_solo() {
            ide_core::git::workspace_worktree_diffs(&self.agent.project_path)
        } else {
            ide_core::git::worktree_diffs(self.agent.runtime_path())
        };
        let Ok(diffs) = diffs else {
            return ChangedFilesSummary::default();
        };
        let files = diffs
            .into_iter()
            .filter_map(|diff| {
                let path = project_relative_path(self.agent.runtime_path(), &diff.path);
                let exact = self.changed_paths.contains(&path);
                let observed = self.observed_changed_paths.contains(&path);
                if !exact && !observed {
                    return None;
                }
                let additions = diff
                    .hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .filter(|line| line.origin == ide_core::git::LineOrigin::Add)
                    .count();
                let deletions = diff
                    .hunks
                    .iter()
                    .flat_map(|hunk| &hunk.lines)
                    .filter(|line| line.origin == ide_core::git::LineOrigin::Remove)
                    .count();
                Some((
                    exact,
                    FileChangeStat::new(diff.path, additions, deletions).as_count_projection(),
                ))
            })
            .collect::<Vec<_>>();
        let mut summary = ChangedFilesSummary::attributed(
            self.active_turn_id.clone(),
            files
                .iter()
                .filter(|(exact, _)| *exact)
                .map(|(_, file)| file.clone())
                .collect(),
            files
                .into_iter()
                .filter(|(exact, _)| !exact)
                .map(|(_, file)| file)
                .collect(),
        );
        if let Some(before) = &self.worktree_baseline {
            match super::worktree_changes::WorktreeChanges::capture(&self.agent) {
                Ok(current) => upsert_file_change_stats(
                    &mut summary.observed_files,
                    current.changes_since(before),
                ),
                Err(error) => eprintln!("failed to observe OpenCode file changes: {error:#}"),
            }
        }
        summary.reconcile_final_files(self.agent.runtime_path());
        summary
    }
}

impl Drop for OpenCodeRuntime {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        terminate_child_process(&mut self.child);
    }
}

impl OpenCodeQuestionBridge {
    fn new(
        port: u16,
        project_path: PathBuf,
        username: String,
        password: String,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(1))
                .timeout(Duration::from_secs(2))
                .build()
                .context("failed to create the OpenCode question client")?,
            base_url: format!("http://127.0.0.1:{port}"),
            project_path,
            username,
            password,
        })
    }

    fn list(&self) -> anyhow::Result<Vec<Value>> {
        let directory = self.project_path.to_string_lossy().to_string();
        let response = self
            .client
            .get(format!("{}/question", self.base_url))
            .basic_auth(&self.username, Some(&self.password))
            .query(&[("directory", directory)])
            .send()
            .context("failed to read OpenCode questions")?
            .error_for_status()
            .context("OpenCode question list returned an error")?;
        let body = response
            .text()
            .context("failed to read the OpenCode question response")?;
        let questions: Value =
            serde_json::from_str(&body).context("OpenCode returned invalid question data")?;
        Ok(questions.as_array().cloned().unwrap_or_default())
    }

    fn session_messages(&self, session_id: &str) -> anyhow::Result<Vec<Value>> {
        let directory = self.project_path.to_string_lossy().to_string();
        let response = self
            .client
            .get(format!("{}/session/{session_id}/message", self.base_url))
            .basic_auth(&self.username, Some(&self.password))
            .query(&[("directory", directory)])
            .timeout(Duration::from_secs(5))
            .send()
            .context("failed to read OpenCode session usage")?
            .error_for_status()
            .context("OpenCode session message list returned an error")?;
        let body = response
            .text()
            .context("failed to read the OpenCode session message response")?;
        let messages: Value = serde_json::from_str(&body)
            .context("OpenCode returned invalid session message data")?;
        Ok(messages.as_array().cloned().unwrap_or_default())
    }

    fn reply(&self, request_id: &str, answers: &[Vec<String>]) -> anyhow::Result<()> {
        self.post(request_id, "reply", Some(json!({ "answers": answers })))
            .with_context(|| format!("failed to answer OpenCode question {request_id}"))
    }

    fn reject(&self, request_id: &str) -> anyhow::Result<()> {
        self.post(request_id, "reject", None)
            .with_context(|| format!("failed to reject OpenCode question {request_id}"))
    }

    fn post(&self, request_id: &str, action: &str, body: Option<Value>) -> anyhow::Result<()> {
        let directory = self.project_path.to_string_lossy().to_string();
        let mut request = self
            .client
            .post(format!("{}/question/{request_id}/{action}", self.base_url))
            .basic_auth(&self.username, Some(&self.password))
            .query(&[("directory", directory)]);
        if let Some(body) = body {
            request = request
                .header("content-type", "application/json")
                .body(serde_json::to_vec(&body)?);
        }
        request
            .send()
            .context("failed to contact the OpenCode question API")?
            .error_for_status()
            .context("OpenCode question API returned an error")?;
        Ok(())
    }
}

fn open_code_usage_from_messages(
    session_id: &str,
    messages: &[Value],
) -> Option<ConversationUsage> {
    let mut totals = UsageTotals::default();
    let mut latest_turn = None;
    let mut models = std::collections::BTreeMap::<(String, String), UsageTotals>::new();
    let mut seen_ids = HashSet::new();
    let mut has_usage = false;

    for item in messages {
        // The HTTP SDK returns `{ info, parts }`; accepting the bare info shape
        // as well keeps this compatible with older OpenCode server builds.
        let info = item.get("info").unwrap_or(item);
        if info.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        if let Some(message_id) = info.get("id").and_then(Value::as_str) {
            if !seen_ids.insert(message_id.to_string()) {
                continue;
            }
        }
        let tokens = info.get("tokens");
        let cost = info.get("cost").and_then(Value::as_f64).unwrap_or(0.0);
        if tokens.is_none() && !info.get("cost").is_some_and(Value::is_number) {
            continue;
        }
        let cache = tokens.and_then(|tokens| tokens.get("cache"));
        let turn = UsageTotals {
            reported_total_tokens: json_u64(tokens.and_then(|tokens| tokens.get("total"))),
            input_tokens: json_u64(tokens.and_then(|tokens| tokens.get("input"))),
            output_tokens: json_u64(tokens.and_then(|tokens| tokens.get("output"))),
            reasoning_tokens: json_u64(tokens.and_then(|tokens| tokens.get("reasoning"))),
            cache_read_tokens: json_u64(cache.and_then(|cache| cache.get("read"))),
            cache_write_tokens: json_u64(cache.and_then(|cache| cache.get("write"))),
            cost_usd: cost,
        };
        totals.add_assign(&turn);
        latest_turn = Some(turn.clone());
        has_usage = true;

        let provider_id = info
            .get("providerID")
            .and_then(Value::as_str)
            .unwrap_or("OpenCode")
            .to_string();
        let model_id = info
            .get("modelID")
            .and_then(Value::as_str)
            .unwrap_or("Unknown model")
            .to_string();
        models
            .entry((provider_id, model_id))
            .or_default()
            .add_assign(&turn);
    }

    has_usage.then(|| ConversationUsage {
        session_id: session_id.to_string(),
        totals,
        latest_turn,
        models: models
            .into_iter()
            .map(|((provider_id, model_id), totals)| ModelUsage {
                provider_id,
                model_id,
                totals,
            })
            .collect(),
    })
}

fn json_u64(value: Option<&Value>) -> u64 {
    value
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
                .or_else(|| {
                    value
                        .as_f64()
                        .filter(|value| value.is_finite() && *value >= 0.0)
                        .map(|value| value as u64)
                })
        })
        .unwrap_or(0)
}

fn available_local_port() -> anyhow::Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .context("failed to reserve a local port for OpenCode")?;
    Ok(listener.local_addr()?.port())
}

const OPEN_CODE_QUESTION_POLL_ACTIVE: Duration = Duration::from_millis(120);
const OPEN_CODE_QUESTION_POLL_IDLE: Duration = Duration::from_millis(1_500);

fn spawn_open_code_question_poller(
    bridge: OpenCodeQuestionBridge,
    questions: Sender<Value>,
    shutdown: Arc<AtomicBool>,
    turn_active: Arc<AtomicBool>,
) {
    let _ = thread::Builder::new()
        .name("choro-opencode-questions".into())
        .spawn(move || {
            let mut seen = HashSet::new();
            while !shutdown.load(Ordering::SeqCst) {
                if let Ok(pending) = bridge.list() {
                    for question in pending {
                        let Some(request_id) = question.get("id").and_then(Value::as_str) else {
                            continue;
                        };
                        if seen.insert(request_id.to_string()) && questions.send(question).is_err()
                        {
                            return;
                        }
                    }
                }
                // Questions are raised by an in-flight prompt: poll fast during
                // a turn, and keep only a slow safety poll while the chat idles.
                let interval = if turn_active.load(Ordering::SeqCst) {
                    OPEN_CODE_QUESTION_POLL_ACTIVE
                } else {
                    OPEN_CODE_QUESTION_POLL_IDLE
                };
                thread::sleep(interval);
            }
        });
}

fn pending_user_input_from_open_code(question: &Value) -> Option<PendingUserInput> {
    let request_id = question.get("id")?.as_str()?.to_string();
    let questions = question
        .get("questions")?
        .as_array()?
        .iter()
        .enumerate()
        .filter_map(|(index, question)| {
            let question_text = question.get("question")?.as_str()?.trim();
            if question_text.is_empty() {
                return None;
            }
            let header = question
                .get("header")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|header| !header.is_empty())
                .unwrap_or("Question");
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| {
                    let label = option.get("label")?.as_str()?.trim();
                    if label.is_empty() {
                        return None;
                    }
                    Some(PendingUserInputOption::new(
                        label,
                        option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or_default(),
                    ))
                })
                .collect::<Vec<_>>();
            let id = format!("{request_id}:{index}");
            if question
                .get("multiple")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                Some(PendingUserInputQuestion::pick_many(
                    id,
                    header,
                    question_text,
                    options,
                ))
            } else {
                Some(PendingUserInputQuestion::pick_one(
                    id,
                    header,
                    question_text,
                    options,
                ))
            }
        })
        .collect::<Vec<_>>();
    (!questions.is_empty()).then(|| PendingUserInput::new(request_id, questions))
}

fn open_code_answer_groups(request: &Value, answers: &[String]) -> Vec<Vec<String>> {
    let questions = request.get("questions").and_then(Value::as_array);
    answers
        .iter()
        .enumerate()
        .map(|(index, answer)| {
            let Some(question) = questions.and_then(|questions| questions.get(index)) else {
                return vec![answer.clone()];
            };
            if !question
                .get("multiple")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return vec![answer.clone()];
            }
            let options = question
                .get("options")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|option| option.get("label").and_then(Value::as_str))
                .collect::<Vec<_>>();
            split_open_code_multi_answer(answer, &options).unwrap_or_else(|| vec![answer.clone()])
        })
        .collect()
}

fn split_open_code_multi_answer(answer: &str, options: &[&str]) -> Option<Vec<String>> {
    if let Some(option) = options.iter().find(|option| **option == answer) {
        return Some(vec![(*option).to_string()]);
    }

    fn split_from(
        remaining: &str,
        options: &[&str],
        used: &mut [bool],
        selected: &mut Vec<String>,
    ) -> bool {
        for (index, option) in options.iter().enumerate() {
            if used[index] || !remaining.starts_with(option) {
                continue;
            }
            let suffix = &remaining[option.len()..];
            if !suffix.is_empty() && !suffix.starts_with(", ") {
                continue;
            }
            used[index] = true;
            selected.push((*option).to_string());
            if suffix.is_empty() || split_from(&suffix[2..], options, used, selected) {
                return true;
            }
            selected.pop();
            used[index] = false;
        }
        false
    }

    let mut used = vec![false; options.len()];
    let mut selected = Vec::new();
    split_from(answer, options, &mut used, &mut selected).then_some(selected)
}

fn open_code_proposed_plan(text: &str) -> Option<String> {
    const OPEN: &str = "<proposed_plan>";
    const CLOSE: &str = "</proposed_plan>";
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(after_open) = trimmed.find(OPEN).map(|index| index + OPEN.len()) {
        if let Some(close) = trimmed[after_open..]
            .find(CLOSE)
            .map(|index| after_open + index)
        {
            let plan = trimmed[after_open..close].trim();
            return (!plan.is_empty()).then(|| plan.to_string());
        }
        let plan = trimmed[after_open..].trim();
        return (!plan.is_empty()).then(|| plan.to_string());
    }
    Some(trimmed.to_string())
}

fn open_code_should_emit_plan(is_plan: bool, stop_reason: &str) -> bool {
    is_plan && !matches!(stop_reason, "refusal" | "cancelled")
}

fn open_code_tool_entry(update: &Value) -> WorkLogEntry {
    let id = update
        .get("toolCallId")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(next_request_id);
    let status = match update.get("status").and_then(Value::as_str) {
        Some("completed") => WorkLogStatus::Completed,
        Some("failed") => WorkLogStatus::Failed,
        Some("pending") => WorkLogStatus::Pending,
        _ => WorkLogStatus::InProgress,
    };
    let title = update
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("OpenCode tool")
        .to_string();
    let detail = update
        .get("rawInput")
        .or_else(|| update.get("rawOutput"))
        .filter(|value| !value.is_null())
        .and_then(|value| serde_json::to_string_pretty(value).ok());
    let kind = if update.get("kind").and_then(Value::as_str) == Some("execute") {
        WorkLogEntryKind::Command
    } else {
        WorkLogEntryKind::Tool
    };
    WorkLogEntry::new(id.clone(), id, kind, title, status)
        .detail(detail)
        .redact_sensitive()
}

fn project_relative_path(project_path: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(project_path)
        .unwrap_or(path)
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mutation_tools_receive_exact_path_attribution() {
        assert_eq!(
            open_code_path_attribution(Some("edit")),
            Some(OpenCodePathAttribution::Exact)
        );
        assert_eq!(
            open_code_path_attribution(Some("execute")),
            Some(OpenCodePathAttribution::Observed)
        );
        assert_eq!(open_code_path_attribution(Some("read")), None);
        assert_eq!(open_code_path_attribution(None), None);
    }

    #[test]
    fn execute_tools_are_rendered_as_commands() {
        let update = json!({
            "toolCallId": "command-1",
            "kind": "execute",
            "title": "Run command",
            "status": "completed",
            "rawInput": { "command": "npm test" }
        });

        let entry = open_code_tool_entry(&update);

        assert_eq!(entry.kind, WorkLogEntryKind::Command);
    }

    #[test]
    fn converts_open_code_questions_to_existing_user_input_model() {
        let pending = pending_user_input_from_open_code(&json!({
            "id": "que_123",
            "sessionID": "ses_123",
            "questions": [
                {
                    "header": "Page type",
                    "question": "What should we build?",
                    "options": [
                        { "label": "Dashboard", "description": "An admin dashboard" },
                        { "label": "Landing page", "description": "A marketing page" }
                    ]
                },
                {
                    "header": "Features",
                    "question": "Which features matter?",
                    "options": [
                        { "label": "Forms", "description": "Interactive forms" },
                        { "label": "Charts", "description": "Data visualizations" }
                    ],
                    "multiple": true
                }
            ]
        }))
        .expect("valid OpenCode question");

        assert_eq!(pending.request_id, "que_123");
        assert_eq!(pending.questions.len(), 2);
        assert_eq!(pending.questions[0].id, "que_123:0");
        assert!(!pending.questions[0].multi_select);
        assert_eq!(pending.questions[0].options[0].label, "Dashboard");
        assert_eq!(pending.questions[1].id, "que_123:1");
        assert!(pending.questions[1].multi_select);
    }

    #[test]
    fn ignores_empty_open_code_question_requests() {
        assert!(pending_user_input_from_open_code(&json!({
            "id": "que_123",
            "sessionID": "ses_123",
            "questions": []
        }))
        .is_none());
    }

    #[test]
    fn extracts_tagged_or_plain_open_code_plan() {
        assert_eq!(
            open_code_proposed_plan(
                "Before\n<proposed_plan>\n# Build it\n\n1. Add UI\n</proposed_plan>"
            ),
            Some("# Build it\n\n1. Add UI".to_string())
        );
        assert_eq!(
            open_code_proposed_plan("# Build it\n\n1. Add UI"),
            Some("# Build it\n\n1. Add UI".to_string())
        );
        assert_eq!(open_code_proposed_plan("  "), None);
    }

    #[test]
    fn extracts_an_unclosed_open_code_plan_without_exposing_the_tag() {
        assert_eq!(
            open_code_proposed_plan("Before\n<proposed_plan>\n# Partial plan\n\n1. Add UI"),
            Some("# Partial plan\n\n1. Add UI".to_string())
        );
    }

    #[test]
    fn does_not_emit_an_open_code_plan_after_cancellation_or_refusal() {
        assert!(open_code_should_emit_plan(true, "end_turn"));
        assert!(!open_code_should_emit_plan(true, "cancelled"));
        assert!(!open_code_should_emit_plan(true, "refusal"));
        assert!(!open_code_should_emit_plan(false, "end_turn"));
    }

    #[test]
    fn restores_open_code_multi_select_answers_as_separate_values() {
        let request = json!({
            "questions": [{
                "multiple": true,
                "options": [
                    { "label": "Forms" },
                    { "label": "Charts" },
                    { "label": "Exports, reports" }
                ]
            }]
        });

        assert_eq!(
            open_code_answer_groups(&request, &["Forms, Charts".to_string()]),
            vec![vec!["Forms".to_string(), "Charts".to_string()]]
        );
        assert_eq!(
            open_code_answer_groups(&request, &["Exports, reports".to_string()]),
            vec![vec!["Exports, reports".to_string()]]
        );
        assert_eq!(
            open_code_answer_groups(&request, &["Something custom".to_string()]),
            vec![vec!["Something custom".to_string()]]
        );
    }

    #[test]
    fn totals_complete_open_code_session_usage_without_double_counting() {
        let messages = vec![
            json!({
                "info": {
                    "id": "msg_user",
                    "role": "user"
                },
                "parts": []
            }),
            json!({
                "info": {
                    "id": "msg_assistant_1",
                    "role": "assistant",
                    "providerID": "anthropic",
                    "modelID": "claude-sonnet",
                    "cost": 0.12,
                    "tokens": {
                        "total": 2_060,
                        "input": 1_000,
                        "output": 200,
                        "reasoning": 50,
                        "cache": { "read": 800, "write": 10 }
                    }
                },
                "parts": []
            }),
            // A duplicate id must not inflate a resumed session snapshot.
            json!({
                "info": {
                    "id": "msg_assistant_1",
                    "role": "assistant",
                    "cost": 0.12,
                    "tokens": { "input": 1_000, "output": 200 }
                }
            }),
            // Older OpenCode builds may return the message info directly.
            json!({
                "id": "msg_assistant_2",
                "role": "assistant",
                "providerID": "openai",
                "modelID": "gpt-5",
                "cost": 0.03,
                "tokens": {
                    "total": 405,
                    "input": 300,
                    "output": 75,
                    "reasoning": 25,
                    "cache": { "read": 0, "write": 5 }
                }
            }),
        ];

        let usage = open_code_usage_from_messages("ses_123", &messages).expect("usage");
        assert_eq!(usage.session_id, "ses_123");
        assert_eq!(usage.totals.input_tokens, 1_300);
        assert_eq!(usage.totals.output_tokens, 275);
        assert_eq!(usage.totals.reasoning_tokens, 75);
        assert_eq!(usage.totals.cache_read_tokens, 800);
        assert_eq!(usage.totals.cache_write_tokens, 15);
        assert_eq!(usage.totals.total_tokens(), 2_465);
        assert!((usage.totals.cost_usd - 0.15).abs() < f64::EPSILON);
        assert_eq!(usage.latest_turn.as_ref().unwrap().input_tokens, 300);
        assert_eq!(usage.latest_turn.as_ref().unwrap().total_tokens(), 405);
        assert_eq!(usage.models.len(), 2);
    }
}
