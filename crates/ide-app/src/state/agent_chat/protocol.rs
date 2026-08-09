mod claude;
mod codex;
mod events;
mod open_code;
mod process;

use codex::capture_changed_files_snapshot;
use events::*;
use process::*;

pub(crate) fn find_opencode_executable() -> Option<PathBuf> {
    std::env::var_os("OPENCODE_CLI")
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| find_executable("opencode"))
}

pub(crate) fn find_agent_cli_executable(name: &str) -> Option<PathBuf> {
    if name == "codex" {
        find_codex_app_server_executable()
    } else {
        find_executable(name)
    }
}

pub(crate) fn agent_command_path_env() -> String {
    command_path_env()
}

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use crossbeam_channel::{Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context as _};
use ide_core::local_store::LocalStore;
use ide_core::{AgentAccessMode, AgentEffort, AgentKind, AgentModel, AgentRecord};
use serde_json::{json, Value};

use super::{
    AgentChatStatus, AgentInteractionMode, ChangedFilesSummary, CodeReview, ConversationUsage,
    FileChangeStat, ModelUsage, PendingApproval, PendingApprovalKind, PendingUserInput,
    PendingUserInputOption, PendingUserInputQuestion, ProposedPlan, UsageTotals, Verification,
    WorkLogEntry, WorkLogEntryKind, WorkLogStatus,
};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);
const CHAT_STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(28);
const CHAT_STREAM_MAX_BUFFER_BYTES: usize = 160;

/// Events flow to the GPUI foreground through an awaitable channel so the
/// per-chat consumer task sleeps until a backend actually produces something,
/// instead of polling on a timer.
pub(crate) type EventSender = async_channel::Sender<ChatBackendEvent>;

pub struct ChatBackendController {
    tx: Sender<ChatBackendCommand>,
    shutdown: Arc<AtomicBool>,
}

/// The next thing a backend run loop should react to. Commands are drained
/// with priority; otherwise the loop blocks on both channels at once and only
/// takes a timed tick while streamed text is waiting to be flushed.
enum BackendInbound {
    Command(ChatBackendCommand),
    CommandsClosed,
    Message(Value),
    MessagesClosed,
    FlushTick,
}

fn next_backend_inbound(
    commands: &Receiver<ChatBackendCommand>,
    messages: &Receiver<Value>,
    flush_pending: bool,
) -> BackendInbound {
    match commands.try_recv() {
        Ok(command) => return BackendInbound::Command(command),
        Err(crossbeam_channel::TryRecvError::Empty) => {}
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            return BackendInbound::CommandsClosed
        }
    }
    if flush_pending {
        crossbeam_channel::select! {
            recv(commands) -> command => match command {
                Ok(command) => BackendInbound::Command(command),
                Err(_) => BackendInbound::CommandsClosed,
            },
            recv(messages) -> message => match message {
                Ok(message) => BackendInbound::Message(message),
                Err(_) => BackendInbound::MessagesClosed,
            },
            default(CHAT_STREAM_FLUSH_INTERVAL) => BackendInbound::FlushTick,
        }
    } else {
        crossbeam_channel::select! {
            recv(commands) -> command => match command {
                Ok(command) => BackendInbound::Command(command),
                Err(_) => BackendInbound::CommandsClosed,
            },
            recv(messages) -> message => match message {
                Ok(message) => BackendInbound::Message(message),
                Err(_) => BackendInbound::MessagesClosed,
            },
        }
    }
}

pub enum ChatBackendCommand {
    Shutdown,
    CancelTurn,
    ForceShutdown,
    SendTurn {
        text: String,
        mode: AgentInteractionMode,
    },
    UpdateAccessMode {
        access_mode: AgentAccessMode,
    },
    UpdateModelEffort {
        model: AgentModel,
        effort: AgentEffort,
    },
    SubmitUserInput {
        request_id: String,
        answers: Vec<String>,
    },
    ResolveApproval {
        request_id: String,
        approved: bool,
    },
}

#[derive(Clone, Debug)]
pub enum ChatBackendEvent {
    ChatSessionReady {
        session_id: String,
    },
    SessionReady {
        session_id: String,
    },
    AssistantChunk {
        message_id: Option<String>,
        text: String,
    },
    ThoughtChunk {
        message_id: Option<String>,
        text: String,
    },
    WorkLog(WorkLogEntry),
    PendingUserInput(PendingUserInput),
    PendingApproval(super::PendingApproval),
    ProposedPlan(ProposedPlan),
    CodeReview(CodeReview),
    Verification(Verification),
    ChangedFiles(ChangedFilesSummary),
    Usage(ConversationUsage),
    Status(AgentChatStatus),
    Error(String),
}

impl ChatBackendController {
    pub fn send(
        &self,
        command: ChatBackendCommand,
    ) -> Result<(), crossbeam_channel::SendError<ChatBackendCommand>> {
        self.tx.send(command)
    }

    fn shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.tx.send(ChatBackendCommand::Shutdown);
    }

    pub fn cancel_turn(&self) {
        let _ = self.tx.send(ChatBackendCommand::CancelTurn);
    }

    pub fn force_shutdown(&self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.tx.send(ChatBackendCommand::ForceShutdown);
    }
}

impl Drop for ChatBackendController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

pub fn spawn_chat_backend(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
) -> anyhow::Result<(
    ChatBackendController,
    async_channel::Receiver<ChatBackendEvent>,
)> {
    if is_design_assistant(&agent) {
        if agent.provider == AgentKind::OpenCode {
            return Err(anyhow!(
                "The dedicated Design Assistant currently supports Codex and Claude"
            ));
        }
        choro_mcp_binary_path()
            .context("The Choro MCP server is unavailable for the Design Assistant")?;
        crate::state::penpot::configured_mcp_url()
            .context("The Design MCP connection is unavailable")?;
    }
    let (command_tx, command_rx) = crossbeam_channel::unbounded();
    let (event_tx, event_rx) = async_channel::unbounded();
    let shutdown = Arc::new(AtomicBool::new(false));
    match agent.provider {
        AgentKind::Codex => {
            spawn_codex_app_server(agent, initial_mode, command_rx, event_tx, shutdown.clone())?
        }
        AgentKind::Claude => {
            spawn_claude_bridge(agent, initial_mode, command_rx, event_tx, shutdown.clone())?
        }
        AgentKind::OpenCode => open_code::spawn_open_code_acp(
            agent,
            initial_mode,
            command_rx,
            event_tx,
            shutdown.clone(),
        )?,
    }
    Ok((
        ChatBackendController {
            tx: command_tx,
            shutdown,
        },
        event_rx,
    ))
}

fn spawn_claude_bridge(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    command_rx: Receiver<ChatBackendCommand>,
    event_tx: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    thread::Builder::new()
        .name("choro-claude-chat-bridge".into())
        .spawn(move || {
            if let Err(error) =
                run_claude_bridge(agent, initial_mode, command_rx, event_tx.clone(), shutdown)
            {
                let _ = event_tx.send_blocking(ChatBackendEvent::Error(format!(
                    "Claude chat bridge failed: {error:#}"
                )));
            }
        })?;
    Ok(())
}

fn spawn_codex_app_server(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    command_rx: Receiver<ChatBackendCommand>,
    event_tx: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    thread::Builder::new()
        .name("choro-codex-app-server".into())
        .spawn(move || {
            if let Err(error) =
                run_codex_app_server(agent, initial_mode, command_rx, event_tx.clone(), shutdown)
            {
                let _ = event_tx.send_blocking(ChatBackendEvent::Error(format!(
                    "Codex app-server failed: {error:#}"
                )));
            }
        })?;
    Ok(())
}

struct CodexRuntime {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    messages: Receiver<Value>,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    agent: AgentRecord,
    thread_id: Option<String>,
    assistant_buffer: String,
    assistant_stream: StreamChunkBuffer,
    plan_buffer: String,
    pending_changed_files: Option<ChangedFilesSummary>,
    pending_user_inputs: std::collections::HashMap<String, PendingRequest>,
    pending_approvals: std::collections::HashMap<String, PendingApprovalRequest>,
    deferred_turns: VecDeque<(String, AgentInteractionMode)>,
    active_reconnect_work_log_id: Option<String>,
    model: Option<String>,
    effort: String,
    access_mode: AgentAccessMode,
    visualization_dir: Option<PathBuf>,
}

struct PendingRequest {
    jsonrpc_id: Value,
    question_ids: Vec<String>,
}

struct PendingApprovalRequest {
    jsonrpc_id: Value,
    response_kind: PendingApprovalResponseKind,
}

enum PendingApprovalResponseKind {
    Decision,
    Permissions(Value),
}

struct ClaudeBridgeRuntime {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    messages: Receiver<Value>,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    agent: AgentRecord,
    model: Option<String>,
    effort: String,
    access_mode: AgentAccessMode,
    claude_path: PathBuf,
    assistant_buffer: String,
    assistant_stream: StreamChunkBuffer,
}

struct StreamChunkBuffer {
    message_id: Option<String>,
    generated_message_id: Option<String>,
    text: String,
    last_flush: Instant,
}

impl StreamChunkBuffer {
    fn new() -> Self {
        Self {
            message_id: None,
            generated_message_id: None,
            text: String::new(),
            last_flush: Instant::now(),
        }
    }

    fn reset(&mut self, events: &EventSender) {
        self.flush(events);
        self.message_id = None;
        self.generated_message_id = None;
    }

    fn push(&mut self, message_id: Option<String>, text: &str, events: &EventSender) {
        if text.is_empty() {
            return;
        }

        let message_id = message_id.unwrap_or_else(|| {
            self.generated_message_id
                .get_or_insert_with(|| format!("generated-{}", next_request_id()))
                .clone()
        });

        if !self.text.is_empty() && self.message_id.as_ref() != Some(&message_id) {
            self.flush(events);
        }

        if self.text.is_empty() {
            self.message_id = Some(message_id);
            self.last_flush = Instant::now();
        }

        self.text.push_str(text);

        if self.text.len() >= CHAT_STREAM_MAX_BUFFER_BYTES
            || self.text.ends_with('\n')
            || self.last_flush.elapsed() >= CHAT_STREAM_FLUSH_INTERVAL
        {
            self.flush(events);
        }
    }

    fn flush_due(&mut self, events: &EventSender) {
        if !self.text.is_empty() && self.last_flush.elapsed() >= CHAT_STREAM_FLUSH_INTERVAL {
            self.flush(events);
        }
    }

    /// Whether buffered stream text is waiting on a timed flush. While false,
    /// run loops can block on their channels instead of ticking.
    fn has_pending(&self) -> bool {
        !self.text.is_empty()
    }

    fn flush(&mut self, events: &EventSender) {
        if self.text.is_empty() {
            return;
        }

        events
            .send_blocking(ChatBackendEvent::AssistantChunk {
                message_id: self.message_id.clone(),
                text: std::mem::take(&mut self.text),
            })
            .ok();
        self.last_flush = Instant::now();
    }
}

fn run_claude_bridge(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    command_rx: Receiver<ChatBackendCommand>,
    event_tx: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let bridge_path = claude_bridge_script_path()?;
    let bridge_dir = bridge_path
        .parent()
        .context("Claude bridge script has no parent directory")?;
    let node_path = find_executable("node").ok_or_else(|| {
        anyhow!("node executable was not found. Install Node.js or add it to a standard location.")
    })?;
    let npm_path = find_executable("npm").ok_or_else(|| {
        anyhow!("npm executable was not found. Install Node.js or add it to a standard location.")
    })?;
    let claude_path = find_executable("claude").ok_or_else(|| {
        anyhow!("Claude Code executable was not found. Install Claude Code or add it to a standard location.")
    })?;
    ensure_claude_bridge_dependencies(bridge_dir, &npm_path, &event_tx)?;

    let mut command = Command::new(&node_path);
    command
        .arg(&bridge_path)
        .env("PATH", command_path_env())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(agent.runtime_path());
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().context("failed to spawn Claude bridge")?;
    let stdin = Arc::new(Mutex::new(
        child
            .stdin
            .take()
            .context("Claude bridge stdin unavailable")?,
    ));
    let stdout = child
        .stdout
        .take()
        .context("Claude bridge stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("Claude bridge stderr unavailable")?;

    let (message_tx, message_rx) = crossbeam_channel::unbounded();
    spawn_json_reader(stdout, message_tx);
    spawn_stderr_reader(stderr, event_tx.clone(), "Claude bridge");

    let mut runtime = ClaudeBridgeRuntime {
        child,
        stdin,
        messages: message_rx,
        commands: command_rx,
        events: event_tx,
        shutdown,
        model: agent.model.cli_value().map(str::to_string),
        effort: agent.effort.cli_value().to_string(),
        access_mode: agent.access_mode,
        agent,
        claude_path,
        assistant_buffer: String::new(),
        assistant_stream: StreamChunkBuffer::new(),
    };

    if runtime.agent.cli_session_id.is_none()
        && !runtime.agent.hidden_doc_assistant
        && !runtime.agent.doc.trim().is_empty()
    {
        runtime.send_turn(runtime.agent.doc.clone(), initial_mode)?;
    }
    runtime.run_loop()
}

/// Absolute path to our bundled `choro-mcp` server binary, if it sits next to the
/// running executable (dev: `target/debug`; macOS bundle: `Contents/MacOS`).
/// Returns `None` when it isn't found so agents still launch without the tool.
fn choro_mcp_binary_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let binary_dir = exe.parent()?;
    let current = binary_dir.join("choro-mcp");
    if current.exists() {
        return Some(current);
    }
    // Compatibility with development bundles created before the rename.
    let legacy = binary_dir.join("ide-mcp");
    legacy.exists().then_some(legacy)
}

fn penpot_http_mcp_server(url: &str) -> Value {
    json!({
        "type": "http",
        "url": url,
    })
}

fn penpot_acp_mcp_server(url: &str) -> Value {
    json!({
        "type": "http",
        "name": "penpot",
        "url": url,
        "headers": [],
    })
}

fn codex_mcp_url_config_arg(name: &str, url: &str) -> String {
    format!(
        "mcp_servers.{name}.url={}",
        serde_json::to_string(url).unwrap_or_else(|_| "\"\"".to_string())
    )
}

fn codex_penpot_config_arg(url: &str) -> String {
    codex_mcp_url_config_arg("penpot", url)
}

fn codex_penpot_approval_config_arg(name: &str) -> String {
    // The dedicated assistant is already file-bound and can only write through
    // this isolated Design MCP server. Without this server-level override,
    // `approvalPolicy=never` turns every Design tool invocation into
    // "user rejected MCP tool call" before Choro can run it.
    format!("mcp_servers.{name}.default_tools_approval_mode=\"approve\"")
}

fn is_design_assistant(agent: &AgentRecord) -> bool {
    agent.design_context.is_some()
}

fn agent_requires_design_mcp(agent: &AgentRecord) -> bool {
    agent.design_context.is_some() || ide_core::penpot_assistant::requires_design_mcp(&agent.doc)
}

fn configured_codex_mcp_names(
    codex_path: &Path,
    cwd: &Path,
    path_env: &str,
) -> anyhow::Result<Vec<String>> {
    let output = Command::new(codex_path)
        .args([
            "-c",
            "features.plugins=false",
            "-c",
            "features.apps=false",
            "mcp",
            "list",
            "--json",
        ])
        .env("PATH", path_env)
        .current_dir(cwd)
        .output()
        .context("failed to inspect the resolved Codex MCP configuration")?;
    if !output.status.success() {
        return Err(anyhow!(
            "Codex could not isolate the Design Assistant MCP configuration"
        ));
    }
    let entries: Value = serde_json::from_slice(&output.stdout)
        .context("Codex returned an invalid MCP configuration")?;
    let entries = entries
        .as_array()
        .context("Codex returned an invalid MCP server list")?;
    let mut names = Vec::new();
    for entry in entries {
        if !entry
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            continue;
        }
        let Some(name) = entry.get("name").and_then(Value::as_str) else {
            continue;
        };
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(anyhow!(
                "Codex has an MCP server name that cannot be isolated safely"
            ));
        }
        names.push(name.to_string());
    }
    Ok(names)
}

fn configure_codex_design_assistant(
    command: &mut Command,
    codex_path: &Path,
    agent: &AgentRecord,
    path_env: &str,
) -> anyhow::Result<()> {
    let mcp = choro_mcp_binary_path()
        .context("The Choro MCP server is unavailable for the Design Assistant")?;
    let design_url = crate::state::penpot::configured_mcp_url()
        .context("The Design MCP connection is not configured")?;
    let inherited_names = configured_codex_mcp_names(codex_path, agent.runtime_path(), path_env)?;
    append_codex_design_assistant_config(command, &mcp, agent, &design_url, inherited_names);
    Ok(())
}

fn append_codex_design_assistant_config(
    command: &mut Command,
    mcp: &Path,
    agent: &AgentRecord,
    design_url: &str,
    inherited_names: impl IntoIterator<Item = String>,
) {
    let suffix = agent.id.simple();
    let choro_name = format!("choro_design_{suffix}");
    let penpot_name = format!("penpot_design_{suffix}");

    command
        .arg("-c")
        .arg("features.plugins=false")
        .arg("-c")
        .arg("features.apps=false")
        .arg("-c")
        .arg("agents.enabled=false")
        .arg("-c")
        .arg("tools.web_search=false")
        .arg("-c")
        .arg("tools.view_image=false");
    for name in inherited_names {
        command
            .arg("-c")
            .arg(format!("mcp_servers.{name}.enabled=false"));
    }
    command
        .arg("-c")
        .arg(format!(
            "mcp_servers.{choro_name}.command={}",
            mcp.display()
        ))
        .arg("-c")
        .arg(format!(
            "mcp_servers.{choro_name}.args=[\"--project-id\", \"{}\", \"--agent-id\", \"{}\"]",
            agent.project_id.0, agent.id
        ))
        .arg("-c")
        .arg(format!(
            "mcp_servers.{choro_name}.enabled_tools=[\"task_read\",\"task_list\",\"task_image\"]"
        ))
        .arg("-c")
        .arg(format!("mcp_servers.{choro_name}.enabled=true"))
        .arg("-c")
        .arg(format!("mcp_servers.{choro_name}.required=true"))
        .arg("-c")
        .arg(codex_mcp_url_config_arg(&penpot_name, &design_url))
        .arg("-c")
        .arg(format!("mcp_servers.{penpot_name}.enabled=true"))
        .arg("-c")
        .arg(codex_penpot_approval_config_arg(&penpot_name))
        .arg("-c")
        .arg(format!("mcp_servers.{penpot_name}.required=true"));
}

/// The `mcpServers` object handed to the Claude Agent SDK, scoped to this Choro
/// chat and project. Design access is capability-based: only the dedicated
/// Design Assistant or an agent carrying an explicit Choro design reference
/// receives the Design MCP server.
fn choro_mcp_servers_json(agent: &AgentRecord) -> Value {
    let mut servers = serde_json::Map::new();
    if let Some(mcp) = choro_mcp_binary_path() {
        servers.insert(
            "choro".to_string(),
            json!({
                "type": "stdio",
                "command": mcp.display().to_string(),
                "args": [
                    "--project-id",
                    agent.project_id.0.to_string(),
                    "--agent-id",
                    agent.id.to_string()
                ],
            }),
        );
    }
    if agent_requires_design_mcp(agent) {
        if let Some(url) = crate::state::penpot::configured_mcp_url() {
            servers.insert("penpot".to_string(), penpot_http_mcp_server(&url));
        }
    }
    if servers.is_empty() {
        Value::Null
    } else {
        Value::Object(servers)
    }
}

/// The provider-neutral stdio MCP entry used by ACP backends. ACP models this
/// as a list (rather than the named object used by Claude), but it launches the
/// same project- and chat-scoped Choro server.
fn choro_acp_mcp_servers_json(agent: &AgentRecord) -> Value {
    let mut servers = match choro_mcp_binary_path() {
        Some(mcp) => choro_acp_mcp_servers_json_at(
            &mcp,
            &agent.project_id.0.to_string(),
            &agent.id.to_string(),
        )
        .as_array()
        .cloned()
        .unwrap_or_default(),
        None => Vec::new(),
    };
    if agent_requires_design_mcp(agent) {
        if let Some(url) = crate::state::penpot::configured_mcp_url() {
            servers.push(penpot_acp_mcp_server(&url));
        }
    }
    Value::Array(servers)
}

fn choro_acp_mcp_servers_json_at(mcp: &Path, project_id: &str, agent_id: &str) -> Value {
    json!([{
        "name": "choro",
        "command": mcp.display().to_string(),
        "args": [
            "--project-id",
            project_id,
            "--agent-id",
            agent_id,
        ],
        "env": [],
    }])
}

fn claude_bridge_script_path() -> anyhow::Result<PathBuf> {
    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    let bundle_resource = current_exe
        .parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Resources/agent-chat/claude_bridge.mjs"))
        .filter(|path| path.exists());
    if let Some(path) = bundle_resource {
        return Ok(path);
    }

    let repo_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/agent-chat/claude_bridge.mjs");
    if repo_path.exists() {
        return Ok(repo_path);
    }

    Err(anyhow!("Claude bridge script was not found"))
}

fn ensure_claude_bridge_dependencies(
    bridge_dir: &Path,
    npm_path: &Path,
    event_tx: &EventSender,
) -> anyhow::Result<()> {
    if bridge_dir
        .join("node_modules/@anthropic-ai/claude-agent-sdk")
        .exists()
    {
        return Ok(());
    }

    event_tx
        .send_blocking(ChatBackendEvent::WorkLog(
            WorkLogEntry::new(
                "claude-bridge-install",
                "claude-bridge-install",
                WorkLogEntryKind::System,
                "Installing Claude Agent SDK bridge dependency",
                WorkLogStatus::InProgress,
            )
            .detail(Some(bridge_dir.display().to_string())),
        ))
        .ok();

    let output = Command::new(npm_path)
        .args(["install", "--omit=dev", "--no-audit", "--no-fund"])
        .env("PATH", command_path_env())
        .current_dir(bridge_dir)
        .output()
        .context("failed to run npm install for Claude bridge")?;
    if output.status.success() {
        event_tx
            .send_blocking(ChatBackendEvent::WorkLog(WorkLogEntry::new(
                "claude-bridge-install",
                "claude-bridge-install",
                WorkLogEntryKind::System,
                "Installed Claude Agent SDK bridge dependency",
                WorkLogStatus::Completed,
            )))
            .ok();
        Ok(())
    } else {
        Err(anyhow!(
            "npm install failed for Claude bridge: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn run_codex_app_server(
    agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    command_rx: Receiver<ChatBackendCommand>,
    event_tx: EventSender,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let path_env = command_path_env();
    let codex_path = find_codex_app_server_executable().ok_or_else(|| {
        anyhow!("Codex executable was not found. Install Codex or add it to your shell PATH.")
    })?;
    let mut command = Command::new(&codex_path);
    command
        .args(["app-server", "--stdio"])
        .env("PATH", &path_env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(agent.runtime_path());
    if is_design_assistant(&agent) {
        // Codex `-c` values merge with the user's config. A dedicated Design
        // Assistant must therefore disable every inherited explicit MCP server,
        // disable plugin/app MCP contributions, and require only Choro + the
        // exact Design connection. If isolation cannot be proven, fail closed.
        configure_codex_design_assistant(&mut command, &codex_path, &agent, &path_env)?;
    } else {
        if let Some(mcp) = choro_mcp_binary_path() {
            command
                .arg("-c")
                .arg(format!("mcp_servers.ide.command={}", mcp.display()));
            command.arg("-c").arg(format!(
                "mcp_servers.ide.args=[\"--project-id\", \"{}\", \"--agent-id\", \"{}\"]",
                agent.project_id.0, agent.id
            ));
        }
        if agent_requires_design_mcp(&agent) {
            if let Some(url) = crate::state::penpot::configured_mcp_url() {
                command.arg("-c").arg(codex_penpot_config_arg(&url));
            }
        }
    }
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().with_context(|| {
        format!(
            "failed to spawn codex app-server at {} from cwd {}",
            codex_path.display(),
            agent.runtime_path().display()
        )
    })?;

    let stdin = Arc::new(Mutex::new(
        child
            .stdin
            .take()
            .context("codex app-server stdin unavailable")?,
    ));
    let stdout = child
        .stdout
        .take()
        .context("codex app-server stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("codex app-server stderr unavailable")?;

    let (message_tx, message_rx) = crossbeam_channel::unbounded();
    spawn_json_reader(stdout, message_tx);
    spawn_stderr_reader(stderr, event_tx.clone(), "Codex app-server");

    let visualization_dir = LocalStore::open_default().ok().and_then(|store| {
        let path = store.agent_artifacts_dir(agent.id).join("visualizations");
        fs::create_dir_all(&path).ok().map(|_| path)
    });

    let mut runtime = CodexRuntime {
        child,
        stdin,
        messages: message_rx,
        commands: command_rx,
        events: event_tx,
        shutdown,
        agent: agent.clone(),
        thread_id: None,
        assistant_buffer: String::new(),
        assistant_stream: StreamChunkBuffer::new(),
        plan_buffer: String::new(),
        pending_changed_files: None,
        pending_user_inputs: std::collections::HashMap::new(),
        pending_approvals: std::collections::HashMap::new(),
        deferred_turns: VecDeque::new(),
        active_reconnect_work_log_id: None,
        model: agent.model.cli_value().map(str::to_string),
        effort: agent.effort.cli_value().to_string(),
        access_mode: agent.access_mode,
        visualization_dir,
    };

    runtime.request(
        "initialize",
        json!({
            "clientInfo": {
                "name": "choro",
                "title": "Choro",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "experimentalApi": true
            }
        }),
    )?;
    runtime.notify("initialized", Value::Null)?;
    let design_assistant = is_design_assistant(&agent);
    let approval_policy = if design_assistant {
        "never"
    } else {
        runtime.access_mode.codex_approval_policy()
    };
    let sandbox = if design_assistant {
        "read-only"
    } else {
        runtime.access_mode.codex_sandbox()
    };
    let start_params = json!({
        "cwd": agent.runtime_path(),
        "approvalPolicy": approval_policy,
        "sandbox": sandbox,
        "model": runtime.model,
    });
    let existing_thread_id = agent
        .chat_session_id
        .as_deref()
        .or(agent.cli_session_id.as_deref());
    let is_resuming_existing_thread = existing_thread_id.is_some();
    let thread = if let Some(thread_id) = existing_thread_id {
        match runtime.request(
            "thread/resume",
            json!({
                "threadId": thread_id,
                "cwd": agent.runtime_path(),
                "approvalPolicy": approval_policy,
                "sandbox": sandbox,
                "model": runtime.model,
            }),
        ) {
            Ok(thread) => thread,
            Err(error) => {
                let _ = runtime
                    .events
                    .send_blocking(ChatBackendEvent::WorkLog(WorkLogEntry::new(
                        "codex-resume-fallback",
                        "codex-resume-fallback",
                        WorkLogEntryKind::System,
                        format!("Codex resume failed; started a new backend thread: {error:#}"),
                        WorkLogStatus::Failed,
                    )));
                return Err(error).context(
                    "Codex resume failed; refusing to start a new backend thread for an existing chat",
                );
            }
        }
    } else {
        runtime.request("thread/start", start_params)?
    };
    runtime.thread_id = thread
        .get("thread")
        .and_then(|thread| thread.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(thread_id) = runtime.thread_id.clone() {
        runtime
            .events
            .send_blocking(ChatBackendEvent::ChatSessionReady {
                session_id: thread_id,
            })
            .ok();
    }
    if !is_resuming_existing_thread && !agent.hidden_doc_assistant && !agent.doc.trim().is_empty() {
        runtime.send_turn(agent.doc.clone(), initial_mode)?;
    }

    runtime.run_loop()
}

fn spawn_json_reader(stdout: impl std::io::Read + Send + 'static, tx: Sender<Value>) {
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                let _ = tx.send(value);
            }
        }
    });
}

fn spawn_stderr_reader(
    stderr: impl std::io::Read + Send + 'static,
    event_tx: EventSender,
    label: &'static str,
) {
    thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let safe_detail = ide_core::redact_sensitive_text(trimmed);
            eprintln!("{label}: {safe_detail}");
            if should_surface_stderr(trimmed) {
                let _ = event_tx.send_blocking(ChatBackendEvent::WorkLog(
                    WorkLogEntry::new(
                        next_request_id(),
                        format!("{label}-stderr"),
                        WorkLogEntryKind::System,
                        format!("{label} reported output on stderr"),
                        WorkLogStatus::Failed,
                    )
                    .detail(Some(safe_detail)),
                ));
            }
        }
    });
}

fn should_surface_stderr(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("\"level\":\"ERROR\"")
        || lower.contains("error")
        || lower.contains("failed")
        || lower.contains("exception")
        || lower.contains("permission denied")
        || lower.contains("enoent")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_id_key_preserves_numeric_request_ids() {
        assert_eq!(jsonrpc_id_key(&json!(77)), "77");
        assert_eq!(jsonrpc_id_key(&json!("abc")), "abc");
    }

    #[test]
    fn design_mcp_capability_is_not_granted_by_generic_design_language() {
        let agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            PathBuf::from("/tmp/project"),
            "Generic agent",
            "Please implement the design.",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::default(),
        );
        assert!(!agent_requires_design_mcp(&agent));

        let mut linked = agent.clone();
        linked
            .doc
            .push_str("\n<choro-penpot-design local-id=\"design\" file-id=\"file\" />");
        assert!(agent_requires_design_mcp(&linked));

        let mut dedicated = agent;
        dedicated.design_context = Some(ide_core::AgentDesignContext {
            design_id: uuid::Uuid::new_v4(),
            file_id: uuid::Uuid::new_v4(),
        });
        assert!(is_design_assistant(&dedicated));
        assert!(agent_requires_design_mcp(&dedicated));
    }

    #[test]
    fn dedicated_design_mcp_tools_are_preapproved() {
        assert_eq!(
            codex_penpot_approval_config_arg("penpot_design_123"),
            "mcp_servers.penpot_design_123.default_tools_approval_mode=\"approve\""
        );
    }

    #[test]
    fn dedicated_codex_design_session_disables_inherited_mcp_servers() {
        let mut agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            PathBuf::from("/tmp/project"),
            "Design Assistant",
            "Dedicated design session",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::default(),
        );
        agent.id = uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000123").unwrap();
        agent.design_context = Some(ide_core::AgentDesignContext {
            design_id: uuid::Uuid::new_v4(),
            file_id: uuid::Uuid::new_v4(),
        });
        let mut command = Command::new("codex");

        append_codex_design_assistant_config(
            &mut command,
            Path::new("/Applications/Choro.app/choro-mcp"),
            &agent,
            "https://design.example/mcp/stream?userToken=secret",
            ["choro".to_string(), "github".to_string()],
        );

        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(args.contains("mcp_servers.choro.enabled=false"));
        assert!(args.contains("mcp_servers.github.enabled=false"));
        assert!(args
            .contains("mcp_servers.choro_design_00000000000000000000000000000123.required=true"));
        assert!(args
            .contains("mcp_servers.penpot_design_00000000000000000000000000000123.required=true"));
        assert!(!args.contains("mcp_servers.penpot.url="));
    }

    #[test]
    fn backend_inbound_prefers_commands_over_messages() {
        let (command_tx, command_rx) = crossbeam_channel::unbounded();
        let (message_tx, message_rx) = crossbeam_channel::unbounded();
        message_tx.send(json!({"type": "noise"})).unwrap();
        command_tx.send(ChatBackendCommand::CancelTurn).unwrap();

        match next_backend_inbound(&command_rx, &message_rx, false) {
            BackendInbound::Command(ChatBackendCommand::CancelTurn) => {}
            _ => panic!("queued command should win over a queued message"),
        }
        match next_backend_inbound(&command_rx, &message_rx, false) {
            BackendInbound::Message(message) => {
                assert_eq!(message.get("type").and_then(Value::as_str), Some("noise"));
            }
            _ => panic!("message should be delivered once commands are drained"),
        }
    }

    #[test]
    fn backend_inbound_reports_closed_channels() {
        let (command_tx, command_rx) = crossbeam_channel::unbounded::<ChatBackendCommand>();
        let (message_tx, message_rx) = crossbeam_channel::unbounded::<Value>();

        drop(message_tx);
        match next_backend_inbound(&command_rx, &message_rx, false) {
            BackendInbound::MessagesClosed => {}
            _ => panic!("closed message channel must surface, not block"),
        }

        drop(command_tx);
        match next_backend_inbound(&command_rx, &message_rx, false) {
            BackendInbound::CommandsClosed => {}
            _ => panic!("closed command channel must surface, not block"),
        }
    }

    #[test]
    fn backend_inbound_ticks_only_while_a_flush_is_pending() {
        let (_command_tx, command_rx) = crossbeam_channel::unbounded::<ChatBackendCommand>();
        let (_message_tx, message_rx) = crossbeam_channel::unbounded::<Value>();

        let start = Instant::now();
        match next_backend_inbound(&command_rx, &message_rx, true) {
            BackendInbound::FlushTick => {}
            _ => panic!("pending flush must produce a timed tick"),
        }
        assert!(start.elapsed() >= CHAT_STREAM_FLUSH_INTERVAL);
    }

    #[test]
    fn stream_chunk_buffer_flushes_exact_text_batch() {
        let (tx, rx) = async_channel::unbounded();
        let mut buffer = StreamChunkBuffer::new();

        buffer.push(Some("msg-1".into()), "hel", &tx);
        buffer.push(Some("msg-1".into()), "lo ", &tx);
        assert!(rx.try_recv().is_err());

        buffer.flush(&tx);
        match rx.recv_blocking().expect("buffer should flush one event") {
            ChatBackendEvent::AssistantChunk { message_id, text } => {
                assert_eq!(message_id.as_deref(), Some("msg-1"));
                assert_eq!(text, "hello ");
            }
            _ => panic!("expected assistant chunk"),
        }
    }

    #[test]
    fn stream_chunk_buffer_flushes_before_switching_message_ids() {
        let (tx, rx) = async_channel::unbounded();
        let mut buffer = StreamChunkBuffer::new();

        buffer.push(Some("msg-1".into()), "first", &tx);
        buffer.push(Some("msg-2".into()), "second", &tx);
        buffer.flush(&tx);

        let first = rx
            .recv_blocking()
            .expect("first message should flush on id switch");
        let second = rx
            .recv_blocking()
            .expect("second message should flush explicitly");

        match first {
            ChatBackendEvent::AssistantChunk { message_id, text } => {
                assert_eq!(message_id.as_deref(), Some("msg-1"));
                assert_eq!(text, "first");
            }
            _ => panic!("expected first assistant chunk"),
        }
        match second {
            ChatBackendEvent::AssistantChunk { message_id, text } => {
                assert_eq!(message_id.as_deref(), Some("msg-2"));
                assert_eq!(text, "second");
            }
            _ => panic!("expected second assistant chunk"),
        }
    }

    #[test]
    fn stream_chunk_buffer_generates_ids_for_anonymous_chunks_per_turn() {
        let (tx, rx) = async_channel::unbounded();
        let mut buffer = StreamChunkBuffer::new();

        buffer.push(None, "first ", &tx);
        buffer.push(None, "turn", &tx);
        buffer.flush(&tx);
        let first_id = match rx.recv_blocking().expect("anonymous chunks should flush") {
            ChatBackendEvent::AssistantChunk { message_id, text } => {
                assert_eq!(text, "first turn");
                message_id.expect("anonymous chunks should receive a generated id")
            }
            _ => panic!("expected assistant chunk"),
        };

        buffer.reset(&tx);
        buffer.push(None, "second turn", &tx);
        buffer.flush(&tx);
        let second_id = match rx.recv_blocking().expect("next turn should flush") {
            ChatBackendEvent::AssistantChunk { message_id, text } => {
                assert_eq!(text, "second turn");
                message_id.expect("anonymous chunks should receive a generated id")
            }
            _ => panic!("expected assistant chunk"),
        };

        assert_ne!(first_id, second_id);
    }

    #[test]
    fn open_code_acp_receives_the_provider_neutral_choro_mcp_server() {
        let servers = choro_acp_mcp_servers_json_at(
            Path::new("/Applications/Choro.app/Contents/MacOS/choro-mcp"),
            "project-123",
            "agent-456",
        );

        assert_eq!(
            servers,
            json!([{
                "name": "choro",
                "command": "/Applications/Choro.app/Contents/MacOS/choro-mcp",
                "args": [
                    "--project-id",
                    "project-123",
                    "--agent-id",
                    "agent-456",
                ],
                "env": [],
            }])
        );
    }

    #[test]
    fn every_chat_backend_receives_the_configured_penpot_mcp_server_shape() {
        let url = "https://design.penpot.app/mcp/stream?userToken=redacted";

        assert_eq!(
            penpot_http_mcp_server(url),
            json!({
                "type": "http",
                "url": url,
            })
        );
        assert_eq!(
            penpot_acp_mcp_server(url),
            json!({
                "type": "http",
                "name": "penpot",
                "url": url,
                "headers": [],
            })
        );
        assert_eq!(
            codex_penpot_config_arg(url),
            format!(
                "mcp_servers.penpot.url={}",
                serde_json::to_string(url).unwrap()
            )
        );
    }
}
