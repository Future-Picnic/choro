use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, FocusHandle, FontWeight,
    InteractiveElement, IntoElement, KeyDownEvent, ParentElement, PathPromptOptions, Render,
    SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement,
    spinner::Spinner,
    text::{TextView, TextViewStyle},
    v_flex, Disableable, Icon, IconName, Sizable, WindowExt,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::keymap;
use crate::remote::{DevicePermission, RelayControl, RelayIdentity, RelayState, RemoteAuth};
use crate::state::{
    AgentCapability, AgentCapabilityCacheFile, ChoroRiff, ChoroRiffStore, DesignProvider,
    OrbitEvent, OrbitState, PenpotConnectionStatus, PenpotState, QuickAskState, Workspace,
    CHORO_RIFFS_SCHEMA_VERSION,
};
use ide_core::{
    config::{
        CompletionNotifications, ConversationLayout, GenerationAgent, NewAgentDefaults,
        ReviewChecklistMode, SeparatorStyle, SidebarStyle, ThemeMode as ConfigTheme,
        VerificationMode, DEFAULT_CODE_REVIEW_PROMPT,
    },
    local_store::{
        analytics_orbit_template, blank_orbit_module, normalize_orbit_field_key,
        validate_orbit_module, LocalStore, OrbitFieldDefinition, OrbitFieldKind,
        OrbitModuleDefinition,
    },
    AgentEffort, AgentKind, AgentModel, AgentRecord, ProjectId, VoiceAnnouncements,
};
use uuid::Uuid;

mod appearance_page;
mod beta_features;
mod brain;
mod companion;
mod data_page;
mod design;
mod experts_page;
mod generation_page;
mod memory;
mod notifications;
mod orbit_page;
mod process_monitor;
mod process_page;
mod remote_page;
mod render;
mod shortcuts;
mod skills_page;
mod theme_preview;
mod voice;

const PENPOT_CLOUD_URL: &str = "https://design.penpot.app";
const PENPOT_CLOUD_MCP_URL: &str = "https://design.penpot.app/mcp/stream";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Design,
    Generation,
    Voice,
    Companion,
    Notifications,
    Process,
    AgentSkills,
    Experts,
    BetaFeatures,
    Orbit,
    Brain,
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
            Self::Voice => "Voice",
            Self::Companion => "Companion",
            Self::Notifications => "Notifications",
            Self::Process => "Process monitor",
            Self::AgentSkills => "Skills",
            Self::Experts => "Band",
            Self::BetaFeatures => "Beta features",
            Self::Orbit => "Orbit",
            Self::Brain => "Knowledge",
            Self::Memory => "Memories",
            Self::Remote => "Remote access",
            Self::Data => "Data & backups",
            Self::Shortcuts => "Keyboard shortcuts",
            Self::Appearance => "Appearance",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Design => {
                "Studio is your default design workspace. Configure optional Penpot connections when enabled."
            }
            Self::Generation => {
                "Choose defaults for agents, Quick Ask, and generated content, plus review behavior."
            }
            Self::Voice => "Manage local speech models, spoken feedback, and patient turn-taking.",
            Self::Companion => {
                "Your desktop companion and the music for every mood."
            }
            Self::Notifications => {
                "Choose when agent questions, approvals, and completed turns can interrupt you."
            }
            Self::Process => "Understand how Choro and its connected tools use system resources.",
            Self::AgentSkills => {
                "Create Choro Riffs for every project and review skills discovered from your coding agents."
            }
            Self::Experts => "Your bandmates are AI specialists. Start a chat with one or ask your lead to bring them into a task.",
            Self::BetaFeatures => "Try optional features, including delegation and Penpot.",
            Self::Orbit => {
                "Create reusable project modules with structured views and an agent job."
            }
            Self::Brain => "Search living summaries from agents across your projects.",
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

struct OrbitModuleEditor {
    request_id: Uuid,
    module: OrbitModuleDefinition,
    name: Entity<InputState>,
    description: Entity<InputState>,
    section: Entity<InputState>,
    agent_job: Entity<InputState>,
    fields: Vec<OrbitFieldEditor>,
    error: Option<String>,
}

struct OrbitFieldEditor {
    id: Uuid,
    key: String,
    label: Entity<InputState>,
    kind: OrbitFieldKind,
    primary: bool,
}

struct CompanionPlaylistInputs {
    urls: Vec<Entity<InputState>>,
}

#[derive(Clone)]
pub(crate) struct ProjectSource {
    pub(crate) id: ide_core::ProjectId,
    pub(crate) name: String,
    pub(crate) path: String,
}

/// Settings dialog: editable keyboard shortcuts (persisted to config).
pub struct SettingsView {
    experts: Vec<ide_core::experts::ExpertProfile>,
    expert_editor: Option<experts_page::ExpertEditor>,
    experts_status: Option<experts_page::ExpertsNotice>,
    experts_search: Entity<InputState>,
    beta_features_error: Option<String>,
    workspace: Entity<Workspace>,
    quick_ask: Entity<QuickAskState>,
    voice: Entity<crate::voice::VoiceState>,
    penpot: Entity<PenpotState>,
    orbit: Entity<OrbitState>,
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
    orbit_editor: Option<OrbitModuleEditor>,
    riffs_status: Option<String>,
    memories: Vec<ide_core::local_store::StoredMemory>,
    brain_summaries: Vec<ide_core::local_store::StoredAgentSummary>,
    brain_agents: Vec<AgentRecord>,
    brain_search_index: HashMap<Uuid, String>,
    brain_data_loading: bool,
    brain_project: Option<ide_core::ProjectId>,
    brain_search: Entity<InputState>,
    brain_expanded: bool,
    /// Which tab: `true` = Global, `false` = Project.
    memory_scope_global: bool,
    memory_project: Option<ide_core::ProjectId>,
    memory_editor: Option<MemoryEditor>,
    memory_status: Option<String>,
    settings_search: Entity<InputState>,
    code_review_prompt: Entity<InputState>,
    code_review_prompt_dirty: bool,
    companion_playlists: Vec<CompanionPlaylistInputs>,
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
pub(crate) struct ProcessSnapshot {
    root_pid: i32,
    app_bytes: u64,
    pub(crate) total_bytes: u64,
    total_cpu: f64,
    process_count: usize,
    pub(crate) processes: Vec<ProcessInfo>,
    categories: Vec<MemoryCategory>,
    measurement_note: String,
    pub(crate) error: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ProcessInfo {
    pub(crate) pid: i32,
    pub(crate) memory_bytes: u64,
    pub(crate) cpu: f64,
    pub(crate) project_id: Option<ProjectId>,
    pub(crate) project: Option<String>,
    pub(crate) agent_id: Option<Uuid>,
    pub(crate) agent: Option<String>,
    pub(crate) name: String,
    pub(crate) command: String,
}

#[derive(Clone)]
struct ProcessAgentGroup {
    agent_id: Option<Uuid>,
    name: String,
    memory_bytes: u64,
    cpu: f64,
    processes: Vec<ProcessInfo>,
}

#[derive(Clone)]
struct ProcessProjectGroup {
    project_id: Option<ProjectId>,
    name: String,
    memory_bytes: u64,
    cpu: f64,
    process_count: usize,
    agents: Vec<ProcessAgentGroup>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoryCategory {
    bytes: u64,
    regions: u64,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FootprintProcess {
    pid: i32,
    name: String,
    bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FootprintSnapshot {
    app_bytes: u64,
    total_bytes: u64,
    processes: Vec<FootprintProcess>,
    categories: Vec<MemoryCategory>,
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
        quick_ask: Entity<QuickAskState>,
        penpot: Entity<PenpotState>,
        voice: Entity<crate::voice::VoiceState>,
        orbit: Entity<OrbitState>,
        remote_auth: RemoteAuth,
        relay_identity: RelayIdentity,
        relay_control: RelayControl,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        Self::new_in_section(
            workspace,
            quick_ask,
            penpot,
            voice,
            orbit,
            remote_auth,
            relay_identity,
            relay_control,
            SettingsSection::AgentSkills,
            window,
            cx,
        )
    }

    pub(crate) fn new_in_section(
        workspace: Entity<Workspace>,
        quick_ask: Entity<QuickAskState>,
        penpot: Entity<PenpotState>,
        voice: Entity<crate::voice::VoiceState>,
        orbit: Entity<OrbitState>,
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
        let companion_music = state.companion_music.clone();
        let shortcut_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search shortcuts"));
        let settings_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search settings…"));
        let brain_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search agent summaries…"));
        let code_review_prompt = cx.new(|cx| InputState::new(window, cx).multi_line(true));
        code_review_prompt.update(cx, |input, cx| {
            input.set_value(code_review_prompt_value, window, cx)
        });
        let companion_playlists = companion_music
            .playlists
            .iter()
            .map(|playlist| CompanionPlaylistInputs {
                urls: playlist
                    .urls
                    .iter()
                    .map(|url| {
                        cx.new(|cx| {
                            InputState::new(window, cx)
                                .default_value(url.clone())
                                .placeholder("Paste a Spotify playlist link…")
                        })
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let skills_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search skills"));
        let experts_search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search bandmates"));
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
            for (index, playlist) in companion_playlists.iter().enumerate() {
                for url in &playlist.urls {
                    cx.subscribe(url, move |this: &mut Self, _, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.sync_companion_playlist(index, cx);
                        }
                    })
                    .detach();
                }
            }
            cx.subscribe(
                &shortcut_search,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(
                &experts_search,
                |_: &mut Self, _, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                },
            )
            .detach();
            cx.subscribe(&brain_search, |_: &mut Self, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.observe(&workspace, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&penpot, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&voice, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&quick_ask, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&orbit, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.subscribe(&orbit, |this: &mut Self, _, event: &OrbitEvent, cx| {
                if let OrbitEvent::ModuleSaved {
                    module_id,
                    request_id,
                } = event
                {
                    if this.orbit_editor.as_ref().is_some_and(|editor| {
                        editor.module.id == *module_id && editor.request_id == *request_id
                    }) {
                        this.orbit_editor = None;
                        cx.notify();
                    }
                }
            })
            .detach();
            Self {
                experts: LocalStore::open_default()
                    .and_then(|s| s.load_experts())
                    .unwrap_or_default(),
                expert_editor: None,
                experts_status: None,
                experts_search,
                beta_features_error: None,
                workspace: workspace.clone(),
                quick_ask: quick_ask.clone(),
                voice: voice.clone(),
                penpot: penpot.clone(),
                orbit: orbit.clone(),
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
                orbit_editor: None,
                riffs_status: None,
                memories: Vec::new(),
                brain_summaries: Vec::new(),
                brain_agents: Vec::new(),
                brain_search_index: HashMap::new(),
                brain_data_loading: true,
                brain_project: active_project
                    .or_else(|| projects.first().map(|project| project.id)),
                brain_search,
                brain_expanded: matches!(section, SettingsSection::Brain | SettingsSection::Memory),
                memory_scope_global: false,
                memory_project: active_project,
                memory_editor: None,
                memory_status: None,
                settings_search,
                code_review_prompt,
                code_review_prompt_dirty: false,
                companion_playlists,
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
        view.update(cx, |view, cx| view.refresh_brain_data(cx));
        view
    }

    fn refresh_brain_data(&mut self, cx: &mut Context<Self>) {
        if !self.brain_data_loading {
            self.brain_data_loading = true;
            cx.notify();
        }
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    let store = LocalStore::open_default()?;
                    Ok::<_, anyhow::Error>((
                        store.load_all_memories()?,
                        store.load_all_agent_summaries()?,
                        store.load_agents()?,
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                this.brain_data_loading = false;
                match loaded {
                    Ok((memories, summaries, agents)) => {
                        this.brain_search_index = summaries
                            .iter()
                            .map(|summary| {
                                let agent_title = agents
                                    .iter()
                                    .find(|agent| agent.id == summary.agent_id)
                                    .map(|agent| agent.title.as_str())
                                    .unwrap_or_default();
                                (
                                    summary.agent_id,
                                    format!(
                                        "{}\n{}\n{}",
                                        agent_title,
                                        summary.summary_text,
                                        summary.outcome_text.as_deref().unwrap_or_default()
                                    )
                                    .to_lowercase(),
                                )
                            })
                            .collect();
                        this.memories = memories;
                        this.brain_summaries = summaries;
                        this.brain_agents = agents;
                        this.memory_status = None;
                    }
                    Err(error) => {
                        this.memory_status = Some(format!("Could not load Brain data: {error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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

    fn select_sidebar_style(&mut self, style: SidebarStyle, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_sidebar_style(style, cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn select_separator_style(&mut self, style: SeparatorStyle, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_separator_style(style, cx);
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

    fn select_quick_ask_provider(&mut self, provider: AgentKind, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_quick_ask_agent(GenerationAgent::for_provider(provider), cx);
            workspace.save_now();
        });
        cx.notify();
    }

    fn select_quick_ask_model(&mut self, model: AgentModel, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            let mut quick_ask_agent = workspace.quick_ask_agent.clone();
            if model.belongs_to(quick_ask_agent.provider) {
                quick_ask_agent.model = model;
                workspace.set_quick_ask_agent(quick_ask_agent, cx);
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
                | "cmd-f"
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

    fn section_button(
        id: &'static str,
        target: SettingsSection,
        selected: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = selected == target;
        crate::ui::style::settings_nav_button(id, target.title(), is_selected, false, cx).on_click(
            cx.listener(move |this, _, _, cx| {
                this.section = target;
                this.recording_shortcut = None;
                this.shortcut_error = None;
                cx.notify();
            }),
        )
    }

    fn nested_section_button(
        id: &'static str,
        target: SettingsSection,
        selected: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = selected == target;
        crate::ui::style::settings_nav_button(id, target.title(), is_selected, true, cx).on_click(
            cx.listener(move |this, _, _, cx| {
                this.section = target;
                this.recording_shortcut = None;
                this.shortcut_error = None;
                cx.notify();
            }),
        )
    }

    fn brain_group_button(
        expanded: bool,
        selected: SettingsSection,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = matches!(selected, SettingsSection::Brain | SettingsSection::Memory);
        crate::ui::style::settings_nav_group_button(
            "settings-brain-group",
            "Brain",
            expanded,
            active,
            cx,
        )
        .on_click(cx.listener(|this, _, _, cx| {
            this.brain_expanded = !this.brain_expanded;
            cx.notify();
        }))
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
}

fn penpot_field_label(label: &'static str, cx: &App) -> gpui::AnyElement {
    div()
        .text_size(crate::ui::design::text_ui())
        .font_weight(FontWeight::MEDIUM)
        .text_color(crate::ui::design::t2(cx))
        .child(label)
        .into_any_element()
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
