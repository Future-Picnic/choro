use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::branding::{APP_ID, LEGACY_APP_ID};
use crate::project::{Project, ProjectId, ProjectSection};
use crate::{AgentEffort, AgentKind, AgentModel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

/// How Choro handles the optional, token-consuming verification pass after an
/// agent finishes implementation work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationMode {
    /// Offer verification when work finishes and let the user decide.
    #[default]
    Ask,
    /// Start verification immediately when eligible work finishes.
    Automatic,
    /// Do not offer or start verification automatically.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewChecklistMode {
    #[default]
    Automatic,
    Off,
}

/// How the project activity switcher (Code / Agents / Tasks / DB / Context) is
/// presented. This is a layout preference, independent of the color theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum NavStyle {
    /// Horizontal segmented tabs centered in the title bar (default).
    Tabs,
    /// A vertical icon rail docked at the far-left edge. Unlike the project
    /// sidebar, the rail stays visible when the sidebar is collapsed.
    Rail,
    /// A compact icon-over-label switcher anchored to the top-right of the
    /// title bar (next to the panels it drives), leaving the script/preset bar
    /// in the middle of the header.
    TopRight,
    /// The same vertical icon rail as `Rail`, but docked at the far-right edge
    /// of the window (outboard of the right panel it drives).
    #[default]
    RailRight,
}

/// Stable product-level identity for an activity that can appear in a
/// project's navigation rail. This lives in `ide-core` because the app default
/// and per-project visibility overrides are persisted in [`AppConfig`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectActivityId {
    Agents,
    Code,
    Tasks,
    Docs,
    Design,
    Db,
    Assets,
    Orbit,
    /// Forward-compatible fallback for activity ids introduced by a newer
    /// Choro build. Unknown entries are dropped during config migration.
    #[serde(other)]
    Unknown,
}

impl ProjectActivityId {
    pub const ALL: [Self; 8] = [
        Self::Agents,
        Self::Code,
        Self::Tasks,
        Self::Docs,
        Self::Design,
        Self::Db,
        Self::Assets,
        Self::Orbit,
    ];
}

pub fn default_pinned_project_activities() -> Vec<ProjectActivityId> {
    vec![
        ProjectActivityId::Agents,
        ProjectActivityId::Code,
        ProjectActivityId::Tasks,
        ProjectActivityId::Docs,
    ]
}

/// Drop unknowns and duplicates while restoring the product's canonical rail
/// order. Keeping this deterministic makes persisted overrides stable today
/// and leaves room for user-controlled ordering later.
pub fn normalized_project_activities(
    activities: impl IntoIterator<Item = ProjectActivityId>,
) -> Vec<ProjectActivityId> {
    let activities: Vec<ProjectActivityId> = activities.into_iter().collect();
    ProjectActivityId::ALL
        .into_iter()
        .filter(|activity| activities.contains(activity))
        .collect()
}

/// Presentation of changed files in the Git panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GitStatusViewMode {
    #[default]
    List,
    Tree,
}

/// Whether the Git status list is divided into Staged / Changes / Untracked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GitStatusGroupMode {
    None,
    #[default]
    Status,
}

/// Where the active edge of an agent conversation lives.
///
/// `Classic` keeps the familiar composer and newest messages at the bottom.
/// `TopDown` puts the composer first and renders the newest conversation row
/// directly underneath it, so new work grows down the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ConversationLayout {
    #[default]
    Classic,
    TopDown,
}

/// Surface treatment of the two side panels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidebarStyle {
    /// The theme-accent cast at the top of each panel.
    #[default]
    Colorful,
    /// The plain sidebar plane — no gradient at all. Also the landing spot for
    /// retired values (a colorless "subtle" lift once existed, but GPUI 0.2
    /// renders such shallow fades as visible 8-bit bands).
    #[serde(other)]
    Flat,
}

/// How the chrome's structural divider lines are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeparatorStyle {
    /// 1px rules that fade out toward their ends ("light instead of lines").
    #[default]
    Soft,
    /// Classic solid hairlines, edge to edge.
    Solid,
}

/// Explicit local opt-ins, stored separately from portable workspace configuration.
/// Missing settings and older installations keep experimental behavior disabled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct BetaFeatures {
    #[serde(default)]
    pub delegation: bool,
}

/// When a completed agent turn should create an operating-system notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionNotifications {
    Never,
    #[default]
    Background,
    Always,
}

/// Preferences for temporary operating-system notification pointers. Choro's
/// Board and unread attention state remain available regardless of permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationSettings {
    #[serde(default)]
    pub completion: CompletionNotifications,
    #[serde(default = "default_questions_and_approvals")]
    pub questions_and_approvals: bool,
    #[serde(default = "default_notification_sound")]
    pub sound: bool,
}

const fn default_questions_and_approvals() -> bool {
    true
}

const fn default_notification_sound() -> bool {
    true
}

const fn default_companion_enabled() -> bool {
    true
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            completion: CompletionNotifications::Background,
            questions_and_approvals: true,
            sound: true,
        }
    }
}

pub const COMPANION_PLAYLIST_SLOT_COUNT: usize = 4;
pub const COMPANION_MUSIC_MOOD_LABELS: [&str; COMPANION_PLAYLIST_SLOT_COUNT] =
    ["Deep Focus", "Lo-fi Flow", "Calm", "High Energy"];
pub const COMPANION_MUSIC_DEFAULT_URLS: [&str; COMPANION_PLAYLIST_SLOT_COUNT] = [
    "https://open.spotify.com/playlist/14KtkIpsvzDSCXR24EqHCL?nd=1&dlsi=c29797ee5956463d",
    "https://open.spotify.com/playlist/0EAo4yaK5HfxrsQXAqaOLz?nd=1&dlsi=258fb346eb6e4547",
    "https://open.spotify.com/playlist/6Uls6BAiuTRMfqEUyyeODT?si=8ad191b86c7a4764",
    "https://open.spotify.com/playlist/7CraD6gr9I0bJfPt2tQ1mL?nd=1&dlsi=45edee8da0c54025",
];

pub fn companion_music_mood_label(index: usize) -> &'static str {
    COMPANION_MUSIC_MOOD_LABELS
        .get(index)
        .copied()
        .unwrap_or("Music")
}

/// The Spotify playlists configured for one fixed companion music mood.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompanionPlaylist {
    pub label: String,
    pub urls: Vec<String>,
}

impl<'de> Deserialize<'de> for CompanionPlaylist {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct StoredPlaylist {
            #[serde(default)]
            label: String,
            urls: Option<Vec<String>>,
            url: Option<String>,
        }

        let stored = StoredPlaylist::deserialize(deserializer)?;
        Ok(Self {
            label: stored.label,
            // An explicitly empty list stays empty; the old single-link setting
            // becomes the first entry without changing the user's link.
            urls: stored
                .urls
                .unwrap_or_else(|| stored.url.into_iter().collect()),
        })
    }
}

impl CompanionPlaylist {
    pub fn display_label(&self, index: usize) -> String {
        companion_music_mood_label(index).to_string()
    }

    pub fn spotify_uris(&self) -> Vec<String> {
        let mut uris = Vec::new();
        for url in &self.urls {
            if let Some(uri) = spotify_playlist_uri(url) {
                if !uris.contains(&uri) {
                    uris.push(uri);
                }
            }
        }
        uris
    }
}

/// Credential-free companion music settings. Playback is delegated to the
/// locally installed Spotify desktop app; no Spotify developer app is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanionMusicSettings {
    #[serde(default = "default_companion_playlists")]
    pub playlists: [CompanionPlaylist; COMPANION_PLAYLIST_SLOT_COUNT],
}

fn default_companion_playlists() -> [CompanionPlaylist; COMPANION_PLAYLIST_SLOT_COUNT] {
    std::array::from_fn(|index| CompanionPlaylist {
        label: companion_music_mood_label(index).to_string(),
        urls: vec![COMPANION_MUSIC_DEFAULT_URLS[index].to_string()],
    })
}

impl Default for CompanionMusicSettings {
    fn default() -> Self {
        Self {
            playlists: default_companion_playlists(),
        }
    }
}

impl CompanionMusicSettings {
    fn migrated(mut self) -> Self {
        for (index, playlist) in self.playlists.iter_mut().enumerate() {
            playlist.label = companion_music_mood_label(index).to_string();
        }
        self
    }
}

/// Accept the canonical Spotify playlist URI or an open.spotify.com playlist
/// URL and normalize it for the desktop player's AppleScript command.
pub fn spotify_playlist_uri(value: &str) -> Option<String> {
    let value = value.trim();
    let id = if let Some(id) = value.strip_prefix("spotify:playlist:") {
        id.to_string()
    } else {
        let url = url::Url::parse(value).ok()?;
        if url.scheme() != "https" || url.host_str() != Some("open.spotify.com") {
            return None;
        }
        let mut segments = url.path_segments()?;
        if segments.next()? != "playlist" {
            return None;
        }
        segments.next()?.to_string()
    };
    (id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric()))
        .then(|| format!("spotify:playlist:{id}"))
}

/// How much spoken feedback Project Talk provides after a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceAnnouncements {
    /// Speak important outcomes and blockers, while leaving routine detail in
    /// the visible transcript.
    #[default]
    Balanced,
    /// Only speak errors and actions that need the user's attention.
    Minimal,
    /// Speak every completed Project Talk turn.
    All,
    /// Never synthesize speech; the transcript remains available.
    Off,
}

/// Local Project Talk and composer-dictation preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceSettings {
    /// Voice remains dormant until the user explicitly activates it.
    #[serde(default = "default_voice_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub announcements: VoiceAnnouncements,
    /// Number of days local text-only voice history is retained.
    #[serde(default = "default_voice_retention_days")]
    pub transcript_retention_days: u32,
    /// Wait for Smart Turn to judge a complete thought instead of stopping at
    /// the first short silence.
    #[serde(default = "default_patient_turn_taking")]
    pub patient_turn_taking: bool,
    /// Optional input-device name. `None` follows the macOS system default.
    #[serde(default)]
    pub input_device: Option<String>,
    /// Optional macOS system voice name. `None` uses the system default.
    #[serde(default)]
    pub system_voice: Option<String>,
    /// Multiplier applied to the native speech rate.
    #[serde(default = "default_voice_speech_rate")]
    pub speech_rate: f32,
}

const fn default_voice_enabled() -> bool {
    true
}

const fn default_voice_retention_days() -> u32 {
    30
}

const fn default_patient_turn_taking() -> bool {
    true
}

const fn default_voice_speech_rate() -> f32 {
    1.0
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            announcements: VoiceAnnouncements::Balanced,
            transcript_retention_days: 30,
            patient_turn_taking: true,
            input_device: None,
            system_voice: None,
            speech_rate: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PanelSizes {
    pub left: f32,
    pub right: f32,
}

impl Default for PanelSizes {
    fn default() -> Self {
        Self {
            left: 240.0,
            right: 380.0,
        }
    }
}

/// The signature theme shipped as the out-of-box default. Kept here (not in the
/// UI crate) so both the config default and serde fallback resolve to it without
/// a reverse dependency on `ide-app`.
pub const DEFAULT_THEME: &str = "Choro Dark";

fn default_theme_name() -> Option<String> {
    Some(DEFAULT_THEME.to_string())
}

fn default_sidebar_active_work() -> bool {
    true
}

pub const DEFAULT_OPENCODE_GENERATION_MODEL_ID: &str = "opencode/big-pickle";
pub const DEFAULT_OPENCODE_GENERATION_MODEL_LABEL: &str = "Big Pickle";
pub const DEFAULT_CODE_REVIEW_PROMPT: &str = "You are a senior engineer reviewing the current uncommitted changes (the working-tree diff — only the files that show as changed). Your value is precision: one wrong finding costs more trust than ten missed nitpicks, so report only issues you would flag in a serious human review.\n\nHow to work:\n1. First understand what the change is trying to do.\n2. Read the actual changed code plus enough surrounding context — callers, guards, error handling, tests — to know how it really behaves. Never infer a problem from a name or a diff hunk alone.\n3. Hunt in priority order: correctness bugs, data loss, security holes, race conditions, resource leaks, breaking behavior changes, performance regressions with a visible cost, and test gaps that would let this specific change break silently.\n4. Before reporting a finding, try to refute it: re-read the code and check whether a guard, caller, or existing test already handles the case. If you cannot describe a concrete sequence — this input or state leads to this wrong outcome — drop it.\n\nYou are reviewing for a developer who will NOT read the diff themselves. Every finding must make sense on its own:\n- The title names the problem in plain language (\"saves can silently overwrite each other\", not \"missing optimistic concurrency guard\").\n- \"What happens\" is the real consequence: what goes wrong, when, and what it costs — a freeze, lost data, a wrong screen, a bug that ships undetected. If an end user would see it, say so; if not, name the actual cost. Never restate code mechanics as the consequence.\n- The suggested fix is one concrete sentence a coding agent could act on directly.\n\nSeverities — when unsure, rate lower; over-escalation is how reviews lose trust:\n- Critical: will cause an outage, data loss, or is exploitable in realistic use.\n- High: a real correctness, security, or concurrency bug users will hit.\n- Medium: a likely bug or risky edge case with a plausible trigger.\n- Low: a minor but real problem worth fixing.\n\nRules that keep the review trustworthy:\n- Every finding cites real evidence: a `path:line` from the changed code plus the failing scenario. No line, no finding.\n- No style nitpicks, no subjective preferences, no \"could potentially\" speculation.\n- A clean result is a valid result: skip any severity with no findings, and if nothing real remains, say the changes look clean rather than inventing problems.";
pub const DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS: &str = "Put your entire review inside a single <code_review>…</code_review> block, written as Markdown:\n- Use a `## Critical` / `## High` / `## Medium` / `## Low` heading for each severity that has findings, highest first.\n- Under each, one list item per finding, in exactly this shape:\n  - **path:line — plain-language title**\n  What happens: the real consequence in one or two sentences, no code jargon.\n  Suggested fix: one concrete sentence describing the change to make.\n- If there are no real issues, write a single line inside the block saying the changes look clean.\n\nWrite nothing outside the <code_review> block.";

/// Retired defaults from older builds. Defaults are written to config.json on
/// save, so a stored value equal to one of these is an untouched default (not a
/// user edit) and should follow the current default instead of shadowing it.
const LEGACY_CODE_REVIEW_PROMPTS: &[&str] = &["You are a senior engineer doing a focused code review of the current uncommitted changes (the working-tree diff — only the files that show as changed). Read the actual changed code before forming any conclusion.\n\nReport only issues that genuinely matter, grouped into these severities:\n- Critical: will cause an outage, data loss, or is exploitable.\n- High: a real correctness, security, or concurrency bug with concrete impact.\n- Medium: a likely bug, risky edge case, or measurable regression.\n- Low: a minor but real problem worth fixing.\n\nRules that keep the review trustworthy:\n- Every finding must cite real evidence: a `path:line` from the changed code, not an inference from a name. If you cannot point to a line, do not report it.\n- No style nitpicks, no subjective preferences, no speculation. Do not invent problems to fill a category — skip any severity with no findings."];
const LEGACY_CODE_REVIEW_OUTPUT_INSTRUCTIONS: &[&str] = &["Put your entire review inside a single <code_review>…</code_review> block, written as Markdown:\n- Use a `## Critical` / `## High` / `## Medium` / `## Low` heading for each severity that has findings, highest first.\n- Under each, one list item per finding starting with `**path:line — short title**`, then on the next lines a one-sentence explanation of the impact and a concrete suggested fix.\n- If there are no real issues, write a single line inside the block saying the changes look clean.\n\nWrite nothing outside the <code_review> block."];

fn default_code_review_prompt() -> String {
    DEFAULT_CODE_REVIEW_PROMPT.to_string()
}

fn default_code_review_output_instructions() -> String {
    DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS.to_string()
}

/// The coding agent Choro uses for small, one-shot writing jobs such as commit
/// messages, pull-request copy, Riff instructions, and Ship content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenerationAgent {
    pub provider: AgentKind,
    pub model: AgentModel,
    #[serde(default)]
    pub external_model_id: Option<String>,
    #[serde(default)]
    pub external_model_label: Option<String>,
}

impl Default for GenerationAgent {
    fn default() -> Self {
        Self::for_provider(AgentKind::Codex)
    }
}

impl GenerationAgent {
    pub fn for_provider(provider: AgentKind) -> Self {
        match provider {
            AgentKind::Codex => Self {
                provider,
                model: AgentModel::CodexGpt56Luna,
                external_model_id: None,
                external_model_label: None,
            },
            AgentKind::Claude => Self {
                provider,
                model: AgentModel::ClaudeHaiku45,
                external_model_id: None,
                external_model_label: None,
            },
            AgentKind::OpenCode => Self {
                provider,
                model: AgentModel::OpenCode,
                external_model_id: Some(DEFAULT_OPENCODE_GENERATION_MODEL_ID.to_string()),
                external_model_label: Some(DEFAULT_OPENCODE_GENERATION_MODEL_LABEL.to_string()),
            },
        }
    }

    pub fn model_label(&self) -> &str {
        self.external_model_label
            .as_deref()
            .filter(|_| self.provider == AgentKind::OpenCode)
            .unwrap_or_else(|| self.model.label())
    }

    pub fn model_cli_value(&self) -> Option<&str> {
        if self.provider == AgentKind::OpenCode {
            self.external_model_id.as_deref()
        } else {
            self.model.cli_value()
        }
    }

    pub fn normalized(mut self) -> Self {
        if !self.model.belongs_to(self.provider)
            || (self.provider == AgentKind::OpenCode && self.external_model_id.is_none())
        {
            return Self::for_provider(self.provider);
        }
        if self.provider != AgentKind::OpenCode {
            self.external_model_id = None;
            self.external_model_label = None;
        }
        self
    }
}

/// What a fresh agent starts with in the composer: provider, model, and
/// reasoning effort. Deliberately separate from [`GenerationAgent`] — that one
/// runs Choro's own one-shot writing jobs and favors small fast models, while
/// these defaults seed real working agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewAgentDefaults {
    pub provider: AgentKind,
    pub model: AgentModel,
    #[serde(default)]
    pub effort: AgentEffort,
    #[serde(default)]
    pub external_model_id: Option<String>,
    #[serde(default)]
    pub external_model_label: Option<String>,
}

impl NewAgentDefaults {
    pub fn for_provider(provider: AgentKind) -> Self {
        let model = AgentModel::default_for(provider);
        Self {
            provider,
            model,
            effort: model.default_effort(),
            external_model_id: (provider == AgentKind::OpenCode)
                .then(|| DEFAULT_OPENCODE_GENERATION_MODEL_ID.to_string()),
            external_model_label: (provider == AgentKind::OpenCode)
                .then(|| DEFAULT_OPENCODE_GENERATION_MODEL_LABEL.to_string()),
        }
    }

    pub fn normalized(mut self) -> Self {
        if !self.model.belongs_to(self.provider) {
            return Self::for_provider(self.provider);
        }
        if self.provider != AgentKind::OpenCode {
            self.external_model_id = None;
            self.external_model_label = None;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub projects: Vec<Project>,
    #[serde(default)]
    pub project_sections: Vec<ProjectSection>,
    #[serde(default)]
    pub active_project: Option<ProjectId>,
    #[serde(default)]
    pub panels: PanelSizes,
    #[serde(default)]
    pub theme: ThemeMode,
    /// Name of a registered UI theme (e.g. "Choro Dark"); overrides `theme`.
    /// Defaults to the signature theme when a config predates this field.
    #[serde(default = "default_theme_name")]
    pub theme_name: Option<String>,
    /// Keyboard shortcut overrides: shortcut id → keystroke (e.g. "cmd-b").
    #[serde(default)]
    pub keymap: std::collections::HashMap<String, String>,
    /// Project ids whose nested sidebar agent list is expanded.
    #[serde(default)]
    pub expanded_projects: Vec<ProjectId>,
    /// Whether the built-in Favorites sidebar group is collapsed.
    #[serde(default)]
    pub favorites_collapsed: bool,
    /// Whether the built-in Projects sidebar group is collapsed.
    #[serde(default)]
    pub projects_collapsed: bool,
    /// Whether the built-in Needs attention sidebar group is collapsed.
    #[serde(default)]
    pub attention_collapsed: bool,
    /// Agent conversations the user keeps at the top of the sidebar.
    #[serde(default)]
    pub pinned_agents: Vec<Uuid>,
    /// Whether the built-in Pinned sidebar group is collapsed.
    #[serde(default)]
    pub pinned_agents_collapsed: bool,
    /// Whether the sidebar project list shows only active work (agents that
    /// are working or waiting) instead of every in-progress agent.
    #[serde(default = "default_sidebar_active_work")]
    pub sidebar_active_work: bool,
    /// Legacy persisted layout. The application now always renders RailRight;
    /// this field remains so older config files continue to deserialize.
    #[serde(default)]
    pub nav_style: NavStyle,
    /// Activities inherited by projects that do not have a local override.
    #[serde(default = "default_pinned_project_activities")]
    pub default_project_activities: Vec<ProjectActivityId>,
    /// Project-specific activity visibility. Missing entries inherit
    /// `default_project_activities`.
    #[serde(default)]
    pub project_activity_overrides: HashMap<ProjectId, Vec<ProjectActivityId>>,
    /// Flat filename list or expandable directory tree in the Git panel.
    #[serde(default)]
    pub git_status_view: GitStatusViewMode,
    /// Optional status-section grouping for changed files.
    #[serde(default)]
    pub git_status_group: GitStatusGroupMode,
    /// Placement and reading direction of agent conversations.
    #[serde(default)]
    pub conversation_layout: ConversationLayout,
    /// Surface treatment of the two side panels.
    #[serde(default)]
    pub sidebar_style: SidebarStyle,
    /// How the chrome's structural divider lines are drawn.
    #[serde(default)]
    pub separator_style: SeparatorStyle,
    /// Operating-system notification preferences.
    #[serde(default)]
    pub notifications: NotificationSettings,
    /// Whether the always-on-top desktop companion is shown. While it is
    /// shown, it replaces temporary operating-system notification banners.
    #[serde(default = "default_companion_enabled")]
    pub companion_enabled: bool,
    /// Four locally controlled Spotify playlists for the floating companion.
    #[serde(default)]
    pub companion_music: CompanionMusicSettings,
    /// Local Project Talk and composer dictation preferences.
    #[serde(default)]
    pub voice: VoiceSettings,
    /// Provider and model used by one-shot AI generation features.
    #[serde(default)]
    pub generation_agent: GenerationAgent,
    /// Provider and model used when a new Quick Ask session opens.
    #[serde(default)]
    pub quick_ask_agent: GenerationAgent,
    /// Defaults a fresh agent composer opens with. `None` in configs from
    /// before the split — those fall back to the generation agent, which used
    /// to double as the composer default.
    #[serde(default)]
    pub new_agent_defaults: Option<NewAgentDefaults>,
    /// Shared by every model picker; older configurations start without favorites.
    #[serde(default)]
    pub favorite_models: Vec<crate::model_favorites::ModelFavorite>,
    /// Instruction sent when the Code review action is used in an agent chat.
    #[serde(default = "default_code_review_prompt")]
    pub code_review_prompt: String,
    /// App-owned response contract appended to every code-review request.
    #[serde(default = "default_code_review_output_instructions")]
    pub code_review_output_instructions: String,
    /// Whether user decisions (plan feedback, answers) may be distilled into
    /// "Remember this?" memory proposals. Nothing is saved without the user
    /// accepting the card; this only gates the suggestion pass.
    #[serde(default = "default_memory_proposals_enabled")]
    pub memory_proposals_enabled: bool,
    /// Whether finished agent work is offered for verification, verified
    /// immediately, or left alone.
    #[serde(default)]
    pub verification_mode: VerificationMode,
    /// Whether file-changing turns automatically produce a manual checklist.
    #[serde(default)]
    pub review_checklist_mode: ReviewChecklistMode,
    /// Whether starting an implementation agent may open its linked design in
    /// the user's browser without showing the explanatory confirmation first.
    #[serde(default)]
    pub design_browser_open_prompt_dismissed: bool,
}

fn default_memory_proposals_enabled() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            projects: Vec::new(),
            project_sections: Vec::new(),
            active_project: None,
            panels: PanelSizes::default(),
            theme: ThemeMode::default(),
            theme_name: default_theme_name(),
            keymap: std::collections::HashMap::new(),
            expanded_projects: Vec::new(),
            favorites_collapsed: false,
            projects_collapsed: false,
            attention_collapsed: false,
            pinned_agents: Vec::new(),
            pinned_agents_collapsed: false,
            sidebar_active_work: true,
            nav_style: NavStyle::default(),
            default_project_activities: default_pinned_project_activities(),
            project_activity_overrides: HashMap::new(),
            git_status_view: GitStatusViewMode::default(),
            git_status_group: GitStatusGroupMode::default(),
            conversation_layout: ConversationLayout::default(),
            sidebar_style: SidebarStyle::default(),
            separator_style: SeparatorStyle::default(),
            notifications: NotificationSettings::default(),
            companion_enabled: true,
            companion_music: CompanionMusicSettings::default(),
            voice: VoiceSettings::default(),
            generation_agent: GenerationAgent::default(),
            quick_ask_agent: GenerationAgent::default(),
            new_agent_defaults: None,
            favorite_models: Vec::new(),
            code_review_prompt: default_code_review_prompt(),
            code_review_output_instructions: default_code_review_output_instructions(),
            memory_proposals_enabled: true,
            verification_mode: VerificationMode::Ask,
            review_checklist_mode: ReviewChecklistMode::Automatic,
            design_browser_open_prompt_dismissed: false,
        }
    }
}

impl AppConfig {
    pub fn config_root() -> PathBuf {
        if let Some(root) = std::env::var_os("CHORO_DATA_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
        {
            return root;
        }
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        resolve_brand_config_root(&base)
    }

    pub fn config_path() -> PathBuf {
        Self::config_root().join("config.json")
    }

    pub fn project_icons_dir() -> PathBuf {
        Self::config_path()
            .parent()
            .map(|path| path.join("project-icons"))
            .unwrap_or_else(|| PathBuf::from("project-icons"))
    }

    pub fn project_chat_attachments_dir(project: ProjectId) -> PathBuf {
        Self::config_path()
            .parent()
            .map(|path| {
                path.join("project-attachments")
                    .join(project.0.to_string())
                    .join("chat")
            })
            .unwrap_or_else(|| {
                PathBuf::from("project-attachments")
                    .join(project.0.to_string())
                    .join("chat")
            })
    }

    pub fn load() -> Self {
        Self::load_from(&Self::config_path())
    }

    /// Missing or corrupt files fall back to defaults — the app must always start.
    pub fn load_from(path: &PathBuf) -> Self {
        match fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text)
                .map(Self::migrated)
                .unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Replace stored copies of retired default prompts with the current
    /// defaults, so default-following configs pick up prompt improvements while
    /// user-customized prompts stay untouched.
    fn migrated(mut self) -> Self {
        if LEGACY_CODE_REVIEW_PROMPTS.contains(&self.code_review_prompt.as_str()) {
            self.code_review_prompt = default_code_review_prompt();
        }
        if LEGACY_CODE_REVIEW_OUTPUT_INSTRUCTIONS
            .contains(&self.code_review_output_instructions.as_str())
        {
            self.code_review_output_instructions = default_code_review_output_instructions();
        }
        self.companion_music = self.companion_music.migrated();
        self.default_project_activities =
            normalized_project_activities(self.default_project_activities);
        if self.default_project_activities.is_empty() {
            self.default_project_activities = default_pinned_project_activities();
        }
        self.project_activity_overrides.retain(|_, activities| {
            *activities = normalized_project_activities(activities.iter().copied());
            !activities.is_empty()
        });
        self
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path())
    }

    /// Atomic save: write to a temp file in the same directory, then rename.
    pub fn save_to(&self, path: &PathBuf) -> Result<()> {
        let dir = path
            .parent()
            .context("config path has no parent directory")?;
        fs::create_dir_all(dir).context("failed to create config directory")?;
        let json = serde_json::to_string_pretty(self).context("failed to serialize config")?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json).context("failed to write temp config file")?;
        fs::rename(&tmp, path).context("failed to move config file into place")?;
        Ok(())
    }
}

/// Prefer Choro's current data directory. When only the former My IDE
/// directory exists, rename it atomically and retain a compatibility symlink
/// on Unix; if the rename fails, keep using the old directory so an upgrade can
/// never make the user's local data disappear.
fn resolve_brand_config_root(base: &Path) -> PathBuf {
    let current = base.join(APP_ID);
    if current.exists() {
        return current;
    }
    let legacy = base.join(LEGACY_APP_ID);
    if legacy.exists() {
        if fs::rename(&legacy, &current).is_ok() {
            #[cfg(unix)]
            {
                let _ = std::os::unix::fs::symlink(&current, &legacy);
            }
            return current;
        }
        return legacy;
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::{ProjectSection, ScriptPreset};

    fn sample_config() -> AppConfig {
        let mut project = Project::from_path(PathBuf::from("/tmp/app server"));
        let mut section = ProjectSection::new("Work");
        section.collapsed = true;
        project.section_id = Some(section.id);
        project
            .presets
            .push(ScriptPreset::new("run server", "npm run dev"));
        let project_id = project.id;
        AppConfig {
            active_project: Some(project.id),
            expanded_projects: vec![project.id],
            project_sections: vec![section],
            favorites_collapsed: true,
            projects_collapsed: true,
            attention_collapsed: true,
            pinned_agents: Vec::new(),
            pinned_agents_collapsed: true,
            sidebar_active_work: false,
            projects: vec![project],
            panels: PanelSizes {
                left: 200.0,
                right: 400.0,
            },
            theme: ThemeMode::Dark,
            theme_name: Some("Choro Dark".into()),
            keymap: [("toggle_left".to_string(), "cmd-b".to_string())]
                .into_iter()
                .collect(),
            nav_style: NavStyle::Rail,
            default_project_activities: vec![
                ProjectActivityId::Agents,
                ProjectActivityId::Code,
                ProjectActivityId::Db,
            ],
            project_activity_overrides: [(
                project_id,
                vec![ProjectActivityId::Agents, ProjectActivityId::Db],
            )]
            .into_iter()
            .collect(),
            git_status_view: GitStatusViewMode::Tree,
            git_status_group: GitStatusGroupMode::None,
            conversation_layout: ConversationLayout::TopDown,
            sidebar_style: SidebarStyle::Flat,
            separator_style: SeparatorStyle::Solid,
            notifications: NotificationSettings {
                completion: CompletionNotifications::Always,
                questions_and_approvals: false,
                sound: false,
            },
            companion_enabled: false,
            companion_music: CompanionMusicSettings::default(),
            generation_agent: GenerationAgent::default(),
            quick_ask_agent: GenerationAgent::default(),
            voice: VoiceSettings::default(),
            new_agent_defaults: Some(NewAgentDefaults::for_provider(AgentKind::Claude)),
            favorite_models: vec![crate::model_favorites::ModelFavorite::BuiltIn(
                AgentModel::ClaudeHaiku45,
            )],
            code_review_prompt: default_code_review_prompt(),
            code_review_output_instructions: default_code_review_output_instructions(),
            memory_proposals_enabled: true,
            verification_mode: VerificationMode::Ask,
            review_checklist_mode: ReviewChecklistMode::Automatic,
            design_browser_open_prompt_dismissed: false,
        }
    }

    #[test]
    fn normalizes_spotify_playlist_links_and_uris() {
        let id = "37i9dQZF1DX8Uebhn9wzrS";
        let expected = format!("spotify:playlist:{id}");
        assert_eq!(
            spotify_playlist_uri(&format!("https://open.spotify.com/playlist/{id}?si=abc123")),
            Some(expected.clone())
        );
        assert_eq!(
            spotify_playlist_uri(&format!("spotify:playlist:{id}")),
            Some(expected)
        );
    }

    #[test]
    fn rejects_non_playlist_or_non_spotify_music_links() {
        assert_eq!(
            spotify_playlist_uri("https://open.spotify.com/track/6rqhFgbbKwnb9MLmUQDhG6"),
            None
        );
        assert_eq!(
            spotify_playlist_uri("https://example.com/playlist/37i9dQZF1DX8Uebhn9wzrS"),
            None
        );
        assert_eq!(spotify_playlist_uri("spotify:playlist:not-an-id"), None);
    }

    #[test]
    fn companion_music_slots_have_fixed_mood_names() {
        let settings = CompanionMusicSettings::default();
        let labels = settings
            .playlists
            .iter()
            .enumerate()
            .map(|(index, playlist)| playlist.display_label(index))
            .collect::<Vec<_>>();
        assert_eq!(labels, COMPANION_MUSIC_MOOD_LABELS.map(str::to_string));
        let urls = settings
            .playlists
            .iter()
            .map(|playlist| playlist.urls[0].clone())
            .collect::<Vec<_>>();
        assert_eq!(
            urls,
            COMPANION_MUSIC_DEFAULT_URLS.map(str::to_string).to_vec()
        );
        assert!(settings
            .playlists
            .iter()
            .all(|playlist| !playlist.spotify_uris().is_empty()));
    }

    #[test]
    fn companion_music_migrates_legacy_links_and_keeps_fixed_labels() {
        let custom = "https://open.spotify.com/playlist/37i9dQZF1DX8Uebhn9wzrS";
        let mut stored = serde_json::to_value(CompanionMusicSettings::default()).unwrap();
        stored["playlists"][0] = serde_json::json!({"label": "Custom focus", "url": custom});
        let settings: CompanionMusicSettings = serde_json::from_value(stored).unwrap();
        let migrated = settings.migrated();
        assert_eq!(migrated.playlists[0].urls, vec![custom]);
        assert_eq!(migrated.playlists[0].label, COMPANION_MUSIC_MOOD_LABELS[0]);
        assert_eq!(
            migrated.playlists[1].urls,
            vec![COMPANION_MUSIC_DEFAULT_URLS[1]]
        );
    }

    #[test]
    fn companion_music_round_trips_multiple_links_and_empty_moods() {
        let mut settings = CompanionMusicSettings::default();
        settings.playlists[0]
            .urls
            .push(COMPANION_MUSIC_DEFAULT_URLS[1].to_string());
        settings.playlists[1].urls.clear();
        settings.playlists[2].urls = vec![String::new()];
        let serialized = serde_json::to_string(&settings).unwrap();
        let loaded: CompanionMusicSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(loaded.migrated(), settings);
    }

    #[test]
    fn companion_music_ignores_invalid_and_duplicate_playlist_links() {
        let id = "37i9dQZF1DX8Uebhn9wzrS";
        let playlist = CompanionPlaylist {
            label: String::new(),
            urls: vec![
                String::new(),
                "https://example.com/not-a-playlist".to_string(),
                format!("https://open.spotify.com/playlist/{id}?si=first"),
                format!("spotify:playlist:{id}"),
                COMPANION_MUSIC_DEFAULT_URLS[1].to_string(),
            ],
        };
        assert_eq!(
            playlist.spotify_uris(),
            vec![
                format!("spotify:playlist:{id}"),
                spotify_playlist_uri(COMPANION_MUSIC_DEFAULT_URLS[1]).unwrap(),
            ]
        );
    }

    #[test]
    fn companion_music_empty_list_takes_precedence_over_legacy_link() {
        let playlist: CompanionPlaylist = serde_json::from_value(serde_json::json!({
            "url": COMPANION_MUSIC_DEFAULT_URLS[0], "urls": []
        }))
        .unwrap();
        assert!(playlist.urls.is_empty());
    }

    #[test]
    fn legacy_app_data_directory_is_migrated_without_losing_files() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join(LEGACY_APP_ID);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("state.db"), "local data").unwrap();

        let resolved = resolve_brand_config_root(dir.path());

        assert_eq!(resolved, dir.path().join(APP_ID));
        assert_eq!(
            fs::read_to_string(resolved.join("state.db")).unwrap(),
            "local data"
        );
        #[cfg(unix)]
        assert_eq!(
            fs::canonicalize(legacy).unwrap(),
            fs::canonicalize(resolved).unwrap()
        );
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        let config = sample_config();
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(config, loaded);
    }

    #[test]
    fn model_favorites_default_to_empty_for_older_configs() {
        let mut json = serde_json::to_value(sample_config()).unwrap();
        json.as_object_mut().unwrap().remove("favorite_models");
        let config: AppConfig = serde_json::from_value(json).unwrap();
        assert!(config.favorite_models.is_empty());
    }

    #[test]
    fn model_favorites_round_trip_without_changing_model_defaults() {
        use crate::model_favorites::ModelFavorite;
        let mut config = sample_config();
        config
            .favorite_models
            .push(ModelFavorite::OpenCode("anthropic/claude-sonnet".into()));
        let restored: AppConfig =
            serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap();
        assert_eq!(restored.favorite_models, config.favorite_models);
        assert_eq!(restored.new_agent_defaults, config.new_agent_defaults);
        assert_eq!(restored.quick_ask_agent, config.quick_ask_agent);
    }

    #[test]
    fn older_configs_get_the_product_activity_defaults() {
        let mut json = serde_json::to_value(sample_config()).unwrap();
        let object = json.as_object_mut().unwrap();
        object.remove("default_project_activities");
        object.remove("project_activity_overrides");

        let config = serde_json::from_value::<AppConfig>(json)
            .unwrap()
            .migrated();

        assert_eq!(
            config.default_project_activities,
            vec![
                ProjectActivityId::Agents,
                ProjectActivityId::Code,
                ProjectActivityId::Tasks,
                ProjectActivityId::Docs,
            ]
        );
        assert!(config.project_activity_overrides.is_empty());
    }

    #[test]
    fn older_configs_get_the_quick_ask_default_model() {
        let mut json = serde_json::to_value(sample_config()).unwrap();
        json.as_object_mut().unwrap().remove("quick_ask_agent");

        let config = serde_json::from_value::<AppConfig>(json).unwrap();

        assert_eq!(config.quick_ask_agent, GenerationAgent::default());
    }

    #[test]
    fn older_configs_default_the_sidebar_to_active_work() {
        let mut json = serde_json::to_value(sample_config()).unwrap();
        json.as_object_mut().unwrap().remove("sidebar_active_work");

        let config = serde_json::from_value::<AppConfig>(json).unwrap();

        assert!(config.sidebar_active_work);
    }

    #[test]
    fn sidebar_view_choice_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = AppConfig {
            sidebar_active_work: false,
            ..AppConfig::default()
        };
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert!(!loaded.sidebar_active_work);
    }

    #[test]
    fn activity_preferences_are_normalized_during_migration() {
        let mut config = sample_config();
        let project_id = config.projects[0].id;
        config.default_project_activities = vec![
            ProjectActivityId::Db,
            ProjectActivityId::Agents,
            ProjectActivityId::Db,
            ProjectActivityId::Unknown,
        ];
        config.project_activity_overrides.insert(
            project_id,
            vec![
                ProjectActivityId::Assets,
                ProjectActivityId::Code,
                ProjectActivityId::Assets,
            ],
        );

        let migrated = config.migrated();

        assert_eq!(
            migrated.default_project_activities,
            vec![ProjectActivityId::Agents, ProjectActivityId::Db]
        );
        assert_eq!(
            migrated.project_activity_overrides.get(&project_id),
            Some(&vec![ProjectActivityId::Code, ProjectActivityId::Assets,])
        );
    }

    #[test]
    fn default_code_review_prompt_is_written_to_json() {
        let json = serde_json::to_value(AppConfig::default()).unwrap();
        assert_eq!(
            json.get("code_review_prompt")
                .and_then(serde_json::Value::as_str),
            Some(DEFAULT_CODE_REVIEW_PROMPT)
        );
    }

    #[test]
    fn stored_legacy_review_prompt_is_migrated_to_current_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut config = sample_config();
        config.code_review_prompt = LEGACY_CODE_REVIEW_PROMPTS[0].to_string();
        config.code_review_output_instructions =
            LEGACY_CODE_REVIEW_OUTPUT_INSTRUCTIONS[0].to_string();
        config.save_to(&path).unwrap();

        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.code_review_prompt, DEFAULT_CODE_REVIEW_PROMPT);
        assert_eq!(
            loaded.code_review_output_instructions,
            DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS
        );
    }

    #[test]
    fn customized_review_prompt_survives_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut config = sample_config();
        config.code_review_prompt = "my own review rules".to_string();
        config.save_to(&path).unwrap();

        let loaded = AppConfig::load_from(&path);
        assert_eq!(loaded.code_review_prompt, "my own review rules");
    }

    #[test]
    fn missing_file_falls_back_to_default() {
        let path = PathBuf::from("/nonexistent/config.json");
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
    }

    #[test]
    fn corrupt_file_falls_back_to_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "{ not json !!!").unwrap();
        assert_eq!(AppConfig::load_from(&path), AppConfig::default());
    }

    #[test]
    fn older_config_projects_default_visuals() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(
            &path,
            r#"{
                "projects": [
                    {
                        "id": "0f2b8585-390c-4b9a-9e3a-ef2ae3a94e5f",
                        "name": "old app",
                        "path": "/tmp/old-app"
                    }
                ]
            }"#,
        )
        .unwrap();

        let loaded = AppConfig::load_from(&path);
        let project = loaded.projects.first().unwrap();
        assert_eq!(project.icon, crate::project::DEFAULT_PROJECT_ICON);
        assert_eq!(
            project.icon_color,
            crate::project::DEFAULT_PROJECT_ICON_COLOR
        );
        assert_eq!(project.section_id, None);
        assert!(!project.is_favorite);
        assert!(loaded.project_sections.is_empty());
        assert!(!loaded.favorites_collapsed);
        assert!(!loaded.projects_collapsed);
        assert_eq!(loaded.git_status_view, GitStatusViewMode::List);
        assert_eq!(loaded.git_status_group, GitStatusGroupMode::Status);
        assert_eq!(loaded.conversation_layout, ConversationLayout::Classic);
        assert_eq!(loaded.notifications, NotificationSettings::default());
        assert!(loaded.companion_enabled);
        assert_eq!(loaded.companion_music, CompanionMusicSettings::default());
        assert_eq!(loaded.verification_mode, VerificationMode::Ask);
        assert_eq!(loaded.review_checklist_mode, ReviewChecklistMode::Automatic);
    }

    #[test]
    fn project_sections_and_favorites_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut project = Project::from_path(PathBuf::from("/tmp/favorite-app"));
        let section = ProjectSection::new("Ops");
        let pinned_agent = Uuid::new_v4();
        project.section_id = Some(section.id);
        project.is_favorite = true;
        let config = AppConfig {
            projects: vec![project],
            project_sections: vec![section],
            favorites_collapsed: true,
            projects_collapsed: true,
            pinned_agents: vec![pinned_agent],
            pinned_agents_collapsed: true,
            ..AppConfig::default()
        };
        config.save_to(&path).unwrap();
        let loaded = AppConfig::load_from(&path);
        assert_eq!(config, loaded);
    }

    #[test]
    fn save_is_atomic_no_tmp_left_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        sample_config().save_to(&path).unwrap();
        assert!(path.exists());
        assert!(!path.with_extension("json.tmp").exists());
    }
}
