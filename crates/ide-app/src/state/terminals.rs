#![allow(dead_code, reason = "retained document-assistant terminal API")]

use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use anyhow::{Context as _, Result};
use gpui::{px, AppContext, Context, Edges, Entity, Hsla, SharedString};
use gpui_component::ActiveTheme;
use gpui_terminal::{ColorPalette, TerminalConfig, TerminalView};
use ide_core::{AgentKind, ProjectId};
use parking_lot::Mutex;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use uuid::Uuid;

pub type SessionId = u64;
type TerminalThemeKey = ((u8, u8, u8), (u8, u8, u8), (u8, u8, u8));

const INITIAL_ROWS: u16 = 30;
const INITIAL_COLS: u16 = 110;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectPreviewService {
    pub session_id: SessionId,
    pub title: String,
    pub url: String,
}

#[derive(Default)]
struct PreviewOutput {
    tail: String,
    urls: Vec<String>,
}

impl PreviewOutput {
    fn ingest(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        let mut combined = String::with_capacity(self.tail.len() + text.len());
        combined.push_str(&self.tail);
        combined.push_str(&text);
        for url in preview_urls_in_text(&combined) {
            if !self.urls.iter().any(|existing| existing == &url) {
                self.urls.push(url);
            }
        }
        self.tail = combined
            .chars()
            .rev()
            .take(512)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
}

struct PreviewDetectingReader<R> {
    inner: R,
    output: Arc<Mutex<PreviewOutput>>,
}

impl<R: Read> Read for PreviewDetectingReader<R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let read = self.inner.read(buffer)?;
        if read > 0 {
            self.output.lock().ingest(&buffer[..read]);
        }
        Ok(read)
    }
}

fn preview_urls_in_text(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut cursor = 0;
    while cursor < text.len() {
        let rest = &text[cursor..];
        let next_http = rest.find("http://");
        let next_https = rest.find("https://");
        let offset = match (next_http, next_https) {
            (Some(left), Some(right)) => left.min(right),
            (Some(offset), None) | (None, Some(offset)) => offset,
            (None, None) => break,
        };
        let start = cursor + offset;
        let candidate = &text[start..];
        let Some(end) = candidate
            .char_indices()
            .skip(1)
            .find_map(|(index, character)| {
                (character.is_whitespace()
                    || character.is_control()
                    || matches!(character, '"' | '\'' | '<' | '>' | ')' | ']' | '}'))
                .then_some(index)
            })
        else {
            // PTY reads may split a URL at any byte. Keep this suffix in the
            // rolling tail and only accept it once a delimiter arrives.
            break;
        };
        let url = candidate[..end]
            .trim_end_matches(|character: char| matches!(character, '.' | ',' | ';' | ':'));
        if url.len() > "http://".len()
            && url.len() <= 2_048
            && !urls.iter().any(|existing| existing == url)
        {
            urls.push(url.to_string());
        }
        cursor = start + end.max(1);
    }
    urls
}

fn preferred_preview_url(urls: &[String]) -> Option<String> {
    urls.iter()
        .enumerate()
        .max_by_key(|(index, value)| {
            let Ok(url) = url::Url::parse(value) else {
                return (0_u8, 0_u8, *index);
            };
            let host = url.host_str().unwrap_or_default().trim_matches(['[', ']']);
            let host_rank = if host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
            {
                2
            } else if host
                .parse::<IpAddr>()
                .is_ok_and(|address| address.is_unspecified())
            {
                1
            } else {
                0
            };
            let root_rank = u8::from(url.path() == "/" && url.query().is_none());
            (host_rank, root_rank, *index)
        })
        .map(|(_, value)| value.clone())
}

/// The Solo slug of a lane script title (`"dev — solo auth-fix"` → `auth-fix`),
/// `None` for ordinary scripts. Sidebars use this to show lane runs as just
/// their Solo name.
pub fn solo_script_slug(title: &str) -> Option<&str> {
    let (_, slug) = title.split_once(TerminalManager::SOLO_SCRIPT_MARKER)?;
    let slug = slug.trim();
    (!slug.is_empty()).then_some(slug)
}

fn collapse_preview_services_by_title(
    services: Vec<ProjectPreviewService>,
) -> Vec<ProjectPreviewService> {
    let mut titles = HashSet::new();
    services
        .into_iter()
        .rev()
        .filter(|service| titles.insert(service.title.clone()))
        .collect()
}

fn push_path_entries(entries: &mut Vec<String>, path: &str) {
    for entry in path.split(':').filter(|entry| !entry.is_empty()) {
        if !entries.iter().any(|existing| existing == entry) {
            entries.push(entry.to_string());
        }
    }
}

fn push_existing_path(entries: &mut Vec<String>, path: PathBuf) {
    if !path.is_dir() {
        return;
    }
    let entry = path.to_string_lossy().to_string();
    if !entries.iter().any(|existing| existing == &entry) {
        entries.push(entry);
    }
}

fn push_nvm_node_paths(entries: &mut Vec<String>, home: &std::path::Path) {
    let node_versions = home.join(".nvm/versions/node");
    let Ok(version_dirs) = std::fs::read_dir(&node_versions) else {
        return;
    };

    let mut bins: Vec<PathBuf> = version_dirs
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("bin"))
        .filter(|path| path.is_dir())
        .collect();
    bins.sort_by(|left, right| right.cmp(left));

    let default_prefix = std::fs::read_to_string(home.join(".nvm/alias/default"))
        .ok()
        .map(|default| default.trim().to_string())
        .filter(|default| !default.is_empty())
        .map(|default| {
            if default.starts_with('v') {
                default
            } else {
                format!("v{default}")
            }
        });

    if let Some(default_prefix) = default_prefix {
        for bin in bins.iter().filter(|bin| {
            bin.parent()
                .and_then(|path| path.file_name())
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&default_prefix))
        }) {
            push_existing_path(entries, bin.clone());
        }
    }

    for bin in bins {
        push_existing_path(entries, bin);
    }
}

fn shell_seed_path() -> String {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let mut entries = Vec::new();

        if let Ok(path) = std::env::var("PATH") {
            push_path_entries(&mut entries, &path);
        }

        if let Some(home) = dirs::home_dir() {
            push_existing_path(&mut entries, home.join(".local/bin"));
            push_existing_path(&mut entries, home.join(".opencode/bin"));
            push_existing_path(&mut entries, home.join(".rbenv/bin"));
            push_existing_path(&mut entries, home.join(".rbenv/shims"));
            push_existing_path(&mut entries, home.join(".bun/bin"));
            push_existing_path(&mut entries, home.join("Library/pnpm"));
            push_nvm_node_paths(&mut entries, &home);
        }

        for entry in [
            "/opt/homebrew/bin",
            "/opt/homebrew/sbin",
            "/usr/local/bin",
            "/usr/local/sbin",
            "/usr/bin",
            "/bin",
            "/usr/sbin",
            "/sbin",
            "/Library/Apple/usr/bin",
        ] {
            push_existing_path(&mut entries, PathBuf::from(entry));
        }

        entries.join(":")
    })
    .clone()
}

fn login_shell_path(shell: &str, seed_path: &str) -> Option<String> {
    static PATH: OnceLock<Option<String>> = OnceLock::new();
    PATH.get_or_init(|| {
        let output = std::process::Command::new(shell)
            .env("PATH", seed_path)
            .args(["-lc", "printf '%s' \"$PATH\""])
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
            .filter(|path| !path.is_empty())
    })
    .clone()
}

pub struct TerminalSession {
    pub id: SessionId,
    pub project: ProjectId,
    pub title: SharedString,
    /// Some(command) for preset runs, None for a plain shell.
    pub command: Option<String>,
    pub cwd: PathBuf,
    pub view: Entity<TerminalView>,
    pub exited: bool,
    /// Some(true) = exited cleanly, Some(false) = non-zero exit code.
    pub exit_success: Option<bool>,
    /// Set when this terminal hosts an AI agent chat (Claude/Codex).
    pub agent: Option<AgentKind>,
    /// The app-owned agent record this terminal belongs to.
    pub agent_record_id: Option<Uuid>,
    /// The docs-assistant record this terminal belongs to.
    pub doc_assistant_key: Option<String>,
    /// The agent CLI's own session id, once known — used to resume the
    /// conversation after an app restart.
    pub agent_session_id: Option<String>,
    /// When the terminal was spawned; used to adopt the session id of a
    /// brand-new agent chat once its transcript appears on disk.
    pub spawned_at: std::time::SystemTime,
    preview_output: Arc<Mutex<PreviewOutput>>,
    /// Kept alive for the session's lifetime — dropping the master closes the PTY.
    _master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
}

/// Owns every terminal session across all projects; the center panel
/// filters them by the active project so sessions survive project switches.
pub struct TerminalManager {
    pub sessions: Vec<TerminalSession>,
    pub active_tab: HashMap<ProjectId, SessionId>,
    /// Active tab of the Agents view, tracked separately from terminals.
    pub active_agent_tab: HashMap<ProjectId, SessionId>,
    /// When the user last focused each agent chat (by agent session id) —
    /// suppresses the "waiting for you" indicator until the agent writes again.
    attention_acked: HashMap<String, std::time::SystemTime>,
    /// Transcript writes that predate this app run are restored history, not new
    /// attention. Only writes after this baseline can make an idle chat unread.
    attention_baseline: std::time::SystemTime,
    terminal_theme_key: Option<TerminalThemeKey>,
    next_id: SessionId,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            active_tab: HashMap::new(),
            active_agent_tab: HashMap::new(),
            attention_acked: HashMap::new(),
            attention_baseline: std::time::SystemTime::now(),
            terminal_theme_key: None,
            next_id: 1,
        }
    }

    /// Plain terminals and preset runs (agent chats live in the Agents view).
    pub fn sessions_for(&self, project: ProjectId) -> Vec<&TerminalSession> {
        self.sessions
            .iter()
            .filter(|s| s.project == project && s.agent.is_none())
            .collect()
    }

    /// Agent chat terminals for the Agents view.
    pub fn agent_sessions_for(&self, project: ProjectId) -> Vec<&TerminalSession> {
        self.sessions
            .iter()
            .filter(|s| s.project == project && s.agent.is_some() && s.agent_record_id.is_some())
            .collect()
    }

    pub fn doc_assistant_terminal(&self, project: ProjectId, key: &str) -> Option<SessionId> {
        self.sessions
            .iter()
            .find(|s| {
                s.project == project && !s.exited && s.doc_assistant_key.as_deref() == Some(key)
            })
            .map(|s| s.id)
    }

    pub fn doc_assistant_session(&self, project: ProjectId, key: &str) -> Option<&TerminalSession> {
        self.sessions
            .iter()
            .find(|s| s.project == project && s.doc_assistant_key.as_deref() == Some(key))
    }

    /// The live (not exited) terminal already hosting an agent session id.
    pub fn agent_session_terminal(
        &self,
        project: ProjectId,
        session_id: &str,
    ) -> Option<SessionId> {
        self.sessions
            .iter()
            .find(|s| {
                s.project == project
                    && !s.exited
                    && s.agent_session_id.as_deref() == Some(session_id)
            })
            .map(|s| s.id)
    }

    /// The live (not exited) terminal attached to an app-owned agent record.
    pub fn agent_record_terminal(&self, project: ProjectId, agent_id: Uuid) -> Option<SessionId> {
        self.sessions
            .iter()
            .find(|s| s.project == project && !s.exited && s.agent_record_id == Some(agent_id))
            .map(|s| s.id)
    }

    pub fn agent_record_session(
        &self,
        project: ProjectId,
        agent_id: Uuid,
    ) -> Option<&TerminalSession> {
        self.sessions
            .iter()
            .find(|s| s.project == project && s.agent_record_id == Some(agent_id))
    }

    /// Names of script presets currently running (not plain shells).
    /// Joins a preset name and a Solo slug into the lane terminal's title, and
    /// is what `solo_script_slug` splits back apart.
    pub const SOLO_SCRIPT_MARKER: &'static str = " — solo ";

    pub fn running_scripts(&self, project: ProjectId) -> Vec<SharedString> {
        self.sessions
            .iter()
            .filter(|s| {
                s.project == project && !s.exited && s.command.is_some() && s.agent.is_none()
            })
            .map(|s| s.title.clone())
            .collect()
    }

    /// HTTP(S) endpoints printed by live project terminals. Development
    /// servers nearly always announce their actual bound URL, which is more
    /// reliable than guessing framework default ports from a script name.
    pub fn project_preview_services(&self, project: ProjectId) -> Vec<ProjectPreviewService> {
        let services = self
            .sessions
            .iter()
            .filter(|session| {
                session.project == project && !session.exited && session.agent.is_none()
            })
            .filter_map(|session| {
                let url = preferred_preview_url(&session.preview_output.lock().urls)?;
                Some(ProjectPreviewService {
                    session_id: session.id,
                    title: session.title.to_string(),
                    url,
                })
            })
            .collect();
        collapse_preview_services_by_title(services)
    }

    pub fn active_session(&self, project: ProjectId) -> Option<&TerminalSession> {
        let id = *self.active_tab.get(&project)?;
        self.sessions
            .iter()
            .find(|s| s.id == id && s.project == project)
    }

    pub fn set_active(&mut self, project: ProjectId, id: SessionId, cx: &mut Context<Self>) {
        let agent_session = self
            .sessions
            .iter()
            .find(|s| s.id == id && s.agent.is_some());
        if let Some(session) = agent_session {
            // Focusing a chat counts as seeing it — clear its attention state.
            if let Some(session_id) = session.agent_session_id.clone() {
                self.attention_acked
                    .insert(session_id, std::time::SystemTime::now());
            }
            self.active_agent_tab.insert(project, id);
        } else {
            self.active_tab.insert(project, id);
        }
        cx.notify();
    }

    /// True when the user already saw this chat's latest state: it was
    /// focused after the transcript's last write.
    pub fn attention_suppressed(
        &self,
        session_id: &str,
        updated_at: std::time::SystemTime,
    ) -> bool {
        if updated_at <= self.attention_baseline {
            return true;
        }
        self.attention_acked
            .get(session_id)
            .map(|acked| *acked >= updated_at)
            .unwrap_or(false)
    }

    pub fn acknowledge_attention(&mut self, session_id: &str, cx: &mut Context<Self>) {
        self.attention_acked
            .insert(session_id.to_string(), std::time::SystemTime::now());
        cx.notify();
    }

    pub fn sync_theme(&mut self, cx: &mut Context<Self>) {
        let foreground = crate::ui::style::focus_text(cx);
        let theme = cx.theme();
        let key = terminal_theme_key(theme.background, foreground, theme.primary);
        if self.terminal_theme_key == Some(key) {
            return;
        }
        self.terminal_theme_key = Some(key);
        let config = terminal_config(theme.background, foreground, theme.primary);
        for session in &self.sessions {
            session.view.update(cx, |terminal, cx| {
                terminal.update_config(config.clone(), cx);
            });
        }
    }

    pub fn spawn_shell(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        self.spawn(project, cwd, "zsh".into(), None, cx)
    }

    pub fn spawn_preset(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        name: &str,
        command: &str,
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        self.spawn_preset_with_env(project, cwd, name, command, &[], cx)
    }

    /// A preset with extra exported env vars — how a Solo lane's dev server
    /// gets its own `PORT` instead of fighting the main one.
    pub fn spawn_preset_with_env(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        name: &str,
        command: &str,
        envs: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        self.spawn_with_env(
            project,
            cwd,
            SharedString::from(name.to_string()),
            Some(command.to_string()),
            envs,
            cx,
        )
    }

    /// Opens an app-owned agent terminal with an already-built CLI command.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_agent_record(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        agent_id: Uuid,
        kind: AgentKind,
        title: String,
        command: String,
        cli_session_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        let previous_tab = self.active_tab.get(&project).copied();
        let id = self.spawn(project, cwd, SharedString::from(title), Some(command), cx)?;
        match previous_tab {
            Some(prev) => {
                self.active_tab.insert(project, prev);
            }
            None => {
                self.active_tab.remove(&project);
            }
        }
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.agent = Some(kind);
            session.agent_record_id = Some(agent_id);
            session.doc_assistant_key = None;
            session.agent_session_id = cli_session_id;
        }
        self.active_agent_tab.insert(project, id);
        Ok(id)
    }

    /// Opens a plain interactive shell scoped to an app-owned chat agent.
    ///
    /// Chat agents already have a live provider backend. Resuming that same
    /// provider session in a second CLI would create two writers for one
    /// conversation, so their Terminal drawer owns an independent shell
    /// instead.
    pub fn spawn_agent_shell(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        agent_id: Uuid,
        kind: AgentKind,
        title: String,
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        let previous_tab = self.active_tab.get(&project).copied();
        let id = self.spawn(project, cwd, SharedString::from(title), None, cx)?;
        match previous_tab {
            Some(prev) => {
                self.active_tab.insert(project, prev);
            }
            None => {
                self.active_tab.remove(&project);
            }
        }
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.agent = Some(kind);
            session.agent_record_id = Some(agent_id);
            session.doc_assistant_key = None;
            session.agent_session_id = None;
        }
        self.active_agent_tab.insert(project, id);
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn_doc_assistant(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        key: String,
        kind: AgentKind,
        title: String,
        command: String,
        cli_session_id: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        let previous_tab = self.active_tab.get(&project).copied();
        let id = self.spawn(project, cwd, SharedString::from(title), Some(command), cx)?;
        match previous_tab {
            Some(prev) => {
                self.active_tab.insert(project, prev);
            }
            None => {
                self.active_tab.remove(&project);
            }
        }
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.agent = Some(kind);
            session.agent_record_id = None;
            session.doc_assistant_key = Some(key);
            session.agent_session_id = cli_session_id;
        }
        Ok(id)
    }

    /// Best-effort: gives terminals hosting brand-new chats their session id
    /// (and real title) once the transcript shows up on disk. `chats_by_cwd`
    /// must contain freshly listed chats for each agent terminal cwd.
    pub fn adopt_agent_ids(
        &mut self,
        chats_by_cwd: &HashMap<PathBuf, Vec<ide_core::AgentChat>>,
        cx: &mut Context<Self>,
    ) -> Vec<(Uuid, String)> {
        let mut claimed: Vec<String> = self
            .sessions
            .iter()
            .filter_map(|s| s.agent_session_id.clone())
            .collect();
        let mut changed = false;
        let mut adopted_records = Vec::new();
        for session in self.sessions.iter_mut().filter(|s| {
            s.agent_record_id.is_some()
                && s.agent.is_some()
                && s.command.is_some()
                && s.agent_session_id.is_none()
                && !s.exited
        }) {
            let Some(chats) = chats_by_cwd.get(&session.cwd) else {
                continue;
            };
            let adopted = chats.iter().find(|chat| {
                Some(chat.kind) == session.agent
                    && !claimed.contains(&chat.session_id)
                    && chat.updated_at >= session.spawned_at
            });
            if let Some(chat) = adopted {
                session.agent_session_id = Some(chat.session_id.clone());
                session.title = SharedString::from(chat.title.clone());
                if let Some(agent_id) = session.agent_record_id {
                    adopted_records.push((agent_id, chat.session_id.clone()));
                }
                claimed.push(chat.session_id.clone());
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
        adopted_records
    }

    pub fn adopt_doc_assistant_ids(
        &mut self,
        chats_by_cwd: &HashMap<PathBuf, Vec<ide_core::AgentChat>>,
        cx: &mut Context<Self>,
    ) -> Vec<(String, String)> {
        let mut claimed: Vec<String> = self
            .sessions
            .iter()
            .filter_map(|s| s.agent_session_id.clone())
            .collect();
        let mut changed = false;
        let mut adopted_records = Vec::new();
        for session in self
            .sessions
            .iter_mut()
            .filter(|s| s.doc_assistant_key.is_some() && s.agent_session_id.is_none() && !s.exited)
        {
            let Some(chats) = chats_by_cwd.get(&session.cwd) else {
                continue;
            };
            let adopted = chats.iter().find(|chat| {
                Some(chat.kind) == session.agent
                    && !claimed.contains(&chat.session_id)
                    && chat.updated_at >= session.spawned_at
            });
            if let Some(chat) = adopted {
                session.agent_session_id = Some(chat.session_id.clone());
                session.title = SharedString::from(chat.title.clone());
                if let Some(key) = session.doc_assistant_key.clone() {
                    adopted_records.push((key, chat.session_id.clone()));
                }
                claimed.push(chat.session_id.clone());
                changed = true;
            }
        }
        if changed {
            cx.notify();
        }
        adopted_records
    }

    pub fn restart(&mut self, id: SessionId, cx: &mut Context<Self>) -> Result<()> {
        let Some(session) = self.sessions.iter().find(|s| s.id == id) else {
            return Ok(());
        };
        let (
            project,
            cwd,
            title,
            command,
            agent,
            agent_record_id,
            doc_assistant_key,
            agent_session_id,
        ) = (
            session.project,
            session.cwd.clone(),
            session.title.clone(),
            session.command.clone(),
            session.agent,
            session.agent_record_id,
            session.doc_assistant_key.clone(),
            session.agent_session_id.clone(),
        );
        self.close(id, cx);
        let new_id = self.spawn(project, cwd, title, command, cx)?;
        if agent.is_some() {
            if let Some(session) = self.sessions.iter_mut().find(|s| s.id == new_id) {
                session.agent = agent;
                session.agent_record_id = agent_record_id;
                session.doc_assistant_key = doc_assistant_key;
                session.agent_session_id = agent_session_id;
            }
            if agent_record_id.is_some() {
                self.active_agent_tab.insert(project, new_id);
            }
        }
        Ok(())
    }

    pub fn close(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if let Some(ix) = self.sessions.iter().position(|s| s.id == id) {
            let session = self.sessions.remove(ix);
            session.terminate();
            let project = session.project;
            if self.active_tab.get(&project) == Some(&id) {
                let fallback = self.sessions_for(project).last().map(|s| s.id);
                match fallback {
                    Some(fallback) => {
                        self.active_tab.insert(project, fallback);
                    }
                    None => {
                        self.active_tab.remove(&project);
                    }
                }
            }
            if self.active_agent_tab.get(&project) == Some(&id) {
                let fallback = self.agent_sessions_for(project).last().map(|s| s.id);
                match fallback {
                    Some(fallback) => {
                        self.active_agent_tab.insert(project, fallback);
                    }
                    None => {
                        self.active_agent_tab.remove(&project);
                    }
                }
            }
            cx.notify();
        }
    }

    pub fn send_text(&mut self, id: SessionId, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(session) = self
            .sessions
            .iter()
            .find(|session| session.id == id && !session.exited)
        else {
            return false;
        };
        session
            .view
            .update(cx, |terminal, _| terminal.send_text(text));
        true
    }

    fn mark_exited(&mut self, id: SessionId, cx: &mut Context<Self>) {
        if let Some(session) = self.sessions.iter_mut().find(|s| s.id == id) {
            session.exited = true;
            session.exit_success = session
                .child
                .lock()
                .try_wait()
                .ok()
                .flatten()
                .map(|status| status.success());
            cx.notify();
        }
    }

    /// Latest run of a preset (by name): (session id, running, exit success).
    pub fn preset_state(
        &self,
        project: ProjectId,
        name: &str,
    ) -> Option<(SessionId, bool, Option<bool>)> {
        self.sessions
            .iter()
            .rev()
            .find(|s| {
                s.project == project
                    && s.command.is_some()
                    && s.agent.is_none()
                    && s.title.as_ref() == name
            })
            .map(|s| (s.id, !s.exited, s.exit_success))
    }

    fn spawn(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        title: SharedString,
        command: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        self.spawn_with_env(project, cwd, title, command, &[], cx)
    }

    fn spawn_with_env(
        &mut self,
        project: ProjectId,
        cwd: PathBuf,
        title: SharedString,
        command: Option<String>,
        envs: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> Result<SessionId> {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: INITIAL_ROWS,
                cols: INITIAL_COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("failed to open PTY")?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        let mut cmd = CommandBuilder::new(&shell);
        let seed_path = shell_seed_path();
        let path = login_shell_path(&shell, &seed_path).unwrap_or(seed_path);
        match &command {
            // Login shell so user PATH (brew, nvm, ...) is loaded. Explicitly
            // cd first: login-shell init files can override the PTY cwd.
            Some(script) => {
                let dir = cwd.display().to_string().replace('\'', "'\\''");
                let mut exports = format!("export PATH='{}'; ", path.replace('\'', "'\\''"));
                // Exported inline like PATH: login-shell init files run after
                // plain process env and could otherwise clobber these.
                for (key, value) in envs {
                    exports.push_str(&format!(
                        "export {}='{}'; ",
                        key,
                        value.replace('\'', "'\\''")
                    ));
                }
                cmd.args(["-lc", &format!("{exports}cd '{dir}' && {script}")]);
            }
            None => {
                cmd.arg("-l");
            }
        }
        cmd.cwd(&cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("PATH", path);
        for (key, value) in envs {
            cmd.env(key, value);
        }
        if let Some(home) = dirs::home_dir() {
            cmd.env("SUPERSET_HOME_DIR", home.join(".superset"));
        }

        let child = pty
            .slave
            .spawn_command(cmd)
            .context("failed to spawn shell")?;
        let writer = pty.master.take_writer().context("PTY writer")?;
        let reader = pty.master.try_clone_reader().context("PTY reader")?;
        let preview_output = Arc::new(Mutex::new(PreviewOutput::default()));
        let reader = PreviewDetectingReader {
            inner: reader,
            output: preview_output.clone(),
        };
        drop(pty.slave);

        let master = Arc::new(Mutex::new(pty.master));
        let child = Arc::new(Mutex::new(child));

        let id = self.next_id;
        self.next_id += 1;

        let resize_master = master.clone();
        let manager = cx.weak_entity();
        let view = cx.new(|cx| {
            let foreground = crate::ui::style::focus_text(cx);
            let theme = cx.theme();
            TerminalView::new(
                writer,
                reader,
                terminal_config(theme.background, foreground, theme.primary),
                cx,
            )
            .with_resize_callback(move |cols, rows| {
                let _ = resize_master.lock().resize(PtySize {
                    rows: rows as u16,
                    cols: cols as u16,
                    pixel_width: 0,
                    pixel_height: 0,
                });
            })
            .with_exit_callback(move |_, cx| {
                manager
                    .update(cx, |manager, cx| manager.mark_exited(id, cx))
                    .ok();
            })
        });

        self.sessions.push(TerminalSession {
            id,
            project,
            title,
            command,
            cwd,
            view,
            exited: false,
            exit_success: None,
            agent: None,
            agent_record_id: None,
            doc_assistant_key: None,
            agent_session_id: None,
            spawned_at: std::time::SystemTime::now(),
            preview_output,
            _master: master,
            child,
        });
        self.active_tab.insert(project, id);
        cx.notify();
        Ok(id)
    }

    /// Terminate and forget every PTY/process group during graceful shutdown.
    pub fn shutdown_all(&mut self, cx: &mut Context<Self>) {
        let sessions = std::mem::take(&mut self.sessions);
        self.active_tab.clear();
        self.active_agent_tab.clear();
        drop(sessions);
        cx.notify();
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        for session in &self.sessions {
            session.terminate();
        }
    }
}

impl TerminalSession {
    fn terminate(&self) {
        #[cfg(unix)]
        let process_group = self._master.lock().process_group_leader();

        #[cfg(unix)]
        if let Some(process_group) = process_group {
            let _ = signal_process_group(process_group, libc::SIGHUP);
        }

        let _ = self.child.lock().kill();

        #[cfg(unix)]
        if let Some(process_group) = process_group {
            std::thread::sleep(std::time::Duration::from_millis(120));
            let _ = signal_process_group(process_group, libc::SIGTERM);
            std::thread::sleep(std::time::Duration::from_millis(120));
            let _ = signal_process_group(process_group, libc::SIGKILL);
        }
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.terminate();
    }
}

#[cfg(unix)]
fn signal_process_group(process_group: libc::pid_t, signal: libc::c_int) -> std::io::Result<()> {
    if process_group <= 1 {
        return Ok(());
    }
    let result = unsafe { libc::kill(-process_group, signal) };
    if result == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

fn terminal_config(background: Hsla, foreground: Hsla, cursor: Hsla) -> TerminalConfig {
    TerminalConfig {
        cols: INITIAL_COLS as usize,
        rows: INITIAL_ROWS as usize,
        font_family: "Menlo".into(),
        font_size: px(13.0),
        scrollback: 10_000,
        line_height_multiplier: 1.0,
        padding: Edges {
            top: px(8.0),
            right: px(12.0),
            bottom: px(8.0),
            left: px(14.0),
        },
        colors: app_palette(background, foreground, cursor),
    }
}

fn hsla_rgb(color: Hsla) -> (u8, u8, u8) {
    let rgb = color.to_rgb();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    (channel(rgb.r), channel(rgb.g), channel(rgb.b))
}

fn terminal_theme_key(background: Hsla, foreground: Hsla, cursor: Hsla) -> TerminalThemeKey {
    (hsla_rgb(background), hsla_rgb(foreground), hsla_rgb(cursor))
}

fn app_palette(background: Hsla, foreground: Hsla, cursor: Hsla) -> ColorPalette {
    let (bg_r, bg_g, bg_b) = hsla_rgb(background);
    let (fg_r, fg_g, fg_b) = hsla_rgb(foreground);
    let (cursor_r, cursor_g, cursor_b) = hsla_rgb(cursor);
    ColorPalette::builder()
        .background(bg_r, bg_g, bg_b)
        .foreground(fg_r, fg_g, fg_b)
        .cursor(cursor_r, cursor_g, cursor_b)
        .black(bg_r, bg_g, bg_b)
        .red(0xE0, 0x6C, 0x75)
        .green(0x98, 0xC3, 0x79)
        .yellow(0xE5, 0xC0, 0x7B)
        .blue(0x61, 0xAF, 0xEF)
        .magenta(0xC6, 0x78, 0xDD)
        .cyan(0x56, 0xB6, 0xC2)
        .white(0xAB, 0xB2, 0xBF)
        .bright_black(0x5C, 0x63, 0x70)
        .bright_red(0xFF, 0x7A, 0x84)
        .bright_green(0xA9, 0xD4, 0x8A)
        .bright_yellow(0xF2, 0xCE, 0x8B)
        .bright_blue(0x74, 0xBE, 0xFF)
        .bright_magenta(0xD8, 0x8A, 0xEF)
        .bright_cyan(0x67, 0xC7, 0xD3)
        .bright_white(fg_r, fg_g, fg_b)
        .build()
}

#[cfg(test)]
mod tests {
    use super::{
        collapse_preview_services_by_title, preferred_preview_url, preview_urls_in_text,
        solo_script_slug, PreviewOutput, ProjectPreviewService,
    };

    #[test]
    fn solo_script_slug_extracts_only_lane_titles() {
        assert_eq!(solo_script_slug("dev — solo auth-fix"), Some("auth-fix"));
        assert_eq!(
            solo_script_slug("Run — solo create-need"),
            Some("create-need")
        );
        assert_eq!(solo_script_slug("dev"), None);
        assert_eq!(solo_script_slug("dev — solo "), None);
    }

    #[test]
    fn lane_suffixed_preset_titles_survive_the_title_collapse() {
        // A Solo runs the same preset in its lane under a suffixed title, so
        // main and lane servers must both stay visible in the preview picker.
        let services = vec![
            ProjectPreviewService {
                session_id: 1,
                title: "Web".to_string(),
                url: "http://localhost:3000".to_string(),
            },
            ProjectPreviewService {
                session_id: 2,
                title: "Web — solo auth-fix".to_string(),
                url: "http://localhost:3001".to_string(),
            },
            // A rerun of the same title keeps only the newest entry.
            ProjectPreviewService {
                session_id: 3,
                title: "Web".to_string(),
                url: "http://localhost:3002".to_string(),
            },
        ];
        let collapsed = collapse_preview_services_by_title(services);
        let mut urls: Vec<_> = collapsed
            .iter()
            .map(|service| service.url.as_str())
            .collect();
        urls.sort();
        assert_eq!(urls, vec!["http://localhost:3001", "http://localhost:3002"]);
    }

    #[test]
    fn detects_preview_urls_and_trims_terminal_punctuation() {
        assert_eq!(
            preview_urls_in_text(
                "Local: http://localhost:5173/\nNetwork: https://10.0.0.8:5173/app)."
            ),
            vec![
                "http://localhost:5173/".to_string(),
                "https://10.0.0.8:5173/app".to_string(),
            ]
        );
    }

    #[test]
    fn detects_url_split_across_pty_reads_once() {
        let mut output = PreviewOutput::default();
        output.ingest(b"server ready at http://local");
        output.ingest(b"host:4310/index.html\n");
        output.ingest(b"again http://localhost:4310/index.html\n");
        assert_eq!(
            output.urls,
            vec!["http://localhost:4310/index.html".to_string()]
        );
    }

    #[test]
    fn preview_prefers_latest_loopback_root_from_one_script() {
        let urls = vec![
            "http://10.0.0.8:5173/".to_string(),
            "http://localhost:5173/docs".to_string(),
            "http://localhost:5173/".to_string(),
            "http://localhost:5174/".to_string(),
        ];

        assert_eq!(
            preferred_preview_url(&urls).as_deref(),
            Some("http://localhost:5174/")
        );
    }

    #[test]
    fn preview_keeps_only_newest_live_run_for_each_script_name() {
        let services = vec![
            ProjectPreviewService {
                session_id: 1,
                title: "Web".to_string(),
                url: "http://localhost:3000/".to_string(),
            },
            ProjectPreviewService {
                session_id: 2,
                title: "Server".to_string(),
                url: "http://localhost:4000/".to_string(),
            },
            ProjectPreviewService {
                session_id: 3,
                title: "Web".to_string(),
                url: "http://localhost:3001/".to_string(),
            },
        ];

        let collapsed = collapse_preview_services_by_title(services);

        assert_eq!(collapsed.len(), 2);
        assert_eq!(collapsed[0].title, "Web");
        assert_eq!(collapsed[0].url, "http://localhost:3001/");
        assert_eq!(collapsed[1].title, "Server");
    }
}
