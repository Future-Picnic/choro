mod change_tracking;
mod claude;
mod codex;
mod events;
pub(crate) mod gemini;
pub(crate) mod managed;
mod open_code;
mod process;
pub(super) mod review;
#[cfg(test)]
mod worktree_changes;

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
use ide_core::{
    prompt_with_connected_context, AgentAccessMode, AgentConnectedContextExtras, AgentEffort,
    AgentKind, AgentModel, AgentRecord, AppConfig,
};
use serde_json::{json, Value};

use super::{
    unix_now, AgentChatStatus, AgentInteractionMode, ChangedFilesSummary, CodeReview,
    ConversationUsage, FileChangeActivity, FileChangeStat, ModelUsage, PendingApproval,
    PendingApprovalKind, PendingUserInput, PendingUserInputOption, PendingUserInputQuestion,
    ProposedPlan, ReviewChecklist, UsageTotals, Verification, WorkLogEntry, WorkLogEntryKind,
    WorkLogStatus,
};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);
const CHAT_STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(28);
const CHAT_STREAM_MAX_BUFFER_BYTES: usize = 160;
const CHORO_NATIVE_TOOL_INSTRUCTIONS: &str = "When the user explicitly asks for a Choro doc, a document in Choro Docs, or equivalent wording, use the Choro MCP create_choro_doc tool instead of writing a .choro file directly. When the user explicitly asks for a Choro script or a command in Choro's top-header Scripts control, use create_choro_script. Do not use these tools for ordinary repository documents, files, scripts, or commands.";

fn append_choro_visualization_instructions(
    instructions: String,
    visualization_dir: Option<&Path>,
) -> String {
    let instructions = format!(
        "{instructions}\n\n{}",
        ide_core::agent_changes::AGENT_CHANGE_INSTRUCTIONS
    );
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

fn agent_visualization_dir(agent: &AgentRecord) -> Option<PathBuf> {
    if agent.review_run_id.is_some() { return None; }
    LocalStore::open_default().ok().and_then(|store| {
        let path = store.agent_artifacts_dir(agent.id).join("visualizations");
        fs::create_dir_all(&path).ok().map(|_| path)
    })
}

/// Events flow to the GPUI foreground through an awaitable channel so the
/// per-chat consumer task sleeps until a backend actually produces something,
/// instead of polling on a timer.
#[derive(Clone)]
pub(crate) struct EventSender {
    tx: async_channel::Sender<ChatBackendEvent>,
    overflow: Arc<AtomicBool>,
    initial_turn_id: String,
}
impl From<async_channel::Sender<ChatBackendEvent>> for EventSender {
    fn from(tx: async_channel::Sender<ChatBackendEvent>) -> Self {
        Self {
            tx,
            overflow: Arc::new(AtomicBool::new(false)),
            initial_turn_id: next_request_id(),
        }
    }
}
impl EventSender {
    fn mark_evidence_overflow(&self) {
        if !self.overflow.swap(true, Ordering::Relaxed) {
            let _ = self.tx.send_blocking(ChatBackendEvent::EvidenceOverflow);
        }
    }
    fn send_blocking(
        &self,
        event: ChatBackendEvent,
    ) -> Result<(), async_channel::SendError<ChatBackendEvent>> {
        let bytes = match &event {
            ChatBackendEvent::FileChangeActivity(activity) => Some(
                change_tracking::file_payload_bytes(&activity.file)
                    + activity.id.len()
                    + activity.turn_id.len(),
            ),
            ChatBackendEvent::ChangeReceiptReady { summary, .. } => Some(
                summary
                    .files
                    .iter()
                    .chain(&summary.observed_files)
                    .map(change_tracking::file_payload_bytes)
                    .sum::<usize>()
                    + 512,
            ),
            _ => None,
        };
        if let Some(bytes) = bytes {
            let Some(permit) = ide_core::agent_changes::EvidenceBudget::global().reserve(bytes)
            else {
                self.mark_evidence_overflow();
                if let ChatBackendEvent::ChangeReceiptReady { summary, .. } = event {
                    return self.tx.send_blocking(ChatBackendEvent::ChangeReceiptReady {
                        summary: ChangedFilesSummary::attributed(
                            summary.turn_id.unwrap_or_default(),
                            vec![],
                            vec![],
                        ),
                        state: ide_core::agent_changes::ChangeReceiptState::Partial,
                    });
                }
                return Ok(());
            };
            return self.tx.send_blocking(ChatBackendEvent::ReservedEvidence {
                event: Box::new(event),
                reservation: Arc::new(permit),
            });
        }
        self.tx.send_blocking(event)
    }
}

fn event_channel() -> (EventSender, async_channel::Receiver<ChatBackendEvent>) {
    let (tx, rx) = async_channel::unbounded();
    (tx.into(), rx)
}

#[cfg(test)]
fn unpack_event(event: ChatBackendEvent) -> ChatBackendEvent {
    match event {
        ChatBackendEvent::ReservedEvidence { event, .. } => *event,
        event => event,
    }
}

pub struct ChatBackendController {
    timing: Arc<ide_core::agent_changes::DispatchTiming>,
    studio_scope: Option<(PathBuf, uuid::Uuid)>,
    tx: Sender<ChatBackendCommand>,
    shutdown: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
}

#[derive(Clone)]
pub(crate) struct ChatBackendStopSignal {
    stopped: Arc<AtomicBool>,
}

impl ChatBackendStopSignal {
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
    }
}

// Keep this wait on the backend worker, never on the UI thread. Even if this
// replacement is stopped too, its stop signal must cover its predecessor so
// a third backend cannot overtake cleanup and resume the same provider thread.
fn wait_for_previous_backend(
    previous: Option<ChatBackendStopSignal>,
    shutdown: &AtomicBool,
) -> bool {
    if let Some(previous) = previous {
        while !previous.is_stopped() {
            thread::sleep(Duration::from_millis(20));
        }
    }
    !shutdown.load(Ordering::SeqCst)
}

struct BackendStoppedOnDrop(Arc<AtomicBool>);

impl Drop for BackendStoppedOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// The next thing a backend run loop should react to. Commands are drained
/// with priority; otherwise the loop blocks on both channels at once and only
/// takes a timed tick while streamed text is waiting to be flushed.
enum BackendInbound {
    Command(ChatBackendCommand),
    CommandsClosed,
    Message(ProviderMessage),
    MessagesClosed,
    FlushTick,
}

fn next_backend_inbound(
    commands: &Receiver<ChatBackendCommand>,
    messages: &Receiver<ProviderMessage>,
    flush_pending: bool,
) -> BackendInbound {
    match commands.try_recv() {
        Ok(command) => return BackendInbound::Command(command),
        Err(crossbeam_channel::TryRecvError::Empty) => {}
        Err(crossbeam_channel::TryRecvError::Disconnected) => {
            return BackendInbound::CommandsClosed;
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
        read_only: bool,
        turn_id: String,
    },
    /// Activate the frozen target only after the preceding backend has stopped.
    SendStudioTurn {
        text: String,
        mode: AgentInteractionMode,
        turn_id: String,
        title: String,
        request: super::StudioChatRequest,
    },
    UpdateAccessMode {
        access_mode: AgentAccessMode,
    },
    UpdateModelEffort {
        external_model: Option<(String, Vec<String>)>,
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
    ReservedEvidence {
        event: Box<ChatBackendEvent>,
        reservation: Arc<ide_core::agent_changes::EvidenceReservation>,
    },
    EvidenceOverflow,
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
    FileChangeActivity(FileChangeActivity),
    PendingUserInput(PendingUserInput),
    PendingApproval(super::PendingApproval),
    ProposedPlan(ProposedPlan),
    CodeReview(CodeReview),
    Verification(Verification),
    ReviewChecklist(ReviewChecklist),
    ChangedFiles(ChangedFilesSummary),
    ChangeReceiptPending(String),
    ChangeReceiptReady {
        summary: ChangedFilesSummary,
        state: ide_core::agent_changes::ChangeReceiptState,
    },
    Usage(ConversationUsage),
    Compaction(bool),
    Status(AgentChatStatus),
    Error(String),
}

impl ChatBackendController {
    pub fn send(
        &self,
        command: ChatBackendCommand,
    ) -> Result<(), crossbeam_channel::SendError<ChatBackendCommand>> {
        if matches!(&command, ChatBackendCommand::SendTurn { .. } | ChatBackendCommand::SendStudioTurn { .. }) {
            self.timing
                .prompt
                .store(ide_core::agent_changes::now_micros(), Ordering::Relaxed);
        }
        self.tx.send(command)
    }

    fn shutdown(&self) {
        if let Some((root, agent)) = &self.studio_scope {
            ide_core::studio::revoke_scope_at(root, *agent);
        }
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.tx.send(ChatBackendCommand::Shutdown);
    }

    pub fn cancel_turn(&self) {
        self.timing
            .interrupt
            .store(ide_core::agent_changes::now_micros(), Ordering::Relaxed);
        if let Some((root, agent)) = &self.studio_scope {
            ide_core::studio::revoke_scope_at(root, *agent);
        }
        let _ = self.tx.send(ChatBackendCommand::CancelTurn);
    }

    pub fn force_shutdown(&self) {
        if let Some((root, agent)) = &self.studio_scope {
            ide_core::studio::revoke_scope_at(root, *agent);
        }
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = self.tx.send(ChatBackendCommand::ForceShutdown);
    }

    pub fn stop_signal(&self) -> ChatBackendStopSignal {
        ChatBackendStopSignal {
            stopped: self.stopped.clone(),
        }
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
    spawn_chat_backend_after_stop(agent, initial_mode, None, None)
}

pub(super) fn spawn_chat_backend_after_stop(
    mut agent: AgentRecord,
    initial_mode: AgentInteractionMode,
    previous_stop: Option<ChatBackendStopSignal>,
    initial_turn_id: Option<String>,
) -> anyhow::Result<(
    ChatBackendController,
    async_channel::Receiver<ChatBackendEvent>,
)> {
    if let Some(run_id) = agent.review_run_id {
        anyhow::ensure!(agent.chat_session_id.is_none() && agent.cli_session_id.is_none() && agent.delegation.is_none() && agent.studio_context.is_none() && agent.design_context.is_none(), "Reviewer must have a fresh isolated role and session");
        choro_mcp_binary_path().context("Build or install Choro MCP before reviewing code")?;
        let store = LocalStore::open_default()?;
        let run = store.load_review_run(run_id)?;
        ide_core::code_review::authorize_review(&run, agent.project_id.0, agent.id, run_id, ide_core::code_review::review_now())?;
        anyhow::ensure!(agent.runtime_path().starts_with(store.review_storage(run_id)), "Reviewer must start outside the repository in its private app-data directory");
    } else {
        agent.doc.push_str("\n\n");
        agent.doc.push_str(ide_core::agent_changes::AGENT_CHANGE_INSTRUCTIONS);
        agent.doc = prompt_with_connected_context(&agent.doc, &agent, &AgentConnectedContextExtras::default());
    }
    anyhow::ensure!(agent.design_context.is_none(),
        "This legacy design conversation is retired. Open Design Studio to continue.");
    if let Some(context) = agent.studio_context.as_ref() {
        anyhow::ensure!(
            matches!(agent.provider, AgentKind::Codex | AgentKind::Claude),
            "Studio supports your existing Codex or Claude account"
        );
        choro_mcp_binary_path().context("Build or install Choro MCP before using Studio Agent")?;
        let store = ide_core::studio::StudioStore::for_project(&agent.project_path)?;
        ide_core::studio::atomic(
            &store.cache.join("roles").join(format!("{}.json", agent.id)),
            &serde_json::to_vec(context)?,
        )?;
    }
    let studio_scope = agent
        .studio_context
        .as_ref()
        .map(|_| (agent.project_path.clone(), agent.id));
    let (command_tx, command_rx) = crossbeam_channel::unbounded();
    let (mut event_tx, provider_rx) = event_channel();
    if let Some(id) = initial_turn_id { event_tx.initial_turn_id = id; }
    let (output, event_rx) = async_channel::unbounded();
    let timing = Arc::new(ide_core::agent_changes::DispatchTiming::default());
    if agent.review_run_id.is_some() {
        // No mutation receipt router, connected context or parent session.
        std::thread::spawn(move || {
            while let Ok(event) = provider_rx.recv_blocking() {
                if output.send_blocking(event).is_err() { break; }
            }
        });
    } else { change_tracking::route(agent.clone(), provider_rx, output.into(), timing.clone()); }
    let shutdown = Arc::new(AtomicBool::new(false));
    let stopped = Arc::new(AtomicBool::new(false));
    match agent.provider {
        AgentKind::Gemini => gemini::spawn_gemini_acp(
            agent, initial_mode, command_rx, event_tx, shutdown.clone(), stopped.clone(), previous_stop,
        )?,
        AgentKind::Codex => spawn_codex_app_server(
            agent,
            initial_mode,
            command_rx,
            event_tx,
            shutdown.clone(),
            stopped.clone(),
            previous_stop,
        )?,
        AgentKind::Claude => spawn_claude_bridge(
            agent,
            initial_mode,
            command_rx,
            event_tx,
            shutdown.clone(),
            stopped.clone(),
            previous_stop,
        )?,
        AgentKind::OpenCode => open_code::spawn_open_code_acp(
            agent,
            initial_mode,
            command_rx,
            event_tx,
            shutdown.clone(),
            stopped.clone(),
            previous_stop,
        )?,
    }
    Ok((
        ChatBackendController {
            timing,
            studio_scope,
            tx: command_tx,
            shutdown,
            stopped,
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
    stopped: Arc<AtomicBool>,
    previous_stop: Option<ChatBackendStopSignal>,
) -> anyhow::Result<()> {
    thread::Builder::new()
        .name("choro-claude-chat-bridge".into())
        .spawn(move || {
            let _stopped = BackendStoppedOnDrop(stopped);
            if !wait_for_previous_backend(previous_stop, &shutdown) {
                return;
            }
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
    stopped: Arc<AtomicBool>,
    previous_stop: Option<ChatBackendStopSignal>,
) -> anyhow::Result<()> {
    thread::Builder::new()
        .name("choro-codex-app-server".into())
        .spawn(move || {
            let _stopped = BackendStoppedOnDrop(stopped);
            if !wait_for_previous_backend(previous_stop, &shutdown) {
                return;
            }
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

/// A stopped or failed turn is not a completed design edit. Shared by providers.
#[derive(Default)]
struct StudioReviewGate {
    pending: bool,
    cancelled: bool,
}
impl StudioReviewGate {
    fn begin(&mut self, studio: bool) {
        self.pending = studio;
        self.cancelled = false;
    }
    fn cancel(&mut self) {
        self.pending = false;
        self.cancelled = true;
    }
    fn complete(&mut self) -> bool {
        std::mem::take(&mut self.pending)
    }
}

struct CodexRuntime {
    child: Child,
    stdin: Arc<Mutex<ChildStdin>>,
    messages: Receiver<ProviderMessage>,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    agent: AgentRecord,
    thread_id: Option<String>,
    turn_control: codex::CodexTurnControl,
    assistant_buffer: String,
    assistant_stream: StreamChunkBuffer,
    studio_review: StudioReviewGate,
    plan_buffer: String,
    pending_changed_files: Option<ChangedFilesSummary>,
    pending_file_previews: ide_core::agent_changes::PendingEvidence<Vec<FileChangeStat>>,
    pending_observed_files: Vec<FileChangeStat>,
    active_turn_id: String,
    command_ran_this_turn: bool,
    active_command_item_id: Option<String>,
    pending_user_inputs: std::collections::HashMap<String, PendingRequest>,
    pending_approvals: std::collections::HashMap<String, PendingApprovalRequest>,
    deferred_turns: VecDeque<ChatBackendCommand>,
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
    messages: Receiver<ProviderMessage>,
    commands: Receiver<ChatBackendCommand>,
    events: EventSender,
    shutdown: Arc<AtomicBool>,
    agent: AgentRecord,
    model: Option<String>,
    effort: String,
    access_mode: AgentAccessMode,
    claude_path: PathBuf,
    visualization_dir: Option<PathBuf>,
    assistant_buffer: String,
    assistant_stream: StreamChunkBuffer,
    studio_review: StudioReviewGate,
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
    if agent.review_run_id.is_some() {
        anyhow::ensure!(bridge_dir.join("node_modules/@anthropic-ai/claude-agent-sdk").exists(), "Review requires the existing bundled Claude bridge. Start a Claude coding session to initialize it, then Review again.");
    } else { ensure_claude_bridge_dependencies(bridge_dir, &npm_path, &event_tx)?; }
    if agent.studio_context.is_some() || agent.review_run_id.is_some() {
        let sdk: Value = serde_json::from_slice(&fs::read(
            bridge_dir.join("node_modules/@anthropic-ai/claude-agent-sdk/package.json"),
        )?)?;
        let output = ide_core::process::output_with_timeout(Command::new(&claude_path).arg("--help").env("PATH",command_path_env()), Duration::from_secs(10))
            .context("Studio compatibility check could not start Claude. Repair Claude Code, then reconnect.")?;
        anyhow::ensure!(
            output.status.success(),
            "Studio compatibility check failed. Repair Claude Code, then reconnect."
        );
        validate_claude_studio_compatibility(
            &String::from_utf8_lossy(&output.stdout),
            sdk["version"].as_str().unwrap_or(""),
        )?;
    }

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

    let (message_tx, message_rx) = crossbeam_channel::bounded(1);
    spawn_json_reader(stdout, message_tx);
    spawn_stderr_reader(stderr, event_tx.clone(), "Claude bridge");

    let visualization_dir = agent_visualization_dir(&agent);

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
        visualization_dir,
        assistant_buffer: String::new(),
        assistant_stream: StreamChunkBuffer::new(),
        studio_review: StudioReviewGate::default(),
    };

    if let Some(prompt) =
        initial_chat_prompt(&runtime.agent, runtime.agent.cli_session_id.is_some())
    {
        runtime.send_turn(prompt.to_owned(), initial_mode, false, runtime.events.initial_turn_id.clone())?;
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
    // Cargo test executables live one directory beneath the development
    // binaries. Acceptance uses the same freshly built MCP executable.
    #[cfg(test)]
    if let Some(path) = binary_dir
        .parent()
        .map(|p| p.join("choro-mcp"))
        .filter(|p| p.is_file())
    {
        return Some(path);
    }
    // Compatibility with development bundles created before the rename.
    let legacy = binary_dir.join("ide-mcp");
    legacy.exists().then_some(legacy)
}

fn choro_mcp_scope_args(project_id: &str, agent_id: &str, data_root: &Path) -> Vec<String> {
    vec![
        "--project-id".to_string(),
        project_id.to_string(),
        "--agent-id".to_string(),
        agent_id.to_string(),
        "--data-root".to_string(),
        data_root.display().to_string(),
    ]
}

fn agent_choro_mcp_scope_args(agent: &AgentRecord) -> Vec<String> {
    let mut args = choro_mcp_scope_args(
        &agent.project_id.0.to_string(),
        &agent.id.to_string(),
        &AppConfig::config_root(),
    );
    if agent.studio_context.is_some() {
        args.push("--studio".into());
    }
    if let Some(run_id) = agent.review_run_id { args.extend(["--review-run".into(), run_id.to_string()]); }
    args
}

fn codex_mcp_args_config_arg(name: &str, agent: &AgentRecord) -> String {
    let args = serde_json::to_string(&agent_choro_mcp_scope_args(agent))
        .unwrap_or_else(|_| "[]".to_string());
    format!("mcp_servers.{name}.args={args}")
}

fn codex_scoped_mcp_approval_config_arg(name: &str) -> String {
    // The dedicated assistant is already file-bound and can only write through
    // this isolated Design MCP server. Without this server-level override,
    // `approvalPolicy=never` turns every Design tool invocation into
    // "user rejected MCP tool call" before Choro can run it.
    format!("mcp_servers.{name}.default_tools_approval_mode=\"approve\"")
}

/// Managed children must report through Choro even when approvalPolicy is never.
/// Grant only the scoped coordination protocol, not other MCP or shell tools.
fn configure_codex_child_coordination(command: &mut Command, agent: &AgentRecord) {
    if !managed::is_child(agent) {
        return;
    }
    for tool in [
        "delegation_read",
        "delegation_message",
        "delegation_complete",
    ] {
        command.arg("-c").arg(format!(
            "mcp_servers.ide.tools.{tool}.approval_mode=\"approve\""
        ));
    }
}

fn configured_codex_mcp_names(
    codex_path: &Path,
    cwd: &Path,
    path_env: &str,
) -> anyhow::Result<Vec<String>> {
    let output = ide_core::process::output_with_timeout(Command::new(codex_path)
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
        .current_dir(cwd), Duration::from_secs(10))
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

fn validate_codex_studio_features(output: &str) -> anyhow::Result<()> {
    let features = output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((parts.next()?, parts.last()?))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    // ShellTool is the outer gate for both exec_command and the legacy shell.
    // New Codex builds keep UnifiedExec enabled; disabling the shell gate still
    // omits both handlers. Do not require that implementation selector to be false.
    // https://github.com/openai/codex/blob/main/codex-rs/core/src/tools/spec_plan.rs
    for name in ["shell_tool", "multi_agent"] {
        anyhow::ensure!(
            features.get(name) == Some(&"false"),
            "Studio could not disable Codex {name}. Update or repair Codex, then reconnect."
        );
    }
    for name in ["multi_agent_v2", "apps", "plugins"] {
        if let Some(value) = features.get(name) {
            anyhow::ensure!(*value == "false", "Studio could not disable Codex {name}. Reconnect with a compatible Codex installation.");
        }
    }
    Ok(())
}

fn configure_codex_studio(
    command: &mut Command,
    codex: &Path,
    agent: &AgentRecord,
    path_env: &str,
) -> anyhow::Result<()> {
    let restrictions = [
        "features.shell_tool=false",
        "features.multi_agent=false",
        "features.multi_agent_v2=false",
        "agents.enabled=false",
        "features.plugins=false",
        "features.apps=false",
    ];
    let mut check = Command::new(codex);
    for restriction in restrictions {
        check.args(["-c", restriction]);
        command.args(["-c", restriction]);
    }
    let output = check
        .args(["features", "list"])
        .env("PATH", path_env)
        .output()?;
    let features = String::from_utf8_lossy(&output.stdout);
    anyhow::ensure!(
        output.status.success(),
        "Studio could not read Codex capabilities. Reconnect or repair the Codex installation."
    );
    validate_codex_studio_features(&features)?;
    for name in configured_codex_mcp_names(codex, agent.runtime_path(), path_env)? {
        command
            .arg("-c")
            .arg(format!("mcp_servers.{name}.enabled=false"));
    }
    let mcp = choro_mcp_binary_path().context("Choro MCP is unavailable")?;
    let name = format!("studio_{}", agent.id.simple());
    command
        .arg("-c")
        .arg(format!("mcp_servers.{name}.command={}", mcp.display()))
        .arg("-c")
        .arg(codex_mcp_args_config_arg(&name, agent))
        .arg("-c")
        .arg(format!("mcp_servers.{name}.enabled=true"))
        .arg("-c")
        .arg(format!("mcp_servers.{name}.required=true"))
        .arg("-c")
        .arg(codex_scoped_mcp_approval_config_arg(&name));
    Ok(())
}

/// The `mcpServers` object handed to the Claude Agent SDK, scoped to this Choro
/// chat and project. Studio roles use the scoped first-party server.
fn choro_mcp_servers_json(agent: &AgentRecord) -> Value {
    let mut servers = serde_json::Map::new();
    if let Some(mcp) = choro_mcp_binary_path() {
        servers.insert(
            "choro".to_string(),
            json!({
                "type": "stdio",
                "command": mcp.display().to_string(),
                "args": agent_choro_mcp_scope_args(agent),
            }),
        );
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
    // ACP reviewers receive tool results through Choro's bounded host protocol;
    // no provider-owned native MCP connector receives the snapshot directory.
    if agent.review_run_id.is_some() { return json!([]); }
    let servers = match choro_mcp_binary_path() {
        Some(mcp) => choro_acp_mcp_servers_json_at(
            &mcp,
            &agent.project_id.0.to_string(),
            &agent.id.to_string(),
            &AppConfig::config_root(),
        )
        .as_array()
        .cloned()
        .unwrap_or_default(),
        None => Vec::new(),
    };
    Value::Array(servers)
}

fn choro_acp_mcp_servers_json_at(
    mcp: &Path,
    project_id: &str,
    agent_id: &str,
    data_root: &Path,
) -> Value {
    json!([{
        "name": "choro",
        "command": mcp.display().to_string(),
        "args": choro_mcp_scope_args(project_id, agent_id, data_root),
        "env": [],
    }])
}

pub(crate) fn claude_bridge_script_path() -> anyhow::Result<PathBuf> {
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

fn validate_claude_studio_compatibility(help: &str, sdk: &str) -> anyhow::Result<()> {
    anyhow::ensure!(sdk == "0.3.170", "Studio requires the bundled Claude Agent SDK 0.3.170. Repair Choro's Claude bridge dependencies, then reconnect.");
    for flag in [
        "--tools",
        "--disallowedTools",
        "--strict-mcp-config",
        "--permission-mode",
        "--setting-sources",
    ] {
        anyhow::ensure!(help.contains(flag), "Claude Code lacks {flag}, required for Studio restrictions. Update Claude Code, then reconnect Studio.");
    }
    Ok(())
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
    if agent.delegation.is_some() {
        managed::preflight(agent.provider)?;
        command.args([
            "-c",
            "agents.enabled=false",
            "-c",
            "features.multi_agent=false",
            "-c",
            "features.multi_agent_v2=false",
        ]);
    }
    command
        .args(["app-server", "--stdio"])
        .env("PATH", &path_env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(agent.runtime_path());
    if agent.review_run_id.is_some() {
        review::configure_codex(&mut command, &codex_path, &agent, &path_env)?;
    } else if agent.studio_context.is_some() {
        configure_codex_studio(&mut command, &codex_path, &agent, &path_env)?;
    } else {
        if let Some(mcp) = choro_mcp_binary_path() {
            command
                .arg("-c")
                .arg(format!("mcp_servers.ide.command={}", mcp.display()));
            command
                .arg("-c")
                .arg(codex_mcp_args_config_arg("ide", &agent));
            if ide_core::delegation::enabled() {
                // Codex filters inherited stdio-server environment variables.
                // Pass the development gate explicitly or discovery loses all
                // delegation tools while the model still receives its brief.
                command.args([
                    "-c",
                    "mcp_servers.ide.env.CHORO_EXPERTS=\"1\"",
                    "-c",
                    "mcp_servers.ide.enabled=true",
                ]);
            }
            if agent.delegation.is_some() {
                command.args(["-c", "mcp_servers.ide.required=true"]);
                configure_codex_child_coordination(&mut command, &agent);
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

    let (message_tx, message_rx) = crossbeam_channel::bounded(1);
    spawn_json_reader(stdout, message_tx);
    spawn_stderr_reader(stderr, event_tx.clone(), "Codex app-server");

    let visualization_dir = agent_visualization_dir(&agent);

    let mut runtime = CodexRuntime {
        child,
        stdin,
        messages: message_rx,
        commands: command_rx,
        events: event_tx,
        shutdown,
        agent: agent.clone(),
        thread_id: None,
        turn_control: codex::CodexTurnControl::default(),
        assistant_buffer: String::new(),
        assistant_stream: StreamChunkBuffer::new(),
        studio_review: StudioReviewGate::default(),
        plan_buffer: String::new(),
        pending_changed_files: None,
        pending_file_previews: Default::default(),
        pending_observed_files: Vec::new(),
        active_turn_id: next_request_id(),
        command_ran_this_turn: false,
        active_command_item_id: None,
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
    let design_assistant = agent.studio_context.is_some() || agent.review_run_id.is_some();
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
    let mut start_params = json!({
        "cwd": agent.runtime_path(),
        "approvalPolicy": approval_policy,
        "sandbox": sandbox,
        "model": runtime.model,
    });
    if agent.review_run_id.is_some() {
        start_params["environments"] = json!([]);
        start_params["ephemeral"] = json!(true);
        start_params["developerInstructions"] = json!(ide_core::code_review::REVIEW_INSTRUCTIONS);
        start_params["baseInstructions"] = json!(ide_core::code_review::REVIEW_INSTRUCTIONS);
    }
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
                        format!("Codex could not resume the existing conversation: {error:#}"),
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
    if agent.review_run_id.is_some() { review::validate_codex_session(&thread)?; }
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
    if let Some(prompt) = initial_chat_prompt(&agent, is_resuming_existing_thread)
        .filter(|_| !runtime.studio_review.cancelled)
    {
        runtime.send_turn(prompt.to_owned(), initial_mode, false, runtime.events.initial_turn_id.clone())?;
    }

    runtime.run_loop()
}

/// Managed sessions are started by the durable delivery queue, including when
/// a lead backend is reconfigured. Only ordinary fresh chats auto-send the doc.
fn initial_chat_prompt(agent: &AgentRecord, resuming: bool) -> Option<&str> {
    (!resuming
        && agent.delegation.is_none()
        && !agent.hidden_doc_assistant
        && !agent.doc.trim().is_empty())
    .then_some(agent.doc.as_str())
}

struct ProviderMessage {
    value: Value,
    _reservation: Option<ide_core::agent_changes::EvidenceReservation>,
}
impl std::ops::Deref for ProviderMessage {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.value
    }
}
impl From<Value> for ProviderMessage {
    fn from(mut value: Value) -> Self {
        let bytes = serde_json::to_vec(&value).map_or(usize::MAX, |s| s.len());
        let reservation = ide_core::agent_changes::EvidenceBudget::global().reserve(bytes);
        if reservation.is_none() {
            // Retain protocol routing/terminal fields, discard unsupported
            // evidence payloads and explicitly mark the receipt incomplete.
            fn strip(value: &mut Value) {
                match value {
                    Value::Object(fields) => {
                        for key in [
                            "diff",
                            "changes",
                            "oldText",
                            "newText",
                            "rawInput",
                            "content",
                            "file",
                            "files",
                            "observed_files",
                        ] {
                            fields.remove(key);
                        }
                        for value in fields.values_mut() {
                            strip(value);
                        }
                    }
                    Value::Array(values) => {
                        for value in values {
                            strip(value);
                        }
                    }
                    Value::String(s) if s.len() > 4096 => {
                        s.truncate(s.floor_char_boundary(4096));
                    }
                    _ => {}
                }
            }
            strip(&mut value);
            value["_choro_evidence_incomplete"] = json!(true);
        }
        Self {
            value,
            _reservation: reservation,
        }
    }
}

fn spawn_json_reader(stdout: impl std::io::Read + Send + 'static, tx: Sender<ProviderMessage>) {
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(value) = serde_json::from_str::<Value>(&line) {
                if tx.send(value.into()).is_err() {
                    break;
                }
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

    #[test]
    fn studio_stop_then_send_uses_a_fresh_completion_review() {
        let mut gate = super::StudioReviewGate::default();
        gate.begin(true);
        gate.cancel();
        assert!(
            !gate.complete(),
            "Stop must not run completion review on revoked scope"
        );
        gate.begin(true);
        assert!(
            gate.complete(),
            "The next Studio turn still requires review"
        );
        assert!(
            !gate.complete(),
            "Duplicate idle events must not re-review revoked scope"
        );
        gate.begin(false);
        assert!(
            !gate.complete(),
            "Ordinary agents do not require Studio review"
        );
    }
    use super::*;

    #[test]
    fn replacement_waits_until_previous_backend_cleanup_finishes() {
        let stopped = Arc::new(AtomicBool::new(false));
        let previous = ChatBackendStopSignal {
            stopped: stopped.clone(),
        };
        let (tx, rx) = crossbeam_channel::bounded(1);
        let worker = thread::spawn(move || {
            tx.send(wait_for_previous_backend(
                Some(previous),
                &AtomicBool::new(false),
            ))
            .unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(60)).is_err());
        stopped.store(true, Ordering::SeqCst);
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn stopping_a_waiting_replacement_does_not_release_its_predecessor() {
        let predecessor_stopped = Arc::new(AtomicBool::new(false));
        let replacement_stopped = Arc::new(AtomicBool::new(false));
        let previous = ChatBackendStopSignal {
            stopped: predecessor_stopped.clone(),
        };
        let replacement = replacement_stopped.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let worker = thread::spawn(move || {
            let _stopped = BackendStoppedOnDrop(replacement);
            tx.send(wait_for_previous_backend(
                Some(previous),
                &AtomicBool::new(true),
            ))
            .unwrap();
        });
        assert!(rx.recv_timeout(Duration::from_millis(60)).is_err());
        assert!(!replacement_stopped.load(Ordering::SeqCst));
        predecessor_stopped.store(true, Ordering::SeqCst);
        assert!(!rx.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
        assert!(replacement_stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn studio_codex_checks_shell_gate_not_execution_backend_selector() {
        let current="shell_tool stable false\nunified_exec stable true\nmulti_agent stable false\nmulti_agent_v2 stable false\napps stable false\nplugins stable false";
        assert!(validate_codex_studio_features(current).is_ok());
        assert!(validate_codex_studio_features(
            "shell_tool stable false\nmulti_agent experimental false"
        )
        .is_ok());
        for name in [
            "shell_tool",
            "multi_agent",
            "multi_agent_v2",
            "apps",
            "plugins",
        ] {
            assert!(validate_codex_studio_features(&current.replace(
                &format!("{name} stable false"),
                &format!("{name} stable true")
            ))
            .is_err());
        }
        assert!(validate_codex_studio_features(
            "unified_exec stable false\nmulti_agent stable false"
        )
        .is_err());
        assert!(validate_codex_studio_features("garbled output").is_err());
    }
    #[test]
    fn studio_claude_preflight_requires_the_pinned_sdk_and_every_policy_flag() {
        let flags = [
            "--tools",
            "--disallowedTools",
            "--strict-mcp-config",
            "--permission-mode",
            "--setting-sources",
        ];
        assert!(validate_claude_studio_compatibility(&flags.join(" "), "0.3.170").is_ok());
        assert!(validate_claude_studio_compatibility(&flags.join(" "), "0.3.169").is_err());
        for missing in flags {
            assert!(validate_claude_studio_compatibility(
                &flags
                    .into_iter()
                    .filter(|f| *f != missing)
                    .collect::<Vec<_>>()
                    .join(" "),
                "0.3.170"
            )
            .is_err());
        }
    }
    #[test]
    fn retired_design_conversation_cannot_launch_a_backend() {
        let mut agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()), PathBuf::from("/tmp/project"),
            "Legacy design", "Continue", AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex), AgentEffort::default(),
            AgentAccessMode::FullAccess,
        );
        agent.design_context = Some(ide_core::AgentDesignContext {
            design_id: uuid::Uuid::new_v4(), file_id: uuid::Uuid::new_v4(),
        });
        let result = spawn_chat_backend(agent.clone(), AgentInteractionMode::Default);
        assert!(matches!(result, Err(error) if error.to_string().contains("retired")));
        agent.design_context = None;
        agent.doc.push_str("\n<choro-penpot-design local-id=\"legacy\" />");
        assert!(choro_mcp_servers_json(&agent).get("penpot").is_none());
        assert!(choro_acp_mcp_servers_json(&agent).as_array().unwrap().iter()
            .all(|server| server.get("name").and_then(Value::as_str) != Some("penpot")));
    }

    #[test]
    fn studio_mcp_scope_survives_full_access() {
        let mut agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            PathBuf::from("/tmp/project"),
            "Studio",
            "Design a screen",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::FullAccess,
        );
        agent.studio_context = Some(ide_core::studio::StudioAgentContext {
            target: ide_core::studio::StudioAgentTarget::Design,
            design_id: uuid::Uuid::new_v4(),
            conversation_id: uuid::Uuid::new_v4(),
        });
        assert!(agent_choro_mcp_scope_args(&agent).contains(&"--studio".to_string()));
        let value = serde_json::to_value(&agent).unwrap();
        assert_eq!(
            serde_json::from_value::<AgentRecord>(value)
                .unwrap()
                .studio_context,
            agent.studio_context
        );
    }

    #[test]
    fn visualization_instructions_are_provider_neutral() {
        let path = Path::new("/tmp/choro artifacts/visualizations");
        let instructions = append_choro_visualization_instructions("base".to_string(), Some(path));

        assert!(instructions.contains(&path.display().to_string()));
        assert!(instructions.contains("::codex-inline-vis{file=\"<absolute-file-path>\"}"));
        assert!(instructions.contains("Do not write visualization HTML into the project"));
    }

    #[test]
    fn jsonrpc_id_key_preserves_numeric_request_ids() {
        assert_eq!(jsonrpc_id_key(&json!(77)), "77");
        assert_eq!(jsonrpc_id_key(&json!("abc")), "abc");
    }

    #[test]
    fn managed_startup_never_sends_an_implicit_assignment() {
        for provider in [AgentKind::Codex, AgentKind::Claude] {
            let mut agent = AgentRecord::new(
                ide_core::ProjectId(uuid::Uuid::new_v4()),
                PathBuf::from("/tmp/project"),
                "Bandmate",
                "Initial assignment",
                provider,
                AgentModel::default_for(provider),
                AgentEffort::default(),
                AgentAccessMode::FullAccess,
            );
            assert_eq!(
                initial_chat_prompt(&agent, false),
                Some("Initial assignment")
            );
            assert_eq!(initial_chat_prompt(&agent, true), None);
            agent.hidden_doc_assistant = true;
            assert_eq!(initial_chat_prompt(&agent, false), None);
            agent.hidden_doc_assistant = false;
            agent.delegation = Some(ide_core::delegation::DelegationBinding {
                run_id: uuid::Uuid::new_v4(),
                parent_agent_id: agent.id,
                task_id: None,
                attempt_id: None,
                workspace: None,
                task_kind: None,
            });
            for task_kind in [
                None,
                Some(ide_core::delegation::TaskKind::Implementation),
                Some(ide_core::delegation::TaskKind::Consultation),
            ] {
                let binding = agent.delegation.as_mut().unwrap();
                binding.task_id = task_kind.map(|_| uuid::Uuid::new_v4());
                binding.task_kind = task_kind;
                // Fresh children and restarted/reconfigured leads all wait for
                // the coordinator's durable delivery instead of executing doc.
                assert_eq!(initial_chat_prompt(&agent, false), None);
                assert_eq!(initial_chat_prompt(&agent, true), None);
            }
            agent.delegation = None;
            assert_eq!(
                initial_chat_prompt(&agent, false),
                Some("Initial assignment")
            );
            agent.doc = "  ".into();
            assert_eq!(initial_chat_prompt(&agent, false), None);
        }
    }

    #[test]
    fn child_coordination_approval_is_scoped_to_reporting_tools() {
        let mut agent = AgentRecord::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            PathBuf::from("/tmp/project"),
            "Bandmate",
            "Assigned task",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::FullAccess,
        );
        let args = |agent: &AgentRecord| {
            let mut command = Command::new("codex");
            configure_codex_child_coordination(&mut command, agent);
            command
                .get_args()
                .map(|s| s.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert!(args(&agent).is_empty());
        agent.delegation = Some(ide_core::delegation::DelegationBinding {
            run_id: uuid::Uuid::new_v4(),
            parent_agent_id: agent.id,
            task_id: None,
            attempt_id: None,
            workspace: None,
            task_kind: None,
        });
        assert!(args(&agent).is_empty(), "lead policy must stay unchanged");
        let binding = agent.delegation.as_mut().unwrap();
        binding.task_id = Some(uuid::Uuid::new_v4());
        binding.attempt_id = Some(uuid::Uuid::new_v4());
        let expected = vec![
            "-c",
            "mcp_servers.ide.tools.delegation_read.approval_mode=\"approve\"",
            "-c",
            "mcp_servers.ide.tools.delegation_message.approval_mode=\"approve\"",
            "-c",
            "mcp_servers.ide.tools.delegation_complete.approval_mode=\"approve\"",
        ];
        for kind in [
            ide_core::delegation::TaskKind::Implementation,
            ide_core::delegation::TaskKind::Consultation,
        ] {
            agent.delegation.as_mut().unwrap().task_kind = Some(kind);
            assert_eq!(args(&agent), expected);
        }
        assert_eq!(agent.access_mode, AgentAccessMode::FullAccess);
    }

    #[test]
    fn dedicated_design_mcp_tools_are_preapproved() {
        assert_eq!(
            codex_scoped_mcp_approval_config_arg("studio_123"),
            "mcp_servers.studio_123.default_tools_approval_mode=\"approve\""
        );
    }

    #[test]
    fn backend_inbound_prefers_commands_over_messages() {
        let (command_tx, command_rx) = crossbeam_channel::unbounded();
        let (message_tx, message_rx) = crossbeam_channel::bounded(1);
        message_tx.send(json!({"type": "noise"}).into()).unwrap();
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
        let (message_tx, message_rx) = crossbeam_channel::unbounded::<ProviderMessage>();

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
        let (_message_tx, message_rx) = crossbeam_channel::unbounded::<ProviderMessage>();

        let start = Instant::now();
        match next_backend_inbound(&command_rx, &message_rx, true) {
            BackendInbound::FlushTick => {}
            _ => panic!("pending flush must produce a timed tick"),
        }
        assert!(start.elapsed() >= CHAT_STREAM_FLUSH_INTERVAL);
    }

    #[test]
    fn stream_chunk_buffer_flushes_exact_text_batch() {
        let (tx, rx) = event_channel();
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
        let (tx, rx) = event_channel();
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
        let (tx, rx) = event_channel();
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
            Path::new("/tmp/choro isolated"),
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
                    "--data-root",
                    "/tmp/choro isolated",
                ],
                "env": [],
            }])
        );
    }

}
