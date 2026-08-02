use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, PathPromptOptions, Render,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement,
    spinner::Spinner,
    v_flex, Disableable, Icon, IconName, Selectable, Sizable,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::keymap;
use crate::remote::{DevicePermission, RelayControl, RelayIdentity, RelayState, RemoteAuth};
use crate::state::{
    AgentCapability, AgentCapabilityCacheFile, ChoroRiff, ChoroRiffStore, DesignProvider,
    PenpotConnectionStatus, PenpotState, Workspace, CHORO_RIFFS_SCHEMA_VERSION,
};
use ide_core::{
    config::{
        ConversationLayout, GenerationAgent, NewAgentDefaults, ThemeMode as ConfigTheme,
        VerificationMode, DEFAULT_CODE_REVIEW_PROMPT,
    },
    local_store::LocalStore,
    AgentEffort, AgentKind, AgentModel,
};
use uuid::Uuid;

const PENPOT_CLOUD_URL: &str = "https://design.penpot.app";
const PENPOT_CLOUD_MCP_URL: &str = "https://design.penpot.app/mcp/stream";

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsSection {
    Design,
    Generation,
    Process,
    AgentSkills,
    Memory,
    Remote,
    Data,
    Shortcuts,
    Appearance,
}

impl SettingsSection {
    fn title(self) -> &'static str {
        match self {
            Self::Design => "Design",
            Self::Generation => "AI generation",
            Self::Process => "Process monitor",
            Self::AgentSkills => "Skills",
            Self::Memory => "Memory",
            Self::Remote => "Remote access",
            Self::Data => "Data & backups",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::Appearance => "Appearance",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Design => {
                "Use Choro’s managed Design workspace or connect an existing Penpot Cloud account."
            }
            Self::Generation => {
                "Choose how Choro writes generated Git content and configure the agent-chat code review prompt."
            }
            Self::Process => "Understand how Choro and its connected tools use system resources.",
            Self::AgentSkills => {
                "Create Choro Riffs for every project and review skills discovered from your coding agents."
            }
            Self::Memory => {
                "Facts every agent starts with — global ones about you, project ones about each repo."
            }
            Self::Remote => "Pair iPhones directly with this Mac and revoke old devices.",
            Self::Data => "Create a portable workspace archive or restore one from disk.",
            Self::Shortcuts => "Shape the keyboard workflow around the way you work.",
            Self::Appearance => "Personalize Choro's theme and conversation layout.",
        }
    }

    fn matches(self, query: &str) -> bool {
        query.is_empty()
            || self.title().to_lowercase().contains(query)
            || self.description().to_lowercase().contains(query)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SkillProviderFilter {
    Choro,
    Codex,
    Claude,
}

struct MemoryEditor {
    /// `Some` = editing an existing memory; `None` = creating one.
    id: Option<Uuid>,
    input: Entity<InputState>,
    error: Option<String>,
}

struct RiffEditor {
    session_id: Uuid,
    id: Option<Uuid>,
    name: Entity<InputState>,
    description: Entity<InputState>,
    instructions: Entity<InputState>,
    generating: bool,
    error: Option<String>,
}

#[derive(Clone)]
struct ProjectSource {
    id: ide_core::ProjectId,
    name: String,
    path: String,
}

/// Settings dialog: editable keyboard shortcuts (persisted to config).
pub struct SettingsView {
    workspace: Entity<Workspace>,
    penpot: Entity<PenpotState>,
    design_provider: DesignProvider,
    design_instance_input: Entity<InputState>,
    design_mcp_input: Entity<InputState>,
    design_access_token_input: Entity<InputState>,
    design_mcp_key_input: Entity<InputState>,
    design_error: Option<String>,
    remote_auth: RemoteAuth,
    remote_relay_control: RelayControl,
    remote_room_id: String,
    remote_status: Option<String>,
    shortcut_overrides: HashMap<String, String>,
    shortcut_search: Entity<InputState>,
    shortcut_modified_only: bool,
    recording_shortcut: Option<String>,
    shortcut_error: Option<String>,
    shortcut_focus: FocusHandle,
    projects: Vec<ProjectSource>,
    section: SettingsSection,
    process_snapshot: ProcessSnapshot,
    process_loading: bool,
    process_seq: u64,
    skills: Vec<AgentCapability>,
    riffs: Vec<ChoroRiff>,
    riff_editor: Option<RiffEditor>,
    riffs_status: Option<String>,
    memories: Vec<ide_core::local_store::StoredMemory>,
    /// Which tab: `true` = Global, `false` = Project.
    memory_scope_global: bool,
    memory_project: Option<ide_core::ProjectId>,
    memory_editor: Option<MemoryEditor>,
    memory_status: Option<String>,
    settings_search: Entity<InputState>,
    code_review_prompt: Entity<InputState>,
    code_review_prompt_dirty: bool,
    skills_provider: SkillProviderFilter,
    skills_search: Entity<InputState>,
    skills_loading: bool,
    skills_error: Option<String>,
    skills_last_refreshed: Option<String>,
    skills_cwd: String,
    skills_seq: u64,
    data_busy: bool,
    data_status: Option<String>,
}

#[derive(Clone, Default)]
struct ProcessSnapshot {
    root_pid: i32,
    app_mb: f64,
    total_mb: f64,
    total_cpu: f64,
    process_count: usize,
    top: Vec<ProcessInfo>,
    error: Option<String>,
}

#[derive(Clone)]
struct ProcessInfo {
    pid: i32,
    rss_kb: u64,
    cpu: f64,
    project: Option<String>,
    command: String,
}

impl SkillProviderFilter {
    fn label(self) -> &'static str {
        match self {
            SkillProviderFilter::Choro => "Choro",
            SkillProviderFilter::Codex => "Codex",
            SkillProviderFilter::Claude => "Claude",
        }
    }

    fn provider(self) -> Option<AgentKind> {
        match self {
            SkillProviderFilter::Choro => None,
            SkillProviderFilter::Codex => Some(AgentKind::Codex),
            SkillProviderFilter::Claude => Some(AgentKind::Claude),
        }
    }
}

impl SettingsView {
    /// Build the settings view. It now lives as a dedicated full-screen route in
    /// the root layout (not a modal), so this just returns the entity for the
    /// caller to mount.
    pub fn new(
        workspace: Entity<Workspace>,
        penpot: Entity<PenpotState>,
        remote_auth: RemoteAuth,
        relay_identity: RelayIdentity,
        relay_control: RelayControl,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        Self::new_in_section(
            workspace,
            penpot,
            remote_auth,
            relay_identity,
            relay_control,
            SettingsSection::AgentSkills,
            window,
            cx,
        )
    }

    /// Open Settings directly on Remote access when invoked from the header's
    /// connection indicator.
    pub fn new_remote(
        workspace: Entity<Workspace>,
        penpot: Entity<PenpotState>,
        remote_auth: RemoteAuth,
        relay_identity: RelayIdentity,
        relay_control: RelayControl,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        Self::new_in_section(
            workspace,
            penpot,
            remote_auth,
            relay_identity,
            relay_control,
            SettingsSection::Remote,
            window,
            cx,
        )
    }

    fn new_in_section(
        workspace: Entity<Workspace>,
        penpot: Entity<PenpotState>,
        remote_auth: RemoteAuth,
        relay_identity: RelayIdentity,
        relay_control: RelayControl,
        section: SettingsSection,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let state = workspace.read(cx);
        let overrides = state.keymap.clone();
        let projects: Vec<ProjectSource> = state
            .projects
            .iter()
            .map(|project| ProjectSource {
                id: project.id,
                name: project.name.clone(),
                path: project.path.display().to_string(),
            })
            .collect();
        let skills_cwd = state
            .active_project()
            .or_else(|| state.projects.first())
            .map(|project| project.path.display().to_string())
            .unwrap_or_else(|| {
                std::env::current_dir()
                    .unwrap_or_default()
                    .display()
                    .to_string()
            });
        let code_review_prompt_value = state.effective_code_review_prompt().to_string();
        let active_project = state.active;
        let shortcut_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts"));
        let settings_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search settings…"));
        let code_review_prompt = cx.new(|cx| InputState::new(window, cx).multi_line(true));
        code_review_prompt.update(cx, |input, cx| {
            input.set_value(code_review_prompt_value, window, cx)
        });
        let skills_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        let design_provider = penpot.read(cx).provider();
        let design_config = penpot.read(cx).config().clone();
        let design_instance_value = if design_provider == DesignProvider::PenpotCloud {
            design_config.instance_url
        } else {
            PENPOT_CLOUD_URL.to_string()
        };
        let design_mcp_value = if design_provider == DesignProvider::PenpotCloud {
            design_config.mcp_url
        } else {
            PENPOT_CLOUD_MCP_URL.to_string()
        };
        let design_instance_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(design_instance_value)
                .placeholder(PENPOT_CLOUD_URL)
        });
        let design_mcp_input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(design_mcp_value)
                .placeholder(PENPOT_CLOUD_MCP_URL)
        });
        let design_access_token_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste your Penpot access token")
                .masked(true)
        });
        let design_mcp_key_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste your Penpot MCP key")
                .masked(true)
        });
        let cached_skills = AgentCapabilityCacheFile::load();
        let riffs = ChoroRiffStore::load().riffs;
        let view = cx.new(|cx| {
            cx.subscribe(
                &code_review_prompt,
                |this: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.code_review_prompt_dirty = true;
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &shortcut_search,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.observe(&penpot, |_: &mut Self, _, cx| cx.notify())
                .detach();
            Self {
                workspace: workspace.clone(),
                penpot: penpot.clone(),
                design_provider,
                design_instance_input,
                design_mcp_input,
                design_access_token_input,
                design_mcp_key_input,
                design_error: None,
                remote_auth,
                remote_relay_control: relay_control,
                remote_room_id: relay_identity.room_id().to_string(),
                remote_status: None,
                shortcut_overrides: overrides,
                shortcut_search,
                shortcut_modified_only: false,
                recording_shortcut: None,
                shortcut_error: None,
                shortcut_focus: cx.focus_handle(),
                projects: projects.clone(),
                section,
                process_snapshot: ProcessSnapshot {
                    root_pid: std::process::id() as i32,
                    ..Default::default()
                },
                process_loading: false,
                process_seq: 0,
                skills: cached_skills.capabilities,
                riffs,
                riff_editor: None,
                riffs_status: None,
                memories: ide_core::local_store::LocalStore::open_default()
                    .and_then(|store| store.load_all_memories())
                    .unwrap_or_default(),
                memory_scope_global: false,
                memory_project: active_project,
                memory_editor: None,
                memory_status: None,
                settings_search,
                code_review_prompt,
                code_review_prompt_dirty: false,
                skills_provider: SkillProviderFilter::Choro,
                skills_search,
                skills_loading: false,
                skills_error: None,
                skills_last_refreshed: cached_skills.refreshed_at,
                skills_cwd,
                skills_seq: 0,
                data_busy: false,
                data_status: None,
            }
        });
        view.update(cx, |view, cx| view.refresh_process_snapshot(cx));
        view.update(cx, |view, cx| view.refresh_agent_skills(cx));
        view
    }

    fn select_theme(&mut self, name: String, cx: &mut Context<Self>) {
        if crate::theme::apply_named(&name, cx) {
            let mode = if name == "Choro Light" {
                ConfigTheme::Light
            } else {
                ConfigTheme::Dark
            };
            self.workspace.update(cx, |workspace, cx| {
                workspace.set_theme(mode, cx);
                workspace.set_theme_name(Some(name), cx);
                workspace.save_now();
            });
            cx.notify();
        }
    }

    fn select_conversation_layout(&mut self, layout: ConversationLayout, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_conversation_layout(layout, cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn select_default_agent_provider(&mut self, provider: AgentKind, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_new_agent_defaults(NewAgentDefaults::for_provider(provider), cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn select_default_agent_model(&mut self, model: AgentModel, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            let mut defaults = workspace.new_agent_defaults();
            if model.belongs_to(defaults.provider) {
                defaults.model = model;
                // A new model resets effort to its own sweet spot; the row
                // below is there to override it.
                defaults.effort = model.default_effort();
                workspace.set_new_agent_defaults(defaults, cx);
                workspace.save_now();
            }
        });
        cx.notify();
    }

    fn select_default_agent_effort(&mut self, effort: AgentEffort, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            let mut defaults = workspace.new_agent_defaults();
            defaults.effort = effort;
            workspace.set_new_agent_defaults(defaults, cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn select_generation_provider(&mut self, provider: AgentKind, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_generation_agent(GenerationAgent::for_provider(provider), cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn select_generation_model(&mut self, model: AgentModel, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            let mut generation_agent = workspace.generation_agent.clone();
            if model.belongs_to(generation_agent.provider) {
                generation_agent.model = model;
                workspace.set_generation_agent(generation_agent, cx);
                workspace.save_now();
            }
        });
        cx.notify();
    }

    fn save_code_review_prompt(&mut self, cx: &mut Context<Self>) {
        let prompt = self.code_review_prompt.read(cx).value().trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_code_review_prompt(prompt, cx);
            workspace.save_now();
        });
        self.code_review_prompt_dirty = false;
        cx.notify();
    }

    fn restore_default_code_review_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let prompt = DEFAULT_CODE_REVIEW_PROMPT.to_string();
        self.code_review_prompt.update(cx, |input, cx| {
            input.set_value(prompt.clone(), window, cx);
        });
        self.workspace.update(cx, |workspace, cx| {
            // Persist the actual default text in config.json so the reset is
            // explicit and remains inspectable/editable outside the app.
            workspace.set_code_review_prompt(prompt, cx);
            workspace.save_now();
        });
        self.code_review_prompt_dirty = false;
        cx.notify();
    }

    fn begin_shortcut_recording(
        &mut self,
        id: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.recording_shortcut = Some(id);
        self.shortcut_error = None;
        window.focus(&self.shortcut_focus);
        cx.notify();
    }

    fn persist_shortcut_overrides(
        &mut self,
        previous: HashMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        keymap::apply_bindings(cx, &previous, &self.shortcut_overrides);
        let next = self.shortcut_overrides.clone();
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_keymap(next, cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn reset_shortcut(&mut self, id: &str, cx: &mut Context<Self>) {
        let previous = self.shortcut_overrides.clone();
        self.shortcut_overrides.remove(id);
        self.shortcut_error = None;
        self.persist_shortcut_overrides(previous, cx);
    }

    fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        if self.shortcut_overrides.is_empty() {
            return;
        }
        let previous = self.shortcut_overrides.clone();
        self.shortcut_overrides.clear();
        self.recording_shortcut = None;
        self.shortcut_error = None;
        self.persist_shortcut_overrides(previous, cx);
    }

    fn capture_shortcut(
        &mut self,
        event: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.recording_shortcut.clone() else {
            return;
        };
        cx.stop_propagation();
        if event.is_held {
            return;
        }
        let stroke = &event.keystroke;
        if stroke.key == "escape" {
            self.recording_shortcut = None;
            self.shortcut_error = None;
            cx.notify();
            return;
        }
        if matches!(stroke.key.as_str(), "backspace" | "delete")
            && !stroke.modifiers.control
            && !stroke.modifiers.alt
            && !stroke.modifiers.shift
            && !stroke.modifiers.platform
        {
            let previous = self.shortcut_overrides.clone();
            self.shortcut_overrides.insert(id, String::new());
            self.recording_shortcut = None;
            self.shortcut_error = None;
            self.persist_shortcut_overrides(previous, cx);
            return;
        }
        if !(stroke.modifiers.platform || stroke.modifiers.control || stroke.modifiers.alt) {
            self.shortcut_error =
                Some("Use Command, Control, or Option so normal typing stays untouched.".into());
            cx.notify();
            return;
        }

        let keys = stroke.unparse();
        let Some(normalized) = keymap::normalized_keystroke(&keys) else {
            self.shortcut_error = Some("Choro could not read that shortcut.".into());
            cx.notify();
            return;
        };
        if matches!(
            normalized.as_str(),
            "cmd-h"
                | "cmd-m"
                | "cmd-space"
                | "cmd-tab"
                | "cmd-a"
                | "cmd-c"
                | "cmd-v"
                | "cmd-x"
                | "cmd-z"
                | "cmd-shift-z"
        ) {
            self.shortcut_error =
                Some("That shortcut is reserved by macOS or a standard text action.".into());
            cx.notify();
            return;
        }

        if let Some(conflict) = keymap::shortcuts().into_iter().find(|shortcut| {
            shortcut.id != id
                && shortcut
                    .keystroke(&self.shortcut_overrides)
                    .and_then(keymap::normalized_keystroke)
                    .is_some_and(|existing| existing == normalized)
        }) {
            self.shortcut_error = Some(format!(
                "{} is already used by {}.",
                keymap::display_keystroke(&normalized),
                conflict.title
            ));
            cx.notify();
            return;
        }

        let previous = self.shortcut_overrides.clone();
        let is_default = keymap::shortcuts()
            .into_iter()
            .find(|shortcut| shortcut.id == id)
            .and_then(|shortcut| shortcut.default_keystroke)
            .and_then(keymap::normalized_keystroke)
            .is_some_and(|default| default == normalized);
        if is_default {
            self.shortcut_overrides.remove(&id);
        } else {
            self.shortcut_overrides.insert(id, normalized);
        }
        self.recording_shortcut = None;
        self.shortcut_error = None;
        self.persist_shortcut_overrides(previous, cx);
    }

    fn load_process_snapshot(projects: &[ProjectSource]) -> ProcessSnapshot {
        let root_pid = std::process::id() as i32;
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,rss=,%cpu=,command="])
            .output();
        let Ok(output) = output else {
            return ProcessSnapshot {
                root_pid,
                error: Some("Unable to read process list".into()),
                ..Default::default()
            };
        };
        if !output.status.success() {
            return ProcessSnapshot {
                root_pid,
                error: Some("ps command failed".into()),
                ..Default::default()
            };
        }

        #[derive(Clone)]
        struct RawProcess {
            pid: i32,
            rss_kb: u64,
            cpu: f64,
            command: String,
        }

        let mut table: HashMap<i32, RawProcess> = HashMap::new();
        let mut children: HashMap<i32, Vec<i32>> = HashMap::new();
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let mut parts = line.split_whitespace();
            let (Some(pid), Some(ppid), Some(rss), Some(cpu)) =
                (parts.next(), parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let Ok(pid) = pid.parse::<i32>() else {
                continue;
            };
            let Ok(ppid) = ppid.parse::<i32>() else {
                continue;
            };
            let rss_kb = rss.parse::<u64>().unwrap_or(0);
            let cpu = cpu.parse::<f64>().unwrap_or(0.0);
            let command = parts.collect::<Vec<_>>().join(" ");
            children.entry(ppid).or_default().push(pid);
            table.insert(
                pid,
                RawProcess {
                    pid,
                    rss_kb,
                    cpu,
                    command,
                },
            );
        }

        let Some(root) = table.get(&root_pid).cloned() else {
            return ProcessSnapshot {
                root_pid,
                error: Some("Current app process was not found in ps output".into()),
                ..Default::default()
            };
        };

        let mut stack = vec![root_pid];
        let mut tree = Vec::new();
        while let Some(pid) = stack.pop() {
            if let Some(process) = table.get(&pid) {
                tree.push(process.clone());
            }
            if let Some(kids) = children.get(&pid) {
                stack.extend(kids.iter().copied());
            }
        }

        let total_kb: u64 = tree.iter().map(|p| p.rss_kb).sum();
        let total_cpu: f64 = tree.iter().map(|p| p.cpu).sum();
        let mut top: Vec<ProcessInfo> = tree
            .iter()
            .map(|p| {
                let cwd = Self::process_cwd(p.pid);
                let command = if p.command.is_empty() {
                    format!("pid {}", p.pid)
                } else {
                    p.command.clone()
                };
                ProcessInfo {
                    pid: p.pid,
                    rss_kb: p.rss_kb,
                    cpu: p.cpu,
                    project: Self::infer_project(projects, cwd.as_deref(), &command),
                    command,
                }
            })
            .collect();
        top.sort_by_key(|process| std::cmp::Reverse(process.rss_kb));
        top.truncate(8);

        ProcessSnapshot {
            root_pid,
            app_mb: root.rss_kb as f64 / 1024.0,
            total_mb: total_kb as f64 / 1024.0,
            total_cpu,
            process_count: tree.len(),
            top,
            error: None,
        }
    }

    fn format_mb(mb: f64) -> String {
        if mb >= 1024.0 {
            format!("{:.2} GB", mb / 1024.0)
        } else {
            format!("{mb:.0} MB")
        }
    }

    fn process_cwd(pid: i32) -> Option<String> {
        let output = Command::new("lsof")
            .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .find_map(|line| line.strip_prefix('n').map(|path| path.to_string()))
    }

    fn infer_project(
        projects: &[ProjectSource],
        cwd: Option<&str>,
        command: &str,
    ) -> Option<String> {
        projects
            .iter()
            .filter(|project| {
                cwd.is_some_and(|cwd| cwd.starts_with(&project.path))
                    || command.contains(&project.path)
            })
            .max_by_key(|project| project.path.len())
            .map(|project| project.name.clone())
    }

    fn refresh_process_snapshot(&mut self, cx: &mut Context<Self>) {
        self.process_loading = true;
        self.process_seq = self.process_seq.wrapping_add(1);
        let seq = self.process_seq;
        let projects = self.projects.clone();

        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move { Self::load_process_snapshot(&projects) })
                .await;
            this.update(cx, |this, cx| {
                if this.process_seq == seq {
                    this.process_snapshot = snapshot;
                    this.process_loading = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn refresh_agent_skills(&mut self, cx: &mut Context<Self>) {
        self.skills_loading = true;
        self.skills_error = None;
        self.skills_seq = self.skills_seq.wrapping_add(1);
        let seq = self.skills_seq;
        let cwd = self.skills_cwd.clone();

        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move { Self::load_agent_skills(&cwd) })
                .await;
            this.update(cx, |this, cx| {
                if this.skills_seq == seq {
                    match snapshot {
                        Ok(cache) => {
                            this.skills = cache.capabilities;
                            this.skills_last_refreshed = cache.refreshed_at;
                            if !cache.cwd.is_empty() {
                                this.skills_cwd = cache.cwd;
                            }
                            this.skills_error = None;
                        }
                        Err(error) => {
                            this.skills_error = Some(error);
                        }
                    }
                    this.skills_loading = false;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn load_agent_skills(cwd: &str) -> Result<AgentCapabilityCacheFile, String> {
        AgentCapabilityCacheFile::refresh_from_runtime(cwd)
    }

    fn open_riff_editor(
        &mut self,
        riff: Option<ChoroRiff>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = riff.as_ref().map(|riff| riff.id);
        let name = riff
            .as_ref()
            .map(|riff| riff.name.clone())
            .unwrap_or_default();
        let description = riff
            .as_ref()
            .and_then(|riff| riff.description.clone())
            .unwrap_or_default();
        let instructions = riff
            .as_ref()
            .map(|riff| riff.instructions.clone())
            .unwrap_or_default();
        let name_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Riff name")
                .default_value(name)
        });
        let description_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("A short description shown in the / menu")
                .default_value(description)
        });
        let instructions_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Write the complete workflow and guidance for this Riff…")
                .default_value(instructions)
                .multi_line(true)
                .rows(12)
        });
        for input in [&name_input, &description_input, &instructions_input] {
            cx.subscribe(input, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
        }
        self.riff_editor = Some(RiffEditor {
            session_id: Uuid::new_v4(),
            id,
            name: name_input,
            description: description_input,
            instructions: instructions_input,
            generating: false,
            error: None,
        });
        self.riffs_status = None;
        cx.notify();
    }

    fn generate_riff_instructions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.riff_editor.as_ref() else {
            return;
        };
        if editor.generating || !editor.instructions.read(cx).value().trim().is_empty() {
            return;
        }

        let name = editor.name.read(cx).value().trim().to_string();
        let description = editor.description.read(cx).value().trim().to_string();
        if name.is_empty() || description.is_empty() {
            if let Some(editor) = self.riff_editor.as_mut() {
                editor.error =
                    Some("Add a Riff name and description before generating instructions.".into());
            }
            cx.notify();
            return;
        }

        let session_id = editor.session_id;
        let instructions = editor.instructions.clone();
        let working_directory = PathBuf::from(self.skills_cwd.clone());
        let generation_agent = self.workspace.read(cx).generation_agent.clone();
        let window_handle = window.window_handle();
        if let Some(editor) = self.riff_editor.as_mut() {
            editor.generating = true;
            editor.error = None;
        }
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::ui::git::git_panel::generate_riff(
                        &generation_agent,
                        &working_directory,
                        &name,
                        &description,
                    )
                })
                .await;

            this.update(cx, |settings, cx| {
                let Some(editor) = settings
                    .riff_editor
                    .as_mut()
                    .filter(|editor| editor.session_id == session_id)
                else {
                    return;
                };
                editor.generating = false;
                match result {
                    Ok(generated) => {
                        let generated = generated.trim().to_string();
                        let applied = window_handle
                            .update(cx, |_, window, cx| {
                                if !instructions.read(cx).value().trim().is_empty() {
                                    return false;
                                }
                                instructions.update(cx, |input, cx| {
                                    input.set_value(generated, window, cx);
                                });
                                true
                            })
                            .unwrap_or(false);
                        if applied {
                            crate::notifications::play_generated_sound();
                        }
                    }
                    Err(error) => {
                        editor.error = Some(format!("Could not generate Riff: {error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn save_riff_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.riff_editor.as_ref() else {
            return;
        };
        let name = editor.name.read(cx).value().trim().to_string();
        let description = editor.description.read(cx).value().trim().to_string();
        let instructions = editor.instructions.read(cx).value().trim().to_string();
        let id = editor.id;
        let error = if name.is_empty() {
            Some("Give this Riff a name.".to_string())
        } else if instructions.is_empty() {
            Some("Add instructions for the agent.".to_string())
        } else if self.riffs.iter().any(|riff| {
            riff.id != id.unwrap_or(Uuid::nil()) && riff.name.eq_ignore_ascii_case(&name)
        }) {
            Some("A Riff with this name already exists.".to_string())
        } else {
            None
        };
        if let Some(error) = error {
            if let Some(editor) = self.riff_editor.as_mut() {
                editor.error = Some(error);
            }
            cx.notify();
            return;
        }

        let previous_riffs = self.riffs.clone();
        if let Some(existing) = id.and_then(|id| self.riffs.iter_mut().find(|riff| riff.id == id)) {
            existing.name = name;
            existing.description = (!description.is_empty()).then_some(description);
            existing.instructions = instructions;
        } else {
            self.riffs.push(ChoroRiff {
                id: Uuid::new_v4(),
                name,
                description: (!description.is_empty()).then_some(description),
                instructions,
                enabled: true,
            });
        }
        self.riffs.sort_by(|left, right| {
            left.name
                .to_ascii_lowercase()
                .cmp(&right.name.to_ascii_lowercase())
        });
        match (ChoroRiffStore {
            schema_version: CHORO_RIFFS_SCHEMA_VERSION,
            riffs: self.riffs.clone(),
        })
        .save()
        {
            Ok(()) => {
                self.riff_editor = None;
                self.riffs_status =
                    Some("Riff saved. It is now available in every project.".into());
            }
            Err(error) => {
                self.riffs = previous_riffs;
                if let Some(editor) = self.riff_editor.as_mut() {
                    editor.error = Some(format!("Could not save Riff: {error:#}"));
                }
            }
        }
        cx.notify();
    }

    fn toggle_riff(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let previous_riffs = self.riffs.clone();
        if let Some(riff) = self.riffs.iter_mut().find(|riff| riff.id == id) {
            riff.enabled = !riff.enabled;
        }
        self.persist_riffs("Riff availability updated.", previous_riffs, cx);
    }

    fn delete_riff(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let previous_riffs = self.riffs.clone();
        self.riffs.retain(|riff| riff.id != id);
        if self.persist_riffs("Riff deleted.", previous_riffs, cx) {
            self.riff_editor = None;
        }
    }

    fn persist_riffs(
        &mut self,
        success: &str,
        previous_riffs: Vec<ChoroRiff>,
        cx: &mut Context<Self>,
    ) -> bool {
        let store = ChoroRiffStore {
            schema_version: CHORO_RIFFS_SCHEMA_VERSION,
            riffs: self.riffs.clone(),
        };
        let saved = match store.save() {
            Ok(()) => {
                self.riffs_status = Some(success.to_string());
                true
            }
            Err(error) => {
                self.riffs = previous_riffs;
                self.riffs_status = Some(format!("Could not save Riffs: {error:#}"));
                false
            }
        };
        cx.notify();
        saved
    }

    fn export_workspace(&mut self, cx: &mut Context<Self>) {
        if self.data_busy {
            return;
        }
        self.data_busy = true;
        self.data_status = Some("Exporting workspace...".into());
        let target = default_export_path();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let target = target.clone();
                    async move {
                        LocalStore::open_default()
                            .and_then(|store| store.export_workspace(&target))
                            .map(|_| target)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.data_busy = false;
                this.data_status = Some(match result {
                    Ok(path) => format!("Exported {}", path.display()),
                    Err(error) => format!("Export failed: {error:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn import_workspace(&mut self, cx: &mut Context<Self>) {
        if self.data_busy {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this, cx| {
            let selected = receiver.await;
            let archive = match selected {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(archive) = archive else {
                return;
            };
            this.update(cx, |this, cx| {
                this.data_busy = true;
                this.data_status = Some("Importing workspace...".into());
                cx.notify();
            })
            .ok();
            let result = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default()
                        .and_then(|store| store.import_workspace_replace(&archive))
                })
                .await;
            this.update(cx, |this, cx| {
                this.data_busy = false;
                this.data_status = Some(match result {
                    Ok(backup) => format!(
                        "Imported workspace. Backup: {}. Restart the app to reload all views.",
                        backup.display()
                    ),
                    Err(error) => format!("Import failed: {error:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn section_button(
        id: &'static str,
        target: SettingsSection,
        selected: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = selected == target;
        h_flex()
            .id(id)
            .w_full()
            .h(px(34.))
            .px_2()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .text_size(crate::ui::design::text_ui())
            .text_color(if is_selected {
                crate::ui::design::t1(cx)
            } else {
                crate::ui::design::t2(cx)
            })
            .when(is_selected, |row| {
                row.bg(crate::ui::design::accent_soft(cx))
                    .border_1()
                    .border_color(crate::ui::design::accent_line(cx))
            })
            .when(!is_selected, |row| {
                row.border_1()
                    .border_color(crate::ui::design::base(cx).opacity(0.0))
                    .hover(|row| row.bg(crate::ui::design::hover(cx)))
            })
            .child(
                Icon::new(IconName::ChevronRight)
                    .size(crate::ui::design::icon_sm())
                    .text_color(if is_selected {
                        crate::ui::design::accent(cx)
                    } else {
                        crate::ui::design::t4(cx)
                    }),
            )
            .child(target.title())
            .on_click(cx.listener(move |this, _, _, cx| {
                this.section = target;
                this.recording_shortcut = None;
                this.shortcut_error = None;
                cx.notify();
            }))
    }

    fn render_shortcut_row(
        &self,
        shortcut: keymap::Shortcut,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let recording = self.recording_shortcut.as_deref() == Some(shortcut.id);
        let modified = shortcut.is_modified(&self.shortcut_overrides);
        let label = if recording {
            "Press shortcut…".to_string()
        } else {
            shortcut
                .keystroke(&self.shortcut_overrides)
                .map(keymap::display_keystroke)
                .unwrap_or_else(|| "Not set".to_string())
        };
        let id = shortcut.id.to_string();
        let reset_id = shortcut.id.to_string();

        h_flex()
            .w_full()
            .min_h(px(54.))
            .px_3()
            .gap_3()
            .items_center()
            .border_t_1()
            .border_color(crate::ui::design::line(cx).opacity(0.55))
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(shortcut.title),
                            )
                            .when(modified, |title| {
                                title.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded(crate::ui::design::r_xs())
                                        .bg(crate::ui::design::accent_soft(cx))
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::accent(cx))
                                        .child("Modified"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(shortcut.description),
                    ),
            )
            .child(
                crate::ui::style::shortcut_key_button(
                    SharedString::from(format!("shortcut-recorder-{}", shortcut.id)),
                    label,
                    recording,
                    cx,
                )
                .min_w(px(116.))
                .tooltip(if recording {
                    "Press a shortcut, Esc to cancel, or Delete to unassign"
                } else {
                    "Click, then press the new shortcut"
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.begin_shortcut_recording(id.clone(), window, cx);
                })),
            )
            .when(modified, |row| {
                row.child(
                    crate::ui::style::icon_button(
                        SharedString::from(format!("shortcut-reset-{}", shortcut.id)),
                        IconName::Undo2,
                        cx,
                    )
                    .tooltip("Restore default")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.reset_shortcut(&reset_id, cx);
                    })),
                )
            })
            .into_any_element()
    }

    fn render_shortcuts(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let query = self.shortcut_search.read(cx).value().trim().to_lowercase();
        let mut groups = Vec::new();
        for category in keymap::ShortcutCategory::ALL {
            let shortcuts = keymap::shortcuts()
                .into_iter()
                .filter(|shortcut| shortcut.category == category)
                .filter(|shortcut| {
                    !self.shortcut_modified_only || shortcut.is_modified(&self.shortcut_overrides)
                })
                .filter(|shortcut| {
                    query.is_empty()
                        || shortcut.title.to_lowercase().contains(&query)
                        || shortcut.description.to_lowercase().contains(&query)
                        || category.title().to_lowercase().contains(&query)
                })
                .collect::<Vec<_>>();
            if shortcuts.is_empty() {
                continue;
            }
            groups.push(
                v_flex()
                    .w_full()
                    .overflow_hidden()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .child(
                        h_flex().h(px(38.)).px_3().items_center().child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t2(cx))
                                .child(category.title()),
                        ),
                    )
                    .children(
                        shortcuts
                            .into_iter()
                            .map(|shortcut| self.render_shortcut_row(shortcut, cx)),
                    )
                    .into_any_element(),
            );
        }

        if !self.shortcut_modified_only {
            let contextual = [
                (
                    "Find in current view",
                    "Standard search inside the focused view",
                    "⌘F",
                ),
                ("Send or continue", "Send from an agent composer", "↩"),
                ("New line", "Insert a line break in an agent composer", "⇧↩"),
                (
                    "Start or steer agent",
                    "Start a draft agent or steer the selected running agent",
                    "⌘↩",
                ),
            ]
            .into_iter()
            .filter(|(title, description, _)| {
                query.is_empty()
                    || title.to_lowercase().contains(&query)
                    || description.to_lowercase().contains(&query)
                    || "contextual essentials".contains(&query)
            })
            .collect::<Vec<_>>();
            if !contextual.is_empty() {
                groups.push(
                    v_flex()
                        .w_full()
                        .overflow_hidden()
                        .rounded(crate::ui::design::r_lg())
                        .border_1()
                        .border_color(crate::ui::design::line_2(cx))
                        .bg(crate::ui::design::surface(cx).opacity(0.55))
                        .child(
                            h_flex().h(px(38.)).px_3().items_center().child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Contextual essentials"),
                            ),
                        )
                        .children(contextual.into_iter().map(|(title, description, keys)| {
                            h_flex()
                                .w_full()
                                .min_h(px(54.))
                                .px_3()
                                .gap_3()
                                .items_center()
                                .border_t_1()
                                .border_color(crate::ui::design::line(cx).opacity(0.55))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .gap_0p5()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::MEDIUM)
                                                .child(title),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(description),
                                        ),
                                )
                                .child(
                                    div()
                                        .min_w(px(116.))
                                        .px_2()
                                        .py_1()
                                        .rounded(crate::ui::design::r_xs())
                                        .border_1()
                                        .border_color(crate::ui::design::line_2(cx))
                                        .bg(crate::ui::design::base(cx))
                                        .text_center()
                                        .font_family(crate::ui::design::FONT_MONO)
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(keys),
                                )
                        }))
                        .into_any_element(),
                );
            }
        }

        v_flex()
            .track_focus(&self.shortcut_focus)
            .on_key_down(cx.listener(Self::capture_shortcut))
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.shortcut_search).prefix(IconName::Search)),
                    )
                    .child(
                        if self.shortcut_modified_only {
                            crate::ui::style::ghost_button_compact("shortcuts-filter-all", "All")
                        } else {
                            crate::ui::style::secondary_button_compact(
                                "shortcuts-filter-all",
                                "All",
                            )
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shortcut_modified_only = false;
                            cx.notify();
                        })),
                    )
                    .child(
                        if self.shortcut_modified_only {
                            crate::ui::style::secondary_button_compact(
                                "shortcuts-filter-modified",
                                "Modified",
                            )
                        } else {
                            crate::ui::style::ghost_button_compact(
                                "shortcuts-filter-modified",
                                "Modified",
                            )
                        }
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.shortcut_modified_only = true;
                            cx.notify();
                        })),
                    )
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "shortcuts-reset-all",
                            "Reset all",
                            cx,
                        )
                        .disabled(self.shortcut_overrides.is_empty())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.reset_all_shortcuts(cx);
                        })),
                    ),
            )
            .when_some(self.shortcut_error.clone(), |view, error| {
                view.child(
                    h_flex()
                        .w_full()
                        .px_3()
                        .py_2()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.45))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .when(groups.is_empty(), |view| {
                view.child(
                    div()
                        .w_full()
                        .p_6()
                        .text_center()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No shortcuts match this view."),
                )
            })
            .children(groups)
            .child(
                div()
                    .pb_2()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child("Click a keycap and press the new shortcut. Choro blocks duplicates and macOS-reserved combinations. Press Delete while recording to leave an action unassigned."),
            )
            .into_any_element()
    }

    fn page_header(section: SettingsSection, cx: &App) -> impl IntoElement {
        v_flex()
            .w_full()
            .gap_1()
            .pb_2()
            .child(
                div()
                    .text_size(crate::ui::design::text_title())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child(section.title()),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(section.description()),
            )
    }

    fn render_design_section(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let penpot = self.penpot.read(cx);
        let status = penpot.status().clone();
        let active_provider = penpot.provider();
        let pending = matches!(
            status,
            PenpotConnectionStatus::Provisioning | PenpotConnectionStatus::Checking
        );
        let selected_provider_is_active = active_provider == self.design_provider;
        let status_text = match (selected_provider_is_active, &status, self.design_provider) {
            (false, _, DesignProvider::Choro) => {
                "Penpot Cloud stays active until you choose Use Choro Design.".to_string()
            }
            (false, _, DesignProvider::PenpotCloud) => {
                "Choro Design stays active until you save and connect the cloud account."
                    .to_string()
            }
            (true, PenpotConnectionStatus::NotChecked, _) => {
                "Connection has not been checked.".to_string()
            }
            (true, PenpotConnectionStatus::Provisioning, _) => {
                "Preparing your Choro Design workspace…".to_string()
            }
            (true, PenpotConnectionStatus::Checking, _) => {
                "Checking the Design connection…".to_string()
            }
            (true, PenpotConnectionStatus::Reachable, _) => "Design is connected.".to_string(),
            (true, PenpotConnectionStatus::Error(error), _) => error.clone(),
        };
        let status_color = if !selected_provider_is_active {
            crate::ui::design::t4(cx)
        } else {
            match status {
                PenpotConnectionStatus::Reachable => crate::ui::design::sage(cx),
                PenpotConnectionStatus::Error(_) => crate::ui::design::rose(cx),
                PenpotConnectionStatus::Provisioning | PenpotConnectionStatus::Checking => {
                    crate::ui::design::amber(cx)
                }
                PenpotConnectionStatus::NotChecked => crate::ui::design::t4(cx),
            }
        };
        let managed_selected = self.design_provider == DesignProvider::Choro;
        let cloud_selected = self.design_provider == DesignProvider::PenpotCloud;
        let instance_input = self.design_instance_input.clone();
        let mcp_input = self.design_mcp_input.clone();
        let access_token_input = self.design_access_token_input.clone();
        let mcp_key_input = self.design_mcp_key_input.clone();

        v_flex()
            .w_full()
            .gap_4()
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::surface(cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Design provider"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        "Choro Design is automatic. Penpot Cloud uses your own account and credentials.",
                                    ),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                if managed_selected {
                                    crate::ui::style::accent_button_compact(
                                        "settings-design-provider-choro",
                                        DesignProvider::Choro.label(),
                                        cx,
                                    )
                                } else {
                                    crate::ui::style::secondary_button_compact(
                                        "settings-design-provider-choro",
                                        DesignProvider::Choro.label(),
                                    )
                                }
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.design_provider = DesignProvider::Choro;
                                    this.design_error = None;
                                    cx.notify();
                                })),
                            )
                            .child(
                                if cloud_selected {
                                    crate::ui::style::accent_button_compact(
                                        "settings-design-provider-cloud",
                                        DesignProvider::PenpotCloud.label(),
                                        cx,
                                    )
                                } else {
                                    crate::ui::style::secondary_button_compact(
                                        "settings-design-provider-cloud",
                                        DesignProvider::PenpotCloud.label(),
                                    )
                                }
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.design_provider = DesignProvider::PenpotCloud;
                                    this.design_error = None;
                                    cx.notify();
                                })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_3()
                    .p_4()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::surface(cx))
                    .when(managed_selected, |card| {
                        card.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child("Managed by Choro"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(
                                            "No signup, URLs, or tokens are required. Choro creates and reconnects the workspace for this installation.",
                                        ),
                                ),
                        )
                        .child(
                            if pending {
                                crate::ui::style::busy_button_compact(
                                    "settings-design-use-managed",
                                    "Connecting",
                                    cx,
                                )
                            } else {
                                crate::ui::style::primary_button_compact(
                                    "settings-design-use-managed",
                                    "Use Choro Design",
                                    cx,
                                )
                                .on_click({
                                    let penpot = self.penpot.clone();
                                    move |_, _, cx| {
                                        penpot.update(cx, |penpot, cx| {
                                            penpot.switch_to_managed(cx)
                                        });
                                    }
                                })
                            },
                        )
                    })
                    .when(cloud_selected, |card| {
                        card.child(
                            v_flex()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child("Connect Penpot Cloud"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(
                                            "Use the service URL, copied MCP URL, personal access token, and MCP key from your Penpot account.",
                                        ),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("Penpot URL", cx))
                                .child(Input::new(&self.design_instance_input).w_full()),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("MCP server URL", cx))
                                .child(Input::new(&self.design_mcp_input).w_full()),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("Personal access token", cx))
                                .child(
                                    Input::new(&self.design_access_token_input)
                                        .w_full()
                                        .mask_toggle(),
                                ),
                        )
                        .child(
                            v_flex()
                                .gap_1()
                                .child(penpot_field_label("MCP key", cx))
                                .child(
                                    Input::new(&self.design_mcp_key_input)
                                        .w_full()
                                        .mask_toggle(),
                                ),
                        )
                        .child(
                            crate::ui::style::primary_button_compact(
                                "settings-design-connect-cloud",
                                "Save & Connect",
                                cx,
                            )
                            .icon(IconName::Globe)
                            .disabled(pending)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let instance = instance_input.read(cx).value().to_string();
                                let mcp = mcp_input.read(cx).value().to_string();
                                let access_token =
                                    access_token_input.read(cx).value().to_string();
                                let key = mcp_key_input.read(cx).value().to_string();
                                let result = this.penpot.update(cx, |penpot, cx| {
                                    penpot.save_connection(
                                        &instance,
                                        &mcp,
                                        &key,
                                        &access_token,
                                        cx,
                                    )
                                });
                                match result {
                                    Ok(()) => {
                                        this.design_error = None;
                                        this.penpot.update(cx, |penpot, cx| {
                                            penpot.test_connection(cx)
                                        });
                                    }
                                    Err(error) => {
                                        this.design_error = Some(error.to_string());
                                    }
                                }
                                cx.notify();
                            })),
                        )
                    }),
            )
            .when_some(self.design_error.clone(), |view, error| {
                view.child(
                    div()
                        .w_full()
                        .p_3()
                        .rounded(crate::ui::design::r_md())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.4))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::design::indicator::dot(status_color))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(status_text),
                    ),
            )
            .into_any_element()
    }
}

fn penpot_field_label(label: &'static str, cx: &App) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(FontWeight::MEDIUM)
        .text_color(crate::ui::design::t2(cx))
        .child(label)
        .into_any_element()
}

fn default_export_path() -> PathBuf {
    let dir = dirs::download_dir()
        .or_else(dirs::desktop_dir)
        .unwrap_or_else(|| {
            LocalStore::open_default()
                .map(|store| store.root().to_path_buf())
                .unwrap_or_else(|_| PathBuf::from("."))
        });
    dir.join(format!("choro-workspace-export-{}.zip", unix_now_secs()))
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn format_remote_time(timestamp: u64) -> String {
    let elapsed = unix_now_secs().saturating_sub(timestamp);
    match elapsed {
        0..=59 => "just now".into(),
        60..=3599 => format!("{}m ago", elapsed / 60),
        3600..=86_399 => format!("{}h ago", elapsed / 3600),
        seconds => format!("{}d ago", seconds / 86_400),
    }
}

fn format_remote_expiry(timestamp: u64) -> String {
    let remaining = timestamp.saturating_sub(unix_now_secs());
    match remaining {
        0 => "expired".into(),
        1..=3_599 => format!("in {}m", (remaining / 60).max(1)),
        3_600..=86_399 => format!("in {}h", remaining / 3_600),
        seconds => format!("in {}d", seconds / 86_400),
    }
}

impl Render for SettingsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = self.process_snapshot.clone();
        let section = self.section;
        let process_loading = self.process_loading;
        let settings_query = self.settings_search.read(cx).value().trim().to_lowercase();
        let current_theme = self
            .workspace
            .read(cx)
            .theme_name
            .clone()
            .unwrap_or_else(|| crate::theme::SIGNATURE_THEME.to_string());
        let conversation_layout = self.workspace.read(cx).conversation_layout;
        let generation_agent = self.workspace.read(cx).generation_agent.clone();
        let agent_defaults = self.workspace.read(cx).new_agent_defaults();
        let verification_mode = self.workspace.read(cx).verification_mode;
        let code_review_prompt = self.code_review_prompt.clone();
        let code_review_prompt_dirty = self.code_review_prompt_dirty;
        let code_review_prompt_empty = code_review_prompt.read(cx).value().trim().is_empty();
        h_flex()
            .h_full()
            .w_full()
            .child(
                v_flex()
                    .w(px(236.))
                    .h_full()
                    .flex_none()
                    .gap_1()
                    .p_3()
                    .border_r_1()
                    .border_color(crate::ui::design::line(cx))
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        div()
                            .w_full()
                            .mb_2()
                            .child(
                                Input::new(&self.settings_search)
                                    .prefix(IconName::Search),
                            ),
                    )
                    .when(SettingsSection::Appearance.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-appearance-section",
                            SettingsSection::Appearance,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Design.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-design-section",
                            SettingsSection::Design,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Generation.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-generation-section",
                            SettingsSection::Generation,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Shortcuts.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-shortcuts-section",
                            SettingsSection::Shortcuts,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::AgentSkills.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-agent-skills-section",
                            SettingsSection::AgentSkills,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Memory.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-memory-section",
                            SettingsSection::Memory,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Remote.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-remote-section",
                            SettingsSection::Remote,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Data.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-data-section",
                            SettingsSection::Data,
                            section,
                            cx,
                        ),
                    ))
                    .when(SettingsSection::Process.matches(&settings_query), |nav| nav.child(
                        Self::section_button(
                            "settings-process-section",
                            SettingsSection::Process,
                            section,
                            cx,
                        ),
                    ))
                    .child(div().flex_1())
                    .child(
                        div()
                            .px_2()
                            .py_2()
                            .border_t_1()
                            .border_color(crate::ui::design::line(cx))
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child("CHORO SETTINGS"),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .id("settings-content-scroll")
                    .when(section == SettingsSection::AgentSkills, |content| {
                        content.overflow_hidden()
                    })
                    .when(section != SettingsSection::AgentSkills, |content| {
                        content.overflow_y_scroll()
                    })
                    .child(
                        v_flex()
                            .w_full()
                            .mx_auto()
                            .when(section == SettingsSection::AgentSkills, |page| {
                                page.h_full()
                                    .min_h(px(0.))
                                    .max_w(px(960.))
                                    .gap_3()
                                    .px_6()
                                    .py_5()
                            })
                            .when(section != SettingsSection::AgentSkills, |page| {
                                page.max_w(px(860.)).gap_5().px_8().py_7()
                            })
                            .child(Self::page_header(section, cx))
                            .child(match section {
                SettingsSection::Design => self.render_design_section(cx),
                SettingsSection::Memory => self.render_memory_section(cx),
                SettingsSection::Generation => {
                    let verification_buttons = [
                        (
                            VerificationMode::Ask,
                            "Ask every time",
                            "Offer verification after eligible work finishes.",
                        ),
                        (
                            VerificationMode::Automatic,
                            "Automatic",
                            "Start verification immediately without asking.",
                        ),
                        (
                            VerificationMode::Off,
                            "Off",
                            "Do not offer or start verification automatically.",
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (mode, label, tooltip))| {
                        let button = if verification_mode == mode {
                            crate::ui::style::primary_button_compact(
                                ("settings-verification-mode", index),
                                label,
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-verification-mode", index),
                                label,
                                cx,
                            )
                        };
                        button
                            .tooltip(tooltip)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.workspace.update(cx, |workspace, cx| {
                                    workspace.set_verification_mode(mode, cx);
                                });
                                cx.notify();
                            }))
                    })
                    .collect::<Vec<_>>();
                    let provider_buttons = [
                        AgentKind::Codex,
                        AgentKind::Claude,
                        AgentKind::OpenCode,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, provider)| {
                        let button = if generation_agent.provider == provider {
                            crate::ui::style::primary_button_compact(
                                ("settings-generation-provider", index),
                                provider.label(),
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-generation-provider", index),
                                provider.label(),
                                cx,
                            )
                        };
                        button
                            .icon(crate::ui::center::provider_brand_icon(provider))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_generation_provider(provider, cx);
                            }))
                    })
                    .collect::<Vec<_>>();
                    let model_buttons = if generation_agent.provider == AgentKind::OpenCode {
                        vec![(AgentModel::OpenCode, "Big Pickle".to_string())]
                    } else {
                        AgentModel::models_for(generation_agent.provider)
                            .iter()
                            .map(|model| (*model, model.menu_label().to_string()))
                            .collect::<Vec<_>>()
                    }
                    .into_iter()
                    .enumerate()
                    .map(|(index, (model, label))| {
                        let selected = generation_agent.model == model;
                        let button = if selected {
                            crate::ui::style::primary_button_compact(
                                ("settings-generation-model", index),
                                label,
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-generation-model", index),
                                label,
                                cx,
                            )
                        };
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.select_generation_model(model, cx);
                        }))
                    })
                    .collect::<Vec<_>>();
                    let default_provider_buttons = [
                        AgentKind::Codex,
                        AgentKind::Claude,
                        AgentKind::OpenCode,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, provider)| {
                        let button = if agent_defaults.provider == provider {
                            crate::ui::style::primary_button_compact(
                                ("settings-agent-default-provider", index),
                                provider.label(),
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-agent-default-provider", index),
                                provider.label(),
                                cx,
                            )
                        };
                        button
                            .icon(crate::ui::center::provider_brand_icon(provider))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.select_default_agent_provider(provider, cx);
                            }))
                    })
                    .collect::<Vec<_>>();
                    let default_model_buttons = if agent_defaults.provider == AgentKind::OpenCode {
                        vec![(AgentModel::OpenCode, "Big Pickle".to_string())]
                    } else {
                        AgentModel::models_for(agent_defaults.provider)
                            .iter()
                            .map(|model| (*model, model.menu_label().to_string()))
                            .collect::<Vec<_>>()
                    }
                    .into_iter()
                    .enumerate()
                    .map(|(index, (model, label))| {
                        let button = if agent_defaults.model == model {
                            crate::ui::style::primary_button_compact(
                                ("settings-agent-default-model", index),
                                label,
                                cx,
                            )
                        } else {
                            crate::ui::style::dialog_neutral_button(
                                ("settings-agent-default-model", index),
                                label,
                                cx,
                            )
                        };
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.select_default_agent_model(model, cx);
                        }))
                    })
                    .collect::<Vec<_>>();
                    let default_effort_buttons = AgentEffort::ALL
                        .into_iter()
                        .enumerate()
                        .map(|(index, effort)| {
                            let button = if agent_defaults.effort == effort {
                                crate::ui::style::primary_button_compact(
                                    ("settings-agent-default-effort", index),
                                    effort.label(),
                                    cx,
                                )
                            } else {
                                crate::ui::style::dialog_neutral_button(
                                    ("settings-agent-default-effort", index),
                                    effort.label(),
                                    cx,
                                )
                            };
                            button.on_click(cx.listener(move |this, _, _, cx| {
                                this.select_default_agent_effort(effort, cx);
                            }))
                        })
                        .collect::<Vec<_>>();
                    let label_size = crate::ui::design::text_label();
                    let label_color = crate::ui::design::t4(cx);
                    let row_label = move |label: &'static str| {
                        div()
                            .text_size(label_size)
                            .text_color(label_color)
                            .child(label)
                    };

                    v_flex()
                        .w_full()
                        .gap_4()
                        .child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .p_4()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.55))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("New agents"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("What a fresh agent starts with. Change any of it per agent in the composer."),
                                )
                                .child(row_label("Provider"))
                                .child(h_flex().w_full().gap_2().flex_wrap().children(default_provider_buttons))
                                .child(row_label("Model"))
                                .child(h_flex().w_full().gap_2().flex_wrap().children(default_model_buttons))
                                .child(row_label("Effort"))
                                .child(h_flex().w_full().gap_2().flex_wrap().children(default_effort_buttons)),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .p_4()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.55))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Quick generation"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(format!(
                                            "Commit messages, PR titles and bodies, Riff instructions — fast one-shot calls, not chat agents. Currently {} · {}.",
                                            generation_agent.provider.label(),
                                            generation_agent.model_label()
                                        )),
                                )
                                .child(row_label("Provider"))
                                .child(h_flex().w_full().gap_2().flex_wrap().children(provider_buttons))
                                .child(row_label("Model"))
                                .child(h_flex().w_full().gap_2().flex_wrap().children(model_buttons)),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .p_4()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.55))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Work verification"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Verification is an extra agent pass and can use significant time and tokens."),
                                )
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_2()
                                        .flex_wrap()
                                        .children(verification_buttons),
                                ),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Code review prompt"),
                                )
                                .child(
                                    Input::new(&code_review_prompt)
                                        .w_full()
                                        .h(px(280.))
                                        .flex_none(),
                                )
                                .child(
                                    h_flex()
                                        .w_full()
                                        .justify_between()
                                        .child(
                                            crate::ui::style::dialog_neutral_button(
                                                "restore-default-code-review-prompt",
                                                "Restore default",
                                                cx,
                                            )
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.restore_default_code_review_prompt(window, cx);
                                            })),
                                        )
                                        .child(
                                            crate::ui::style::primary_button_compact(
                                                "save-code-review-prompt",
                                                "Save prompt",
                                                cx,
                                            )
                                            .disabled(
                                                !code_review_prompt_dirty
                                                    || code_review_prompt_empty,
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.save_code_review_prompt(cx);
                                            })),
                                        ),
                                ),
                        )
                        .into_any_element()
                }
                SettingsSection::Appearance => v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .p_4()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::surface(cx).opacity(0.55))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Theme"),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .flex_wrap()
                                    .children(
                                        crate::theme::available_themes(cx)
                                            .into_iter()
                                            .enumerate()
                                            .map(|(index, name)| {
                                                let selected = name == current_theme;
                                                let label = name.clone();
                                                Button::new(("settings-theme-option", index))
                                                    .small()
                                                    .h(crate::ui::design::control_h())
                                                    .text_size(crate::ui::design::text_ui())
                                                    .outline()
                                                    .selected(selected)
                                                    .label(label)
                                                    .on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.select_theme(name.clone(), cx);
                                                        },
                                                    ))
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Custom themes can be dropped into the themes folder next to the config file."),
                            ),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .p_4()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::surface(cx).opacity(0.55))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Conversation layout"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Choose where new agent work appears while you chat."),
                            )
                            .child(
                                crate::ui::style::segmented_container_quiet(cx)
                                    .max_w(px(420.))
                                    .child(
                                        crate::ui::style::segment(
                                            "settings-conversation-layout-classic",
                                            IconName::ArrowUp,
                                            "Classic · newest at bottom",
                                            conversation_layout == ConversationLayout::Classic,
                                            cx,
                                        )
                                        .flex_1()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_conversation_layout(
                                                ConversationLayout::Classic,
                                                cx,
                                            );
                                        })),
                                    )
                                    .child(
                                        crate::ui::style::segment(
                                            "settings-conversation-layout-top-down",
                                            IconName::ArrowDown,
                                            "Top-down · newest first",
                                            conversation_layout == ConversationLayout::TopDown,
                                            cx,
                                        )
                                        .flex_1()
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.select_conversation_layout(
                                                ConversationLayout::TopDown,
                                                cx,
                                            );
                                        })),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t4(cx))
                                    .child("Top-down places the composer first, then grows the latest conversation downward."),
                            ),
                    )
                    .into_any_element(),
                SettingsSection::Remote => {
                    let pairing = self.remote_auth.snapshot();
                    let active_code = pairing.active_code.clone();
                    let device_count = pairing.devices.len();
                    let mac_id = self.remote_room_id.clone();
                    let mac_id_for_copy = mac_id.clone();
                    let relay_state = self.remote_relay_control.state();
                    let relay_description = match relay_state {
                        RelayState::Disconnected => {
                            "Off. Connect to pair a phone or let paired iPhones reach this Mac."
                        }
                        RelayState::Connecting => "Connecting securely to the relay…",
                        RelayState::Connected => "On. Paired iPhones can reach this Mac.",
                        RelayState::Reconnecting => {
                            "Connection interrupted. Choro is reconnecting securely…"
                        }
                    };
                    let relay_button = match relay_state {
                        RelayState::Disconnected => crate::ui::style::primary_button_compact(
                            "settings-remote-connect",
                            "Connect",
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remote_relay_control.connect();
                            cx.notify();
                        })),
                        RelayState::Connecting => crate::ui::style::secondary_button_compact(
                            "settings-remote-disconnect",
                            "Cancel",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.remote_relay_control.disconnect();
                            cx.notify();
                        })),
                        RelayState::Connected | RelayState::Reconnecting => {
                            crate::ui::style::secondary_button_compact(
                                "settings-remote-disconnect",
                                "Disconnect",
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.remote_relay_control.disconnect();
                                cx.notify();
                            }))
                        }
                    };
                    v_flex()
                        .w_full()
                        .gap_3()
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .gap_3()
                                .p_4()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.55))
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Remote connection"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(relay_description),
                                        ),
                                )
                                .child(relay_button),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .gap_3()
                                .p_4()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.55))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .gap_3()
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_body())
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .child("Pair a phone"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child("Pairing stays on this Mac. No Choro account, email, or cloud identity is used."),
                                                ),
                                        )
                                        .child(
                                            crate::ui::style::primary_button_compact(
                                                "settings-start-remote-pairing",
                                                if active_code.is_some() {
                                                    "New code"
                                                } else {
                                                    "Connect & pair"
                                                },
                                                cx,
                                            )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.remote_relay_control.connect();
                                                    this.remote_auth.start_pairing();
                                                    this.remote_status = None;
                                                    cx.notify();
                                                })),
                                        ),
                                )
                                .child(
                                    v_flex()
                                        .gap_1()
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_label())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(crate::ui::design::t4(cx))
                                                .child("MAC ID"),
                                        )
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .items_center()
                                                .gap_2()
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w(px(0.))
                                                        .font_family(crate::ui::design::FONT_MONO)
                                                        .text_size(crate::ui::design::text_body())
                                                        .text_color(crate::ui::design::t2(cx))
                                                        .child(mac_id),
                                                )
                                                .child(
                                                    Button::new("settings-copy-remote-mac-id")
                                                        .ghost()
                                                        .xsmall()
                                                        .compact()
                                                        .h(crate::ui::design::control_h_xs())
                                                        .icon(IconName::Copy)
                                                        .label("Copy")
                                                        .tooltip("Copy Mac ID")
                                                        .on_click(move |_, _, cx| {
                                                            cx.write_to_clipboard(
                                                                gpui::ClipboardItem::new_string(
                                                                    mac_id_for_copy.clone(),
                                                                ),
                                                            );
                                                        }),
                                                ),
                                        ),
                                )
                                .when_some(active_code, |card, code| {
                                    let code_for_copy = code.clone();
                                    card.child(
                                        v_flex()
                                            .w_full()
                                            .gap_2()
                                            .p_3()
                                            .rounded(crate::ui::design::r_md())
                                            .border_1()
                                            .border_color(crate::ui::design::accent_line(cx))
                                            .bg(crate::ui::design::accent_soft(cx))
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_label())
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child("ONE-TIME PAIRING CODE · VALID FOR 5 MINUTES"),
                                            )
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .items_center()
                                                    .gap_2()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_w(px(0.))
                                                            .font_family(crate::ui::design::FONT_MONO)
                                                            .text_size(px(24.))
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(crate::ui::design::accent(cx))
                                                            .child(code),
                                                    )
                                                    .child(
                                                        Button::new("settings-copy-remote-pairing-code")
                                                            .ghost()
                                                            .xsmall()
                                                            .compact()
                                                            .h(crate::ui::design::control_h_xs())
                                                            .icon(IconName::Copy)
                                                            .label("Copy")
                                                            .tooltip("Copy pairing code")
                                                            .on_click(move |_, _, cx| {
                                                                cx.write_to_clipboard(
                                                                    gpui::ClipboardItem::new_string(
                                                                        code_for_copy.clone(),
                                                                    ),
                                                                );
                                                            }),
                                                    ),
                                            )
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .text_color(crate::ui::design::t3(cx))
                                                            .child("Enter this Mac ID and code in Choro Remote. The code encrypts pairing end to end."),
                                                    )
                                                    .child(
                                                        Button::new("settings-cancel-remote-pairing")
                                                            .ghost()
                                                            .small()
                                                            .label("Cancel")
                                                            .on_click(cx.listener(|this, _, _, cx| {
                                                                this.remote_auth.cancel_pairing();
                                                                cx.notify();
                                                            })),
                                                    ),
                                            ),
                                    )
                                }),
                        )
                        .child(
                            v_flex()
                                .w_full()
                                .gap_2()
                                .p_4()
                                .rounded(crate::ui::design::r_lg())
                                .border_1()
                                .border_color(crate::ui::design::line_2(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.55))
                                .child(
                                    h_flex()
                                        .w_full()
                                        .items_center()
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_size(crate::ui::design::text_body())
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .child("Paired devices"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(format!("{device_count}")),
                                        ),
                                )
                                .when(pairing.devices.is_empty(), |card| {
                                    card.child(
                                        div()
                                            .py_2()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child("No phones are paired. Remote data stays locked until a device is paired."),
                                    )
                                })
                                .children(pairing.devices.into_iter().enumerate().map(|(index, device)| {
                                    let device_id = device.id.clone();
                                    let permission = device.permission;
                                    let permission_label = match permission {
                                        DevicePermission::ViewOnly => "View only",
                                        DevicePermission::Control => "Control",
                                        DevicePermission::FullAccess => "Full access",
                                    };
                                    h_flex()
                                        .id(("settings-paired-device", index))
                                        .w_full()
                                        .min_h(px(42.))
                                        .gap_2()
                                        .items_center()
                                        .border_t_1()
                                        .border_color(crate::ui::design::line(cx).opacity(0.35))
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .gap_0p5()
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_body())
                                                        .text_color(crate::ui::design::t1(cx))
                                                        .child(device.name),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(crate::ui::design::text_label())
                                                        .text_color(crate::ui::design::t4(cx))
                                                        .child(format!(
                                                            "Paired {} · expires {}",
                                                            format_remote_time(device.paired_at),
                                                            format_remote_expiry(device.expires_at)
                                                        )),
                                                ),
                                        )
                                        .child(
                                            Button::new(("settings-device-permission", index))
                                                .ghost()
                                                .small()
                                                .label(permission_label)
                                                .tooltip("Change what this phone may do")
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    let next = match permission {
                                                        DevicePermission::ViewOnly => DevicePermission::Control,
                                                        DevicePermission::Control => DevicePermission::FullAccess,
                                                        DevicePermission::FullAccess => DevicePermission::ViewOnly,
                                                    };
                                                    this.remote_status = this
                                                        .remote_auth
                                                        .set_device_permission(&device_id, next)
                                                        .err()
                                                        .map(|error| format!("Could not update device access: {error:?}"));
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            Button::new(("settings-revoke-device", index))
                                                .ghost()
                                                .small()
                                                .label("Revoke")
                                                .on_click(cx.listener({
                                                    let device_id = device.id.clone();
                                                    move |this, _, _, cx| {
                                                    this.remote_status = this
                                                        .remote_auth
                                                        .revoke(&device_id)
                                                        .err()
                                                        .map(|error| format!("Could not revoke device: {error:?}"));
                                                    cx.notify();
                                                }})),
                                        )
                                }))
                                .when_some(self.remote_status.clone(), |card, status| {
                                    card.child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::rose(cx))
                                            .child(status),
                                    )
                                }),
                        )
                        .into_any_element()
                }
                SettingsSection::Data => v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .p_4()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::surface(cx).opacity(0.55))
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child("Export or restore the local workspace archive."),
                                    )
                                    .when(self.data_busy, |row| row.child(Spinner::new().xsmall()))
                                    .child(
                                        Button::new("settings-export-workspace")
                                            .label("Export")
                                            .disabled(self.data_busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.export_workspace(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("settings-import-workspace")
                                            .label("Import")
                                            .disabled(self.data_busy)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.import_workspace(cx);
                                            })),
                                    ),
                            )
                            .when_some(self.data_status.clone(), |card, status| {
                                card.child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(SharedString::from(status)),
                                )
                            }),
                    )
                    .into_any_element(),
                SettingsSection::AgentSkills => {
                    let search = self.skills_search.read(cx).value().trim().to_string();
                    let choro_total = self.riffs.len();
                    let codex_total = self
                        .skills
                        .iter()
                        .filter(|skill| skill.provider == AgentKind::Codex)
                        .count();
                    let claude_total = self
                        .skills
                        .iter()
                        .filter(|skill| skill.provider == AgentKind::Claude)
                        .count();
                    let filtered_skills = self
                        .skills
                        .iter()
                        .filter(|skill| {
                            self.skills_provider
                                .provider()
                                .is_some_and(|provider| skill.provider == provider)
                        })
                        .filter(|skill| skill.matches(&search))
                        .cloned()
                        .collect::<Vec<_>>();
                    let filtered_riffs = self
                        .riffs
                        .iter()
                        .filter(|_| self.skills_provider == SkillProviderFilter::Choro)
                        .filter(|riff| riff.matches(&search))
                        .cloned()
                        .collect::<Vec<_>>();
                    let editor = self.riff_editor.as_ref().map(|editor| {
                        let name_value = editor.name.read(cx).value().trim().to_string();
                        let description_value =
                            editor.description.read(cx).value().trim().to_string();
                        let instructions_empty =
                            editor.instructions.read(cx).value().trim().is_empty();
                        (
                            editor.id,
                            editor.name.clone(),
                            editor.description.clone(),
                            editor.instructions.clone(),
                            editor.generating,
                            instructions_empty,
                            !name_value.is_empty() && !description_value.is_empty(),
                            editor.error.clone(),
                        )
                    });
                    let editing_riff = editor.is_some();
                    let show_riff_empty_state = self.skills_provider == SkillProviderFilter::Choro
                        && self.riffs.is_empty()
                        && search.is_empty()
                        && editor.is_none();
                    let source_description = match self.skills_provider {
                        SkillProviderFilter::Choro => {
                            "Reusable instructions for Codex and Claude, available in every project."
                        }
                        SkillProviderFilter::Codex => {
                            "Skills discovered from Codex for the active workspace. Managed by Codex."
                        }
                        SkillProviderFilter::Claude => {
                            "Skills and commands discovered from Claude Code for the active workspace."
                        }
                    };
                    v_flex()
                        .w_full()
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_hidden()
                        .gap_3()
                        .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .child(
                                    crate::ui::style::segmented_container_quiet(cx)
                                        .w_full()
                                        .min_w(px(0.))
                                        .child(
                                            crate::ui::style::segment_with_leading(
                                                "settings-skills-choro",
                                                crate::ui::style::choro_riff_icon(
                                                    crate::ui::design::icon_md(),
                                                    if self.skills_provider
                                                        == SkillProviderFilter::Choro
                                                    {
                                                        crate::ui::design::accent(cx)
                                                    } else {
                                                        crate::ui::design::t3(cx)
                                                    },
                                                ),
                                                format!("Choro Riffs ({choro_total})"),
                                                self.skills_provider == SkillProviderFilter::Choro,
                                                cx,
                                            )
                                            .flex_1()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.skills_provider = SkillProviderFilter::Choro;
                                                cx.notify();
                                            })),
                                        )
                                        .child(
                                            crate::ui::style::segment(
                                                "settings-skills-codex",
                                                IconName::SquareTerminal,
                                                format!("Codex ({codex_total})"),
                                                self.skills_provider == SkillProviderFilter::Codex,
                                                cx,
                                            )
                                            .flex_1()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.skills_provider = SkillProviderFilter::Codex;
                                                cx.notify();
                                            })),
                                        )
                                        .child(
                                            crate::ui::style::segment(
                                                "settings-skills-claude",
                                                IconName::Bot,
                                                format!("Claude ({claude_total})"),
                                                self.skills_provider == SkillProviderFilter::Claude,
                                                cx,
                                            )
                                            .flex_1()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.skills_provider = SkillProviderFilter::Claude;
                                                cx.notify();
                                            })),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .min_w(px(0.))
                                .text_size(crate::ui::design::text_ui())
                                .line_height(gpui::relative(1.4))
                                .whitespace_normal()
                                .text_color(crate::ui::design::t3(cx))
                                .child(source_description),
                        )
                        .when_some(editor, |section, (id, name, description, instructions, generating, instructions_empty, has_generation_source, error)| {
                            section.child(
                                v_flex()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .overflow_y_scrollbar()
                                    .gap_3()
                                    .rounded(crate::ui::design::r_lg())
                                    .border_1()
                                    .border_color(crate::ui::design::accent_line(cx))
                                    .bg(crate::ui::design::surface(cx).opacity(0.64))
                                    .p_4()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .items_center()
                                            .child(
                                                div()
                                                    .size(px(34.))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .rounded_full()
                                                    .bg(crate::ui::design::accent_soft(cx))
                                                    .child(crate::ui::style::choro_riff_icon(
                                                        crate::ui::design::icon_lg(),
                                                        crate::ui::design::accent(cx),
                                                    )),
                                            )
                                            .child(
                                                v_flex()
                                                    .gap_0p5()
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_body())
                                                            .font_weight(FontWeight::SEMIBOLD)
                                                            .text_color(crate::ui::design::t1(cx))
                                                            .child(if id.is_some() { "Edit Riff" } else { "New Riff" }),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .text_color(crate::ui::design::t3(cx))
                                                            .child("Give agents a reusable way of working."),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(crate::ui::design::t2(cx))
                                                    .child("Name"),
                                            )
                                            .child(Input::new(&name)),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .font_weight(FontWeight::MEDIUM)
                                                    .text_color(crate::ui::design::t2(cx))
                                                    .child("Description"),
                                            )
                                            .child(Input::new(&description)),
                                    )
                                    .child(
                                        v_flex()
                                            .gap_1()
                                            .child(
                                                h_flex()
                                                    .w_full()
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .font_weight(FontWeight::MEDIUM)
                                                            .text_color(crate::ui::design::t2(cx))
                                                            .child("Instructions"),
                                                    )
                                                    .child(div().flex_1())
                                                    .child(
                                                        div()
                                                            .text_size(crate::ui::design::text_label())
                                                            .text_color(crate::ui::design::t4(cx))
                                                            .child("Hidden from the composer"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .w_full()
                                                    .h(px(280.))
                                                    .flex_none()
                                                    .overflow_hidden()
                                                    .rounded(crate::ui::design::r_md())
                                                    .border_1()
                                                    .border_color(crate::ui::design::line(cx))
                                                    .bg(crate::ui::design::base(cx).opacity(0.42))
                                                    .child(
                                                        v_flex()
                                                            .size_full()
                                                            .child(
                                                                div()
                                                                    .w_full()
                                                                    .flex_1()
                                                                    .min_h(px(0.))
                                                                    .overflow_hidden()
                                                                    .px_3()
                                                                    .py_2()
                                                                    .child(
                                                                        Input::new(&instructions)
                                                                            .appearance(false)
                                                                            .bordered(false)
                                                                            .focus_bordered(false)
                                                                            .w_full()
                                                                            .min_w(px(0.))
                                                                            .h_full(),
                                                                    ),
                                                            )
                                                            .when(instructions_empty, |editor| {
                                                                editor.child(
                                                                    h_flex()
                                                                        .w_full()
                                                                        .h(px(42.))
                                                                        .flex_none()
                                                                        .items_center()
                                                                        .gap_2()
                                                                        .px_2()
                                                                        .border_t_1()
                                                                        .border_color(crate::ui::design::line(cx).opacity(0.55))
                                                                        .child(
                                                                            crate::ui::style::ghost_button_compact(
                                                                                "generate-riff-instructions",
                                                                                if generating { "Generating…" } else { "Generate with AI" },
                                                                            )
                                                                            .icon(IconName::Bot)
                                                                            .text_color(crate::ui::design::accent(cx))
                                                                            .disabled(generating || !has_generation_source)
                                                                            .on_click(cx.listener(|this, _, window, cx| {
                                                                                this.generate_riff_instructions(window, cx);
                                                                            })),
                                                                        )
                                                                        .when(generating, |row| {
                                                                            row.child(Spinner::new().xsmall())
                                                                        })
                                                                        .child(div().flex_1())
                                                                        .child(
                                                                            div()
                                                                                .text_size(crate::ui::design::text_label())
                                                                                .text_color(crate::ui::design::t4(cx))
                                                                                .child("Uses the name and description"),
                                                                        ),
                                                                )
                                                            }),
                                                    ),
                                            ),
                                    )
                                    .when_some(error, |card, error| {
                                        card.child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::rose(cx))
                                                .child(error),
                                        )
                                    })
                                    .child(
                                        h_flex()
                                            .w_full()
                                            .gap_2()
                                            .justify_end()
                                            .when_some(id, |row, id| {
                                                row.child(
                                                    crate::ui::style::danger_button_compact(
                                                        ("delete-choro-riff", id.as_u128() as u64),
                                                        "Delete",
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.delete_riff(id, cx);
                                                    })),
                                                )
                                            })
                                            .child(
                                                crate::ui::style::dialog_neutral_button(
                                                    "cancel-choro-riff",
                                                    "Cancel",
                                                    cx,
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.riff_editor = None;
                                                    cx.notify();
                                                })),
                                            )
                                            .child(
                                                crate::ui::style::primary_button_compact(
                                                    "save-choro-riff",
                                                    "Save Riff",
                                                    cx,
                                                )
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.save_riff_editor(cx);
                                                })),
                                            ),
                                    ),
                                )
                            })
                        .when(show_riff_empty_state, |section| {
                            section.child(
                                v_flex()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .items_center()
                                    .justify_center()
                                    .gap_4()
                                    .rounded(crate::ui::design::r_lg())
                                    .border_1()
                                    .border_color(crate::ui::design::line(cx).opacity(0.38))
                                    .bg(crate::ui::design::surface(cx).opacity(0.32))
                                    .child(
                                        div()
                                            .size(px(64.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded_full()
                                            .border_1()
                                            .border_color(crate::ui::design::accent_line(cx))
                                            .bg(crate::ui::design::accent_soft(cx))
                                            .child(crate::ui::style::choro_riff_icon(
                                                px(30.),
                                                crate::ui::design::accent(cx),
                                            )),
                                    )
                                    .child(
                                        v_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_title())
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(crate::ui::design::t1(cx))
                                                    .child("Create your first Riff"),
                                            )
                                            .child(
                                                div()
                                                    .max_w(px(430.))
                                                    .text_center()
                                                    .whitespace_normal()
                                                    .line_height(gpui::relative(1.45))
                                                    .text_size(crate::ui::design::text_body())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child("Save the way you like agents to work, then call it from any project with /."),
                                            ),
                                    )
                                    .child(
                                        crate::ui::style::primary_button_compact(
                                            "empty-add-choro-riff",
                                            "Create a Riff",
                                            cx,
                                        )
                                        .icon(IconName::Plus)
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_riff_editor(None, window, cx);
                                        })),
                                    ),
                            )
                        })
                        .when(!show_riff_empty_state && !editing_riff, |section| {
                            section.child(
                                h_flex()
                                    .w_full()
                                    .gap_3()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .max_w(px(360.))
                                            .child(Input::new(&self.skills_search)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .truncate()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t4(cx))
                                            .child(if self.skills_provider == SkillProviderFilter::Choro {
                                                format!("{} Riffs · available everywhere", filtered_riffs.len())
                                            } else {
                                                format!(
                                                    "{} shown · {}{}",
                                                    filtered_skills.len(),
                                                    self.skills_cwd,
                                                    self.skills_last_refreshed
                                                        .as_ref()
                                                        .filter(|value| !value.is_empty())
                                                        .map(|value| format!(" · refreshed {value}"))
                                                        .unwrap_or_default()
                                                )
                                            }),
                                    )
                                    .when(
                                        self.skills_provider == SkillProviderFilter::Choro,
                                        |row| {
                                            row.child(
                                                crate::ui::style::primary_button_compact(
                                                    "add-choro-riff",
                                                    "New Riff",
                                                    cx,
                                                )
                                                .icon(IconName::Plus)
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_riff_editor(None, window, cx);
                                                })),
                                            )
                                        },
                                    )
                                    .when(
                                        self.skills_provider != SkillProviderFilter::Choro,
                                        |row| {
                                            row.when(self.skills_loading, |row| {
                                                row.child(Spinner::new().xsmall())
                                            })
                                            .child(
                                                crate::ui::style::refresh_button(
                                                    "refresh-agent-skills",
                                                    "Refresh",
                                                    cx,
                                                )
                                                .disabled(self.skills_loading)
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.refresh_agent_skills(cx);
                                                })),
                                            )
                                        },
                                    ),
                            )
                        })
                        .when_some(
                            (!editing_riff).then(|| self.riffs_status.clone()).flatten(),
                            |section, status| {
                            section.child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(status),
                            )
                        })
                        .when_some(
                            (!editing_riff).then(|| self.skills_error.clone()).flatten(),
                            |section, error| {
                            section.child(
                                div()
                                    .rounded(crate::ui::design::r_sm())
                                    .border_1()
                                    .border_color(crate::ui::design::rose(cx).opacity(0.35))
                                    .bg(crate::ui::design::rose(cx).opacity(0.08))
                                    .px_2()
                                    .py_1()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(SharedString::from(error)),
                            )
                        })
                        .when(!show_riff_empty_state && !editing_riff, |section| {
                            section.child(v_flex()
                                .w_full()
                                .flex_1()
                                .min_h(px(0.))
                                .overflow_y_scrollbar()
                                .rounded(crate::ui::design::r_md())
                                .border_1()
                                .border_color(crate::ui::design::line(cx))
                                .bg(crate::ui::design::surface(cx).opacity(0.32))
                                .when(
                                    self.skills_provider == SkillProviderFilter::Choro
                                        && filtered_riffs.is_empty(),
                                    |list| {
                                    list.child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(if search.is_empty() {
                                                "No Choro Riffs yet. Create one to make it available in every project.".to_string()
                                            } else {
                                                format!("No matches for {search}")
                                            }),
                                    )
                                })
                                .when(
                                    self.skills_provider != SkillProviderFilter::Choro
                                        && filtered_skills.is_empty(),
                                    |list| list.child(
                                        div()
                                            .px_3()
                                            .py_2()
                                            .text_size(crate::ui::design::text_body())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(if self.skills_loading {
                                                "Loading skills...".to_string()
                                            } else if search.is_empty() {
                                                format!("No {} skills loaded", self.skills_provider.label())
                                            } else {
                                                format!("No matches for {search}")
                                            }),
                                    ),
                                )
                                .children(filtered_riffs.iter().enumerate().map(|(index, riff)| {
                                    let riff_for_edit = riff.clone();
                                    let id = riff.id;
                                    h_flex()
                                        .id(("settings-choro-riff-row", index))
                                        .w_full()
                                        .min_w(px(0.))
                                        .min_h(px(64.))
                                        .gap_3()
                                        .items_center()
                                        .px_3()
                                        .py_2()
                                        .border_b_1()
                                        .border_color(crate::ui::design::line(cx).opacity(0.18))
                                        .child(
                                            div()
                                                .size(px(32.))
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(crate::ui::design::r_sm())
                                                .bg(crate::ui::design::accent_soft(cx))
                                                .child(crate::ui::style::choro_riff_icon(
                                                    crate::ui::design::icon_md(),
                                                    crate::ui::design::accent(cx),
                                                )),
                                        )
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .gap_1()
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .text_size(crate::ui::design::text_body())
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(crate::ui::design::t1(cx))
                                                        .child(riff.name.clone()),
                                                )
                                                .when_some(riff.description.clone(), |col, description| {
                                                    col.child(
                                                        div()
                                                            .truncate()
                                                            .text_size(crate::ui::design::text_ui())
                                                            .text_color(crate::ui::design::t3(cx))
                                                            .child(description),
                                                    )
                                                }),
                                        )
                                        .child(
                                            h_flex()
                                                .gap_1()
                                                .child(
                                                    crate::ui::style::dialog_neutral_button(
                                                        ("toggle-choro-riff", id.as_u128() as u64),
                                                        if riff.enabled { "Enabled" } else { "Disabled" },
                                                        cx,
                                                    )
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.toggle_riff(id, cx);
                                                    })),
                                                )
                                                .child(
                                                    crate::ui::style::dialog_neutral_button(
                                                        ("edit-choro-riff", id.as_u128() as u64),
                                                        "Edit",
                                                        cx,
                                                    )
                                                    .on_click(cx.listener(move |this, _, window, cx| {
                                                        this.open_riff_editor(Some(riff_for_edit.clone()), window, cx);
                                                    })),
                                                ),
                                        )
                                }))
                                .children(filtered_skills.iter().enumerate().map(|(index, skill)| {
                                    let subtitle = skill
                                        .description
                                        .clone()
                                        .unwrap_or_else(|| skill.name.clone());
                                    h_flex()
                                        .id(("settings-agent-skill-row", index))
                                        .w_full()
                                        .min_w(px(0.))
                                        .min_h(px(64.))
                                        .gap_3()
                                        .items_center()
                                        .px_3()
                                        .py_2()
                                        .border_b_1()
                                        .border_color(crate::ui::design::line(cx).opacity(0.18))
                                        .child(
                                            div()
                                                .w(px(120.))
                                                .flex_none()
                                                .truncate()
                                                .font_family(crate::ui::design::FONT_MONO)
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(crate::ui::design::accent(cx))
                                                .child(skill.invocation.trim().to_string()),
                                        )
                                        .child(
                                            v_flex()
                                                .flex_1()
                                                .min_w(px(0.))
                                                .gap_0p5()
                                                .child(
                                                    h_flex()
                                                        .w_full()
                                                        .min_w(px(0.))
                                                        .gap_2()
                                                        .items_center()
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .min_w(px(0.))
                                                                .truncate()
                                                                .text_size(crate::ui::design::text_body())
                                                                .font_weight(FontWeight::SEMIBOLD)
                                                                .child(skill.title.clone()),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_none()
                                                                .rounded(crate::ui::design::r_sm())
                                                                .border_1()
                                                                .border_color(
                                                                    crate::ui::design::line(cx)
                                                                        .opacity(0.24),
                                                                )
                                                                .bg(
                                                                    crate::ui::design::base(cx)
                                                                        .opacity(0.55),
                                                                )
                                                                .px_2()
                                                                .py_0p5()
                                                                .text_size(crate::ui::design::text_ui())
                                                                .text_color(
                                                                    crate::ui::design::t3(cx),
                                                                )
                                                                .child(skill.source.label()),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .text_size(crate::ui::design::text_ui())
                                                        .text_color(crate::ui::design::t3(cx))
                                                        .child(subtitle),
                                                ),
                                        )
                                }))
                            )
                        })
                        .into_any_element()
                }
                SettingsSection::Process => v_flex()
                    .w_full()
                    .gap_3()
                    .child(
                        v_flex()
                            .w_full()
                            .gap_3()
                            .p_4()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::surface(cx).opacity(0.55))
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        v_flex()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .gap_0p5()
                                            .child(
                                                h_flex()
                                                    .gap_3()
                                                    .text_size(crate::ui::design::text_body())
                                                    .child(format!(
                                                        "App {}",
                                                        Self::format_mb(snapshot.app_mb)
                                                    ))
                                                    .child(format!(
                                                        "Tree {}",
                                                        Self::format_mb(snapshot.total_mb)
                                                    ))
                                                    .child(format!("CPU {:.1}%", snapshot.total_cpu)),
                                            )
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(format!(
                                                        "PID {} · {} process{} including terminals, scripts, and agents",
                                                        snapshot.root_pid,
                                                        snapshot.process_count,
                                                        if snapshot.process_count == 1 { "" } else { "es" },
                                                    )),
                                            ),
                                    )
                                    .when(process_loading, |row| {
                                        row.child(Spinner::new().xsmall())
                                    })
                                    .child(
                                        crate::ui::style::refresh_button(
                                            "refresh-process-monitor",
                                            "Refresh",
                                            cx,
                                        )
                                            .disabled(process_loading)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.refresh_process_snapshot(cx);
                                            })),
                                    ),
                            )
                            .when_some(snapshot.error.clone(), |card, error| {
                                card.child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::rose(cx))
                                        .child(SharedString::from(error)),
                                )
                            })
                            .when(snapshot.error.is_none(), |card| {
                                card.child(v_flex().w_full().gap_1().children(snapshot.top.iter().map(
                                    |process| {
                                        h_flex()
                                            .w_full()
                                            .gap_2()
                                            .items_center()
                                            .text_size(crate::ui::design::text_ui())
                                            .child(
                                                div()
                                                    .w(px(56.))
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(format!("{}", process.pid)),
                                            )
                                            .child(
                                                div()
                                                    .w(px(70.))
                                                    .child(Self::format_mb(process.rss_kb as f64 / 1024.0)),
                                            )
                                            .child(
                                                div()
                                                    .w(px(52.))
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(format!("{:.1}%", process.cpu)),
                                            )
                                            .child(
                                                div()
                                                    .w(px(92.))
                                                    .truncate()
                                                    .text_color(crate::ui::design::accent(cx))
                                                    .child(SharedString::from(
                                                        process
                                                            .project
                                                            .clone()
                                                            .unwrap_or_else(|| "App".into()),
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .truncate()
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(SharedString::from(process.command.clone())),
                                            )
                                    },
                                )))
                            }),
                    )
                    .into_any_element(),
                SettingsSection::Shortcuts => self.render_shortcuts(cx),
            }),
                    ),
            )
    }
}

/// Settings → Memory: the facts every agent starts with. Two tabs (Global /
/// Project), inline editor, one-tap enable/pin/delete. Rows live in the
/// shared `memories` table — the same one the `memory_save` MCP tool writes.
impl SettingsView {
    fn reload_memories(&mut self, cx: &mut gpui::Context<Self>) {
        self.memories = ide_core::local_store::LocalStore::open_default()
            .and_then(|store| store.load_all_memories())
            .unwrap_or_default();
        cx.notify();
    }

    fn with_memory_store(
        &mut self,
        cx: &mut gpui::Context<Self>,
        apply: impl FnOnce(&ide_core::local_store::LocalStore) -> anyhow::Result<()>,
    ) {
        match ide_core::local_store::LocalStore::open_default() {
            Ok(store) => {
                if let Err(error) = apply(&store) {
                    self.memory_status = Some(format!("{error:#}"));
                }
            }
            Err(error) => self.memory_status = Some(format!("{error:#}")),
        }
        self.reload_memories(cx);
    }

    fn open_memory_editor(
        &mut self,
        memory: Option<&ide_core::local_store::StoredMemory>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let initial = memory.map(|memory| memory.text.clone()).unwrap_or_default();
        let input = cx.new(|cx| {
            let mut state = InputState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("One short, self-contained fact");
            state.set_value(&initial, window, cx);
            state
        });
        self.memory_editor = Some(MemoryEditor {
            id: memory.map(|memory| memory.id),
            input,
            error: None,
        });
        cx.notify();
    }

    fn save_memory_editor(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(editor) = self.memory_editor.as_ref() else {
            return;
        };
        let text = editor.input.read(cx).value().trim().to_string();
        if text.is_empty() {
            if let Some(editor) = self.memory_editor.as_mut() {
                editor.error = Some("Write the fact first.".to_string());
            }
            cx.notify();
            return;
        }
        let editing = editor.id;
        let global = self.memory_scope_global;
        let project = self.memory_project;
        if !global && editing.is_none() && project.is_none() {
            if let Some(editor) = self.memory_editor.as_mut() {
                editor.error = Some("Choose a project for a project memory.".to_string());
            }
            cx.notify();
            return;
        }
        self.memory_editor = None;
        self.with_memory_store(cx, move |store| {
            match editing {
                Some(id) => store.update_memory_text(id, &text)?,
                None => {
                    if global {
                        store.save_memory("global", None, &text, None)?;
                    } else {
                        store.save_memory("project", project, &text, None)?;
                    }
                }
            }
            Ok(())
        });
    }

    fn render_memory_section(&mut self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let global_tab = self.memory_scope_global;
        let selected_project = self.memory_project;
        let project_label = selected_project
            .and_then(|id| {
                self.projects
                    .iter()
                    .find(|candidate| candidate.id == id)
                    .map(|candidate| candidate.name.clone())
            })
            .unwrap_or_else(|| "Choose project".to_string());
        let rows: Vec<ide_core::local_store::StoredMemory> = self
            .memories
            .iter()
            .filter(|memory| {
                if global_tab {
                    memory.is_global()
                } else {
                    memory.project_id.is_some() && memory.project_id == selected_project
                }
            })
            .cloned()
            .collect();
        let proposals_enabled = self.workspace.read(cx).memory_proposals_enabled;

        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .child("Suggest memories from decisions"),
                    )
                    .child(
                        crate::ui::style::ghost_button_compact(
                            "settings-memory-proposals-toggle",
                            if proposals_enabled { "On" } else { "Off" },
                        )
                        .tooltip(
                            "When plan feedback or an answer reveals a preference, Choro proposes a memory — nothing is saved without your OK",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.workspace.update(cx, |workspace, cx| {
                                workspace.set_memory_proposals_enabled(!proposals_enabled, cx);
                            });
                            cx.notify();
                        })),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .child(
                        crate::ui::style::segmented_container_quiet(cx)
                            .w_auto()
                            .child(
                                crate::ui::style::segment(
                                    "settings-memory-global",
                                    IconName::CircleUser,
                                    "Global",
                                    global_tab,
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.memory_scope_global = true;
                                    this.memory_editor = None;
                                    cx.notify();
                                })),
                            )
                            .child(
                                crate::ui::style::segment(
                                    "settings-memory-project",
                                    IconName::FolderOpen,
                                    "Project",
                                    !global_tab,
                                    cx,
                                )
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.memory_scope_global = false;
                                    this.memory_editor = None;
                                    cx.notify();
                                })),
                            ),
                    )
                    .when(!global_tab, |row| {
                        let projects = self.projects.clone();
                        let view = cx.entity();
                        row.child(
                            crate::ui::style::dialog_neutral_button(
                                "settings-memory-project-picker",
                                project_label,
                                cx,
                            )
                            .icon(IconName::ChevronDown)
                            .dropdown_menu(move |mut menu, window_ref, _| {
                                for source in projects.clone() {
                                    let id = source.id;
                                    menu = menu.item(
                                        PopupMenuItem::new(source.name.clone())
                                            .checked(selected_project == Some(id))
                                            .on_click(window_ref.listener_for(
                                                &view,
                                                move |this: &mut SettingsView, _, _, cx| {
                                                    this.memory_project = Some(id);
                                                    this.memory_editor = None;
                                                    cx.notify();
                                                },
                                            )),
                                    );
                                }
                                menu
                            }),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        crate::ui::style::primary_button_compact(
                            "settings-memory-new",
                            "New memory",
                            cx,
                        )
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.open_memory_editor(None, window, cx);
                        })),
                    ),
            )
            .when_some(self.memory_status.clone(), |section, status| {
                section.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(status),
                )
            })
            .when_some(
                self.memory_editor.as_ref().map(|editor| {
                    (editor.input.clone(), editor.error.clone(), editor.id)
                }),
                |section, (input, error, editing)| {
                    section.child(
                        v_flex()
                            .w_full()
                            .gap_2()
                            .p_4()
                            .rounded(crate::ui::design::r_lg())
                            .border_1()
                            .border_color(crate::ui::design::line_2(cx))
                            .bg(crate::ui::design::surface(cx).opacity(0.55))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(if editing.is_some() {
                                        "Edit memory"
                                    } else {
                                        "New memory"
                                    }),
                            )
                            .child(Input::new(&input))
                            .when_some(error, |editor, error| {
                                editor.child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::rose(cx))
                                        .child(error),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_2()
                                    .justify_end()
                                    .child(
                                        crate::ui::style::secondary_button_compact(
                                            "settings-memory-cancel",
                                            "Cancel",
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.memory_editor = None;
                                            cx.notify();
                                        })),
                                    )
                                    .child(
                                        crate::ui::style::primary_button_compact(
                                            "settings-memory-save",
                                            "Save",
                                            cx,
                                        )
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_memory_editor(cx);
                                        })),
                                    ),
                            ),
                    )
                },
            )
            .child(if rows.is_empty() {
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child(if global_tab {
                        "Nothing remembered about you yet. Global memories are added explicitly here."
                    } else {
                        "Nothing remembered for this project yet. Say \"remember …\" in any agent chat, or add one here."
                    })
                    .into_any_element()
            } else {
                v_flex()
                    .w_full()
                    .rounded(crate::ui::design::r_lg())
                    .border_1()
                    .border_color(crate::ui::design::line_2(cx))
                    .bg(crate::ui::design::surface(cx).opacity(0.55))
                    .children(rows.into_iter().enumerate().map(|(index, memory)| {
                        let id = memory.id;
                        let pinned = memory.pinned;
                        let enabled = memory.enabled;
                        h_flex()
                            .w_full()
                            .px_4()
                            .py_2p5()
                            .gap_3()
                            .items_center()
                            .when(index > 0, |row| {
                                row.border_t_1()
                                    .border_color(crate::ui::design::line(cx))
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(if enabled {
                                        crate::ui::design::t2(cx)
                                    } else {
                                        crate::ui::design::t4(cx)
                                    })
                                    .child(memory.text.clone()),
                            )
                            .when(pinned, |row| {
                                row.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::amber(cx))
                                        .child("pinned"),
                                )
                            })
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("settings-memory-pin", index),
                                    if pinned { "Unpin" } else { "Pin" },
                                )
                                    .tooltip("Pinned memories are considered first")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.with_memory_store(cx, |store| {
                                            store.set_memory_pinned(id, !pinned)
                                        });
                                    })),
                            )
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("settings-memory-toggle", index),
                                    if enabled { "On" } else { "Off" },
                                )
                                    .tooltip("Disabled memories stay saved but never inject")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.with_memory_store(cx, |store| {
                                            store.set_memory_enabled(id, !enabled)
                                        });
                                    })),
                            )
                            .child(
                                crate::ui::style::ghost_button_compact(
                                    ("settings-memory-edit", index),
                                    "Edit",
                                )
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        let memory = this
                                            .memories
                                            .iter()
                                            .find(|memory| memory.id == id)
                                            .cloned();
                                        if let Some(memory) = memory {
                                            this.open_memory_editor(Some(&memory), window, cx);
                                        }
                                    })),
                            )
                            .child(
                                crate::ui::style::danger_button_compact(
                                    ("settings-memory-delete", index),
                                    "Delete",
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.with_memory_store(cx, |store| store.delete_memory(id));
                                })),
                            )
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }
}
