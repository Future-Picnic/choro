use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, prelude::FluentBuilder, px, Animation, AnimationExt, App, AppContext, Bounds,
    Context, Div, Entity, Global, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{button::ButtonVariants, h_flex, v_flex, Disableable, Icon, IconName};
use ide_core::ProjectId;
use uuid::Uuid;

use crate::onboarding::{manifest, OnboardingManifest, OnboardingProgress};
use crate::state::agent_chat::AgentChatStatus;
use crate::state::{AgentChatState, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::right_panel::RightPanel;
use crate::ui::style;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpotlightTarget {
    ProjectSidebar,
    ProjectTools,
    WorkArea,
    ChangedFiles,
    AgentPlan,
    AgentContext,
    ShipResult,
    GitPanel,
    DocsNav,
    TasksNav,
    Questions,
    NewAgent,
    Composer,
    ComposerPlan,
    ComposerSend,
    DocImplement,
    DocBody,
    AssetBody,
    PreviewActionZone,
    PlanCard,
    PlanApprove,
    TaskImplement,
    Ship,
    ShipNewBranch,
    ShipAllChanges,
    ShipPush,
    ShipDialog,
    ShipPrimary,
    RunScript,
    AddProject,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentSource {
    FirstPrompt,
    Doc,
    Task,
}

/// The tools someone already juggles. Each maps to a Choro panel, but the tour
/// never says so — it just asks what you use, then shows them pulled together on
/// the page you build. Order is the order they appear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackTool {
    Database,
    GitHub,
    Docs,
    Agents,
    Editor,
    Issues,
    Design,
    Scripts,
}

impl StackTool {
    pub const ALL: [StackTool; 8] = [
        StackTool::Database,
        StackTool::GitHub,
        StackTool::Docs,
        StackTool::Agents,
        StackTool::Editor,
        StackTool::Issues,
        StackTool::Design,
        StackTool::Scripts,
    ];

    /// Shown on the chip and, later, on the page node.
    fn label(self) -> &'static str {
        match self {
            StackTool::Database => "Database",
            StackTool::GitHub => "GitHub",
            StackTool::Docs => "Docs",
            StackTool::Agents => "AI agents",
            StackTool::Editor => "Editor",
            StackTool::Issues => "Issues",
            StackTool::Design => "Design",
            StackTool::Scripts => "Scripts",
        }
    }

    /// The familiar names under it, so the category reads as "oh, that's me".
    fn examples(self) -> &'static str {
        match self {
            StackTool::Database => "Mongo · Postgres · Supabase",
            StackTool::GitHub => "branches · commits · PRs",
            StackTool::Docs => "Notion · Google Docs",
            StackTool::Agents => "Claude Code · Codex · OpenCode",
            StackTool::Editor => "VS Code · Cursor",
            StackTool::Issues => "Jira · Linear · Monday",
            StackTool::Design => "Figma · Sketch",
            StackTool::Scripts => "npm · make · shell",
        }
    }

    fn glyph(self) -> lucide_icons::Icon {
        use lucide_icons::Icon as L;
        match self {
            StackTool::Database => L::Database,
            StackTool::GitHub => L::GitBranch,
            StackTool::Docs => L::BookOpen,
            StackTool::Agents => L::Bot,
            StackTool::Editor => L::Code,
            StackTool::Issues => L::ListTodo,
            StackTool::Design => L::PenTool,
            StackTool::Scripts => L::SquareTerminal,
        }
    }

    /// Stable slug the page reads from `stack.json`.
    fn slug(self) -> &'static str {
        match self {
            StackTool::Database => "database",
            StackTool::GitHub => "github",
            StackTool::Docs => "docs",
            StackTool::Agents => "agents",
            StackTool::Editor => "editor",
            StackTool::Issues => "issues",
            StackTool::Design => "designs",
            StackTool::Scripts => "scripts",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.slug() == slug)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingProviderChoice {
    Claude,
    Codex,
    OpenCode,
}

impl OnboardingProviderChoice {
    const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::OpenCode];
    const DEFAULT_PRIORITY: [Self; 3] = [Self::Codex, Self::Claude, Self::OpenCode];

    fn key(self) -> String {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::OpenCode => "opencode",
        }
        .to_string()
    }

    fn from_key(key: &str) -> Option<Self> {
        match key {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "opencode" => Some(Self::OpenCode),
            _ => None,
        }
    }
}

fn default_provider_choice(
    choices: &[OnboardingProviderChoice],
) -> Option<OnboardingProviderChoice> {
    OnboardingProviderChoice::DEFAULT_PRIORITY
        .into_iter()
        .find(|provider| choices.contains(provider))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderConnectionStatus {
    Checking,
    Connected,
    NeedsSignIn,
    NotInstalled,
    Unavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ProviderConnectionStatuses {
    claude: ProviderConnectionStatus,
    codex: ProviderConnectionStatus,
    opencode: ProviderConnectionStatus,
}

impl Default for ProviderConnectionStatuses {
    fn default() -> Self {
        Self {
            claude: ProviderConnectionStatus::Checking,
            codex: ProviderConnectionStatus::Checking,
            opencode: ProviderConnectionStatus::Checking,
        }
    }
}

impl ProviderConnectionStatuses {
    fn for_provider(self, provider: OnboardingProviderChoice) -> ProviderConnectionStatus {
        match provider {
            OnboardingProviderChoice::Claude => self.claude,
            OnboardingProviderChoice::Codex => self.codex,
            OnboardingProviderChoice::OpenCode => self.opencode,
        }
    }
}

const PROVIDER_STATUS_TIMEOUT: Duration = Duration::from_secs(4);

fn provider_status_output(executable: &Path, arguments: &[&str]) -> Option<Output> {
    let mut child = Command::new(executable)
        .args(arguments)
        .env(
            "PATH",
            crate::state::agent_chat::protocol::agent_command_path_env(),
        )
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + PROVIDER_STATUS_TIMEOUT;

    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) | Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn claude_connection_status(output: &Output) -> ProviderConnectionStatus {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("\"loggedIn\": true") {
        ProviderConnectionStatus::Connected
    } else if stdout.contains("\"loggedIn\": false") || !output.status.success() {
        ProviderConnectionStatus::NeedsSignIn
    } else {
        ProviderConnectionStatus::Unavailable
    }
}

fn opencode_connection_status(output: &Output) -> ProviderConnectionStatus {
    if !output.status.success() {
        return ProviderConnectionStatus::NeedsSignIn;
    }
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let fields = text.split_whitespace().collect::<Vec<_>>();
    let credential_count = fields.windows(2).find_map(|pair| {
        pair[1]
            .trim_matches(|character: char| !character.is_ascii_alphabetic())
            .starts_with("credential")
            .then(|| {
                pair[0]
                    .trim_matches(|character: char| !character.is_ascii_digit())
                    .parse::<usize>()
                    .ok()
            })
            .flatten()
    });
    match credential_count {
        Some(count) if count > 0 => ProviderConnectionStatus::Connected,
        Some(_) => ProviderConnectionStatus::NeedsSignIn,
        None => ProviderConnectionStatus::Unavailable,
    }
}

fn detect_provider_connection_statuses() -> ProviderConnectionStatuses {
    let claude = crate::state::agent_chat::protocol::find_agent_cli_executable("claude")
        .map(|path| {
            provider_status_output(&path, &["auth", "status"])
                .as_ref()
                .map(claude_connection_status)
                .unwrap_or(ProviderConnectionStatus::Unavailable)
        })
        .unwrap_or(ProviderConnectionStatus::NotInstalled);
    let codex = crate::state::agent_chat::protocol::find_agent_cli_executable("codex")
        .map(|path| {
            provider_status_output(&path, &["login", "status"])
                .map(|output| {
                    if output.status.success() {
                        ProviderConnectionStatus::Connected
                    } else {
                        ProviderConnectionStatus::NeedsSignIn
                    }
                })
                .unwrap_or(ProviderConnectionStatus::Unavailable)
        })
        .unwrap_or(ProviderConnectionStatus::NotInstalled);
    let opencode = crate::state::agent_chat::protocol::find_opencode_executable()
        .map(|path| {
            provider_status_output(&path, &["auth", "list"])
                .as_ref()
                .map(opencode_connection_status)
                .unwrap_or(ProviderConnectionStatus::Unavailable)
        })
        .unwrap_or(ProviderConnectionStatus::NotInstalled);

    ProviderConnectionStatuses {
        claude,
        codex,
        opencode,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingEvent {
    Start,
    StackToggled(StackTool),
    StackContinue,
    ProviderSelected(OnboardingProviderChoice),
    ProviderContinue,
    OrientationNext,
    FirstResultNext,
    DocsSeenNext,
    Back,
    NewAgentOpened,
    AgentStarted { id: Uuid, source: AgentSource },
    DocsOpened,
    DocImplementOpened,
    TasksOpened,
    TaskImplementOpened,
    ShipOpened,
    ShipNewBranch,
    ShipAllChanges,
    ShipPushDisabled,
    ShipPreparing,
    ShipPrepared,
    ShipCommitting,
    ShipCompleted,
    ShipResultNext,
    ScriptStarted,
    StartWorking,
    Exit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Welcome,
    Stack,
    ProviderChoice,
    Map,
    NewAgent,
    FirstSend,
    WaitFirstAgent,
    FirstResult,
    DocsNav,
    DocsSeen,
    TasksNav,
    TaskImplement,
    TaskSend,
    WaitTaskAgent,
    Ship,
    ShipGenerate,
    WaitShipContent,
    ShipCommit,
    WaitShip,
    ShipResult,
    RunScript,
    PreviewLive,
    AddProject,
    Finished,
}

impl Phase {
    fn shows_connected_context_message(self) -> bool {
        matches!(self, Self::RunScript | Self::PreviewLive | Self::ShipResult)
    }

    fn key(self) -> &'static str {
        match self {
            Self::Welcome => "welcome",
            Self::Stack => "stack",
            Self::ProviderChoice => "provider_choice",
            Self::Map => "map",
            Self::NewAgent => "new_agent",
            Self::FirstSend => "first_send",
            Self::WaitFirstAgent => "wait_first_agent",
            Self::FirstResult => "first_result",
            Self::DocsNav => "docs_nav",
            Self::DocsSeen => "docs_seen",
            Self::TasksNav => "tasks_nav",
            Self::TaskImplement => "task_implement",
            Self::TaskSend => "task_send",
            Self::WaitTaskAgent => "wait_task_agent",
            Self::Ship => "ship",
            Self::ShipGenerate => "ship_generate",
            Self::WaitShipContent => "wait_ship_content",
            Self::ShipCommit => "ship_commit",
            Self::WaitShip => "wait_ship",
            Self::ShipResult => "ship_result",
            Self::RunScript => "run_script",
            Self::PreviewLive => "preview_live",
            Self::AddProject => "add_project",
            Self::Finished => "finished",
        }
    }

    fn restore(key: &str, has_active_agent: bool) -> Self {
        match key {
            "stack" => Self::Stack,
            "provider_choice" => Self::ProviderChoice,
            "map" => Self::Map,
            "new_agent" | "first_send" => Self::NewAgent,
            "wait_first_agent" if has_active_agent => Self::WaitFirstAgent,
            "first_result" => Self::FirstResult,
            "docs_nav" => Self::DocsNav,
            "docs_seen" => Self::DocsSeen,
            "tasks_nav" => Self::TasksNav,
            "task_implement" | "task_send" => Self::TaskImplement,
            "wait_task_agent" if has_active_agent => Self::WaitTaskAgent,
            "ship" | "ship_generate" | "wait_ship_content" | "ship_commit" | "wait_ship" => {
                Self::Ship
            }
            "ship_result" | "run_script" | "preview_live" => Self::RunScript,
            _ => Self::Welcome,
        }
    }

    fn target(self) -> Option<SpotlightTarget> {
        match self {
            Self::FirstResult => Some(SpotlightTarget::ChangedFiles),
            Self::ShipResult => Some(SpotlightTarget::ShipResult),
            Self::DocsNav => Some(SpotlightTarget::DocsNav),
            Self::DocsSeen => Some(SpotlightTarget::DocBody),
            Self::TasksNav => Some(SpotlightTarget::TasksNav),
            Self::NewAgent => Some(SpotlightTarget::NewAgent),
            Self::FirstSend | Self::TaskSend => Some(SpotlightTarget::ComposerSend),
            Self::TaskImplement => Some(SpotlightTarget::TaskImplement),
            Self::Ship => Some(SpotlightTarget::Ship),
            Self::ShipGenerate | Self::ShipCommit => Some(SpotlightTarget::ShipPrimary),
            Self::RunScript => Some(SpotlightTarget::RunScript),
            Self::PreviewLive => Some(SpotlightTarget::AssetBody),
            _ => None,
        }
    }

    /// The step's face. Each card leads with the glyph of the thing it is about
    /// — the rail, the Doc, the branch — so the tour reads as a sequence of
    /// places rather than twenty identical paragraphs. Drawn from the bundled
    /// Lucide font, which carries the product glyphs `IconName` doesn't.
    fn glyph(self) -> lucide_icons::Icon {
        use lucide_icons::Icon as L;
        match self {
            Self::Map => L::Compass,
            Self::NewAgent => L::Bot,
            Self::FirstSend | Self::TaskSend => L::Send,
            Self::FirstResult => L::FileDiff,
            Self::DocsNav | Self::DocsSeen => L::BookOpen,
            Self::TasksNav | Self::TaskImplement => L::ListTodo,
            Self::Ship => L::GitBranch,
            Self::ShipGenerate => L::Sparkles,
            Self::ShipCommit => L::GitCommitVertical,
            Self::ShipResult => L::GitPullRequest,
            Self::RunScript | Self::PreviewLive => L::Play,
            _ => L::Rocket,
        }
    }

    fn is_waiting(self) -> bool {
        matches!(
            self,
            Self::WaitFirstAgent | Self::WaitTaskAgent | Self::WaitShipContent | Self::WaitShip
        )
    }

    /// Which of the [`STEPS`] chapters this phase belongs to. One rule, applied
    /// evenly: a chapter owns its whole arc — the setup, the wait, and the
    /// result it produces.
    fn progress(self) -> usize {
        match self {
            Self::Welcome | Self::Stack | Self::ProviderChoice | Self::Map => 0,
            Self::NewAgent | Self::FirstSend | Self::WaitFirstAgent | Self::FirstResult => 1,
            Self::DocsNav
            | Self::DocsSeen
            | Self::TasksNav
            | Self::TaskImplement
            | Self::TaskSend
            | Self::WaitTaskAgent => 2,
            Self::Ship
            | Self::ShipGenerate
            | Self::WaitShipContent
            | Self::ShipCommit
            | Self::WaitShip
            | Self::ShipResult => 3,
            Self::RunScript | Self::PreviewLive => 4,
            Self::AddProject | Self::Finished => STEPS + 1,
        }
    }

    fn is_orientation(self) -> bool {
        matches!(self, Self::Map)
    }

    /// Does this step's answer live on the card, or out in the app? A step with
    /// a continue button is answered by pressing it; every other step is
    /// answered by doing the thing the spotlight is pointing at — and those are
    /// the ones that need a beacon.
    fn has_continue(self) -> bool {
        self.is_orientation()
            || matches!(
                self,
                Self::FirstResult | Self::DocsSeen | Self::ShipResult | Self::PreviewLive
            )
    }

    fn can_go_back(self) -> bool {
        matches!(
            self,
            Self::ProviderChoice
                | Self::Map
                | Self::DocsSeen
                | Self::TaskImplement
                | Self::TaskSend
        )
    }
}

#[derive(Clone)]
pub struct OnboardingGlobal {
    tour: Entity<OnboardingTour>,
}

impl Global for OnboardingGlobal {}

pub fn install_global(tour: Entity<OnboardingTour>, cx: &mut App) {
    cx.set_global(OnboardingGlobal { tour });
}

fn tour(cx: &App) -> Option<Entity<OnboardingTour>> {
    cx.try_global::<OnboardingGlobal>()
        .map(|global| global.tour.clone())
}

fn scoped_tour(cx: &App) -> Option<Entity<OnboardingTour>> {
    let tour = tour(cx)?;
    tour.read(cx).is_active_project(cx).then_some(tour)
}

pub fn emit(event: OnboardingEvent, cx: &mut App) {
    if let Some(tour) = tour(cx) {
        tour.update(cx, |tour, cx| tour.handle_event(event, cx));
    }
}

pub fn emit_for_project(project: ProjectId, event: OnboardingEvent, cx: &mut App) {
    if let Some(tour) = tour(cx).filter(|tour| tour.read(cx).project_id == project) {
        tour.update(cx, |tour, cx| tour.handle_event(event, cx));
    }
}

pub fn is_project(project: ProjectId, cx: &App) -> bool {
    tour(cx).is_some_and(|tour| tour.read(cx).project_id == project)
}

pub fn expects_first_agent(project: ProjectId, cx: &App) -> bool {
    tour(cx).is_some_and(|tour| {
        let tour = tour.read(cx);
        tour.project_id == project && tour.phase == Phase::NewAgent
    })
}

pub fn provider_choice(cx: &App) -> Option<OnboardingProviderChoice> {
    scoped_tour(cx).and_then(|tour| default_provider_choice(&tour.read(cx).provider_choices))
}

pub struct OnboardingAgentDefaults {
    pub provider: ide_core::AgentKind,
    pub model: ide_core::AgentModel,
    pub effort: ide_core::AgentEffort,
    pub external_model_id: Option<String>,
    pub external_model_label: Option<String>,
}

pub fn agent_defaults(project: ProjectId, cx: &App) -> Option<OnboardingAgentDefaults> {
    let tour = tour(cx)?;
    let tour = tour.read(cx);
    if tour.project_id != project {
        return None;
    }
    if matches!(
        tour.phase,
        Phase::Welcome | Phase::Stack | Phase::ProviderChoice | Phase::Map | Phase::Finished
    ) {
        return None;
    }
    match default_provider_choice(&tour.provider_choices)? {
        OnboardingProviderChoice::Claude => Some(OnboardingAgentDefaults {
            provider: ide_core::AgentKind::Claude,
            model: ide_core::AgentModel::ClaudeHaiku45,
            effort: ide_core::AgentEffort::Low,
            external_model_id: None,
            external_model_label: None,
        }),
        OnboardingProviderChoice::OpenCode => Some(OnboardingAgentDefaults {
            provider: ide_core::AgentKind::OpenCode,
            model: ide_core::AgentModel::OpenCode,
            effort: ide_core::AgentEffort::Low,
            external_model_id: Some(
                ide_core::config::DEFAULT_OPENCODE_GENERATION_MODEL_ID.to_string(),
            ),
            external_model_label: Some(
                ide_core::config::DEFAULT_OPENCODE_GENERATION_MODEL_LABEL.to_string(),
            ),
        }),
        OnboardingProviderChoice::Codex => Some(OnboardingAgentDefaults {
            provider: ide_core::AgentKind::Codex,
            model: ide_core::AgentModel::CodexGpt56Luna,
            effort: ide_core::AgentEffort::Low,
            external_model_id: None,
            external_model_label: None,
        }),
    }
}

/// The preview step docks a big action bar under the live page instead of a
/// floating card. designs.rs reserves a strip at the bottom of the preview for
/// it and shrinks the WKWebView out of that strip, so the bar — a normal gpui
/// element — isn't painted behind the native web view.
/// The composer's send is a small accent square — fine normally, but on the
/// tour's send steps people don't always read it as "press this". So we swap it
/// for a labelled primary button while the tour is waiting on that one tap.
pub fn emphasizes_send(project: ProjectId, cx: &App) -> bool {
    tour(cx).is_some_and(|tour| {
        let tour = tour.read(cx);
        if tour.project_id != project {
            return false;
        }
        matches!(tour.phase, Phase::FirstSend | Phase::TaskSend)
    })
}

/// On those same steps the prompt is the tour's, not theirs — editing it would
/// send the agent somewhere the tour can't follow. So the composer is locked,
/// and a click on it earns a friendly nudge rather than a cursor.
pub fn locks_composer(project: ProjectId, cx: &App) -> bool {
    emphasizes_send(project, cx)
}

/// True only on the very last pointer step, so the sidebar's Add Project click
/// can quietly end the tour as it opens the real add-project flow.
pub fn finishing_at_add_project(cx: &App) -> bool {
    scoped_tour(cx).is_some_and(|tour| tour.read(cx).phase == Phase::AddProject)
}

pub fn reserves_preview_action_bar(cx: &App) -> bool {
    scoped_tour(cx).is_some_and(|tour| tour.read(cx).phase == Phase::PreviewLive)
}

/// Height of that reserved strip.
pub const PREVIEW_ACTION_BAR_H: f32 = 88.0;

pub fn ship_defaults_to_new_branch(project: ProjectId, cx: &App) -> bool {
    tour(cx).is_some_and(|tour| {
        let tour = tour.read(cx);
        tour.project_id == project && tour.phase == Phase::Ship
    })
}

/// Vestigial: the tour no longer teaches Plan mode — that is Claude Code and
/// Codex behaviour, and Choro's audience arrives already knowing it. The Doc
/// agent now launches in Build like any other. Callers are no-ops and should be
/// removed.
pub fn defaults_doc_agent_to_plan(_cx: &App) -> bool {
    false
}

/// Vestigial, as [`defaults_doc_agent_to_plan`]: no phase pins Plan on any more.
pub fn locks_onboarding_plan_mode(_cx: &App) -> bool {
    false
}

pub fn ship_defaults_to_all_changes(project: ProjectId, cx: &App) -> bool {
    ship_defaults_to_new_branch(project, cx)
}

pub fn ship_uses_demo_pull_request(project: ProjectId, cx: &App) -> bool {
    ship_defaults_to_new_branch(project, cx)
}

pub fn shows_connected_context_message(project: ProjectId, cx: &App) -> bool {
    tour(cx).is_some_and(|tour| {
        let tour = tour.read(cx);
        if tour.project_id != project {
            return false;
        }
        tour.phase.shows_connected_context_message()
    })
}

/// The final run step teaches the saved project script, not Choro's separate
/// project Preview toggle. Hiding that adjacent control keeps one visible,
/// highlighted action under the instruction card.
pub fn hides_project_preview_toggle(project: ProjectId, cx: &App) -> bool {
    tour(cx).is_some_and(|tour| {
        let tour = tour.read(cx);
        tour.project_id == project && tour.phase == Phase::RunScript
    })
}

pub fn target_marker(target: SpotlightTarget, cx: &App) -> gpui::AnyElement {
    let Some(tour) = scoped_tour(cx) else {
        // Target markers are overlays and must never consume flex space. A
        // plain empty div here still participates in rows and introduces their
        // configured gap before the first real indicator.
        return div().absolute().inset_0().into_any_element();
    };
    canvas(
        move |bounds, _, cx| {
            tour.update(cx, |tour, cx| tour.register_target(target, bounds, cx));
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

// ---- tour surfaces -------------------------------------------------------
//
// The tour floats above the live app, so it speaks the app's own elevated-card
// language instead of inventing one: the `focus` plane, a visible edge, and the
// canonical soft shadow (design law 2 — elevation is border + shadow, never a
// bright lift). The accent never outlines a box; it stays in the ink (eyebrow,
// medallion glyph, the one primary action) and on the spotlight ring, which is
// the single thing on screen that must pull the eye.

/// Scrim behind a full-takeover step — nothing under it is actionable.
const SHADE_MODAL: f32 = 0.92;
/// Scrim behind a spotlight step — the app must stay readable beneath it.
const SHADE_SPOTLIGHT: f32 = 0.84;
/// An agent card on the provider step. The stack inside it measures ~114px —
/// a 38px plate, the name and status pair, and the Default slot, with 10px
/// between the three groups — so the card is sized to leave a clear 14px of air
/// above the plate and below the badge. At 118 the content filled the card to
/// within 3px and the badge read as falling out of the bottom edge.
const PROVIDER_CARD_H: f32 = 142.0;
/// The Default badge's row, held open on every card whether or not it carries
/// the badge. Without it the badge would push one card's name and status up
/// while its neighbours sat lower — the misalignment that made the first pass
/// look accidental.
const PROVIDER_BADGE_H: f32 = 18.0;
/// Rough height of the instruction card, for centring the map's copy. Only ever
/// an estimate — the card sizes to its text — so it is used for placement, never
/// for clipping.
const MAP_CARD_H: f32 = 210.0;

/// The instruction card's step glyph plate. Sized against the 11px label it now
/// sits beside, not against the title it used to tower over.
const PLATE: f32 = 22.0;
/// The tour's chapters. One segment each on the ribbon, one row each on the
/// waiting card — the single place the tour's length is decided.
const STEPS: usize = 4;

/// The chapters, named. A ribbon says *how far*; only names can say *what* — so
/// while an agent works, the sidebar card lists all six and answers the three
/// questions a tour actually has to answer: what's done, what's happening now,
/// and what's coming.
const STEP_NAMES: [&str; STEPS] = [
    "Your first agent",
    "Anything into an agent",
    "Ship it",
    "See it run",
];

// ---- the two takeover moments --------------------------------------------
//
// Welcome and finale are not instruction cards and shouldn't be dressed like
// them. They own the whole screen, they're read once, and they're the only two
// places the tour gets to make an argument rather than give an order — so they
// take the display type, a real hero CTA, and room to breathe.

const WELCOME_W: f32 = 640.0;
/// The hero CTA — taller and wider than the app's 28px control, because these
/// two screens have exactly one thing to press.
const HERO_CTA_H: f32 = 36.0;
/// Beak geometry: how far it juts from the card, and how wide its base is.
const BEAK_D: f32 = 8.0;
const BEAK_W: f32 = 16.0;
/// Keeps the beak off the card's rounded corners, and — paired with
/// [`BEAK_SAFE_SPAN`] — inside the shortest card the tour renders.
const BEAK_INSET: f32 = 24.0;
const BEAK_SAFE_SPAN: f32 = 120.0;

/// Breathing room between a revealed element and the shade around it.
const SPOTLIGHT_MARGIN: f32 = 7.0;

/// The beacon: a dot that ripples on the thing you're meant to press.
///
/// The ring says "this is highlighted"; that's an annotation, and in testing it
/// wasn't enough — people read the card, found no obvious control on it, and
/// pressed the only button they could see, which was Exit. A beacon is not an
/// annotation, it's an instruction: *press this*. It only appears on steps whose
/// answer is out in the app rather than on the card ([`Phase::has_continue`]),
/// and it loops, because it is asking for something and shouldn't stop until it
/// gets it. It rides the halo's existing frame cost — the spotlight already
/// animates, so this adds no new class of work.
const BEACON_MS: u64 = 1800;
const BEACON_DOT: f32 = 7.0;
const BEACON_REACH: f32 = 11.0;

fn beacon(at: (f32, f32), color: gpui::Hsla) -> gpui::AnyElement {
    let (bx, by) = at;
    div()
        .absolute()
        .with_animation(
            "onboarding-beacon",
            Animation::new(Duration::from_millis(BEACON_MS)).repeat(),
            move |_, delta| {
                // gpui can't scale a div, so the ripple grows by recomputing its
                // box each frame. Two of them, half a cycle apart, so there's
                // always one travelling.
                let ripple = |phase: f32| {
                    let t = (delta + phase) % 1.0;
                    let r = BEACON_DOT / 2.0 + (BEACON_REACH - BEACON_DOT / 2.0) * t;
                    div()
                        .absolute()
                        .left(px(bx - r))
                        .top(px(by - r))
                        .size(px(r * 2.0))
                        .rounded_full()
                        .bg(color.opacity(0.45 * (1.0 - t)))
                };
                div()
                    .absolute()
                    .child(ripple(0.0))
                    .child(ripple(0.5))
                    .child(
                        div()
                            .absolute()
                            .left(px(bx - BEACON_DOT / 2.0))
                            .top(px(by - BEACON_DOT / 2.0))
                            .size(px(BEACON_DOT))
                            .rounded_full()
                            .bg(color),
                    )
            },
        )
        .into_any_element()
}

/// The shade, as the complement of any number of holes.
///
/// A step can need to light several things at once that aren't neighbours — the
/// agent's header, the Git panel down the right, the scripts up top. That is the
/// "nothing scattered" claim made by showing rather than saying, and one rect
/// can't do it: the union of those swallows the whole window.
///
/// This was horizontal bands, which quietly assumed no two holes ever shared a
/// row — the Git panel runs the full height, so it overlapped everything and the
/// later holes were silently *lost* (dimmed, not revealed). So instead: cut the
/// screen on every hole edge, keep the cells no hole covers, and merge each row
/// back into runs. Handles any arrangement, overlapping or not, and emits a
/// handful of rects rather than a grid of them.
fn shade_holes(
    holes: &[Bounds<Pixels>],
    screen: gpui::Size<Pixels>,
    shade: gpui::Hsla,
) -> Vec<gpui::AnyElement> {
    let (sw, sh) = (f32::from(screen.width), f32::from(screen.height));
    let cuts: Vec<(f32, f32, f32, f32)> = holes
        .iter()
        .map(|b| {
            (
                (f32::from(b.left()) - SPOTLIGHT_MARGIN).max(0.0),
                (f32::from(b.top()) - SPOTLIGHT_MARGIN).max(0.0),
                (f32::from(b.right()) + SPOTLIGHT_MARGIN).min(sw),
                (f32::from(b.bottom()) + SPOTLIGHT_MARGIN).min(sh),
            )
        })
        .filter(|(l, t, r, b)| r > l && b > t)
        .collect();
    if cuts.is_empty() {
        return vec![div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(shade)
            .into_any_element()];
    }

    let axis = |mut v: Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        v
    };
    let xs = axis(
        std::iter::once(0.0)
            .chain(std::iter::once(sw))
            .chain(cuts.iter().flat_map(|c| [c.0, c.2]))
            .collect(),
    );
    let ys = axis(
        std::iter::once(0.0)
            .chain(std::iter::once(sh))
            .chain(cuts.iter().flat_map(|c| [c.1, c.3]))
            .collect(),
    );

    let mut out = Vec::new();
    for row in ys.windows(2) {
        let (t, b) = (row[0], row[1]);
        let (cy, h) = (0.5 * (t + b), b - t);
        if h <= 0.0 {
            continue;
        }
        // Walk the row and merge consecutive un-holed cells into one rect.
        let mut run: Option<f32> = None;
        for i in 0..xs.len() - 1 {
            let (l, r) = (xs[i], xs[i + 1]);
            let cx = 0.5 * (l + r);
            let covered = cuts
                .iter()
                .any(|(hl, ht, hr, hb)| cx > *hl && cx < *hr && cy > *ht && cy < *hb);
            match (covered, run) {
                (false, None) => run = Some(l),
                (true, Some(start)) => {
                    out.push((start, t, l - start, h));
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(start) = run {
            out.push((start, t, sw - start, h));
        }
    }
    out.into_iter()
        .filter(|(_, _, w, h)| *w > 0.5 && *h > 0.5)
        .map(|(x, y, w, h)| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .occlude()
                .bg(shade)
                .into_any_element()
        })
        .collect()
}

/// Which edge of the card the beak sits on — the edge facing the target.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Beak {
    Left,
    Right,
    Top,
    Bottom,
}

/// The beak: the bit that turns a card sitting near a thing into a card that is
/// clearly *about* that thing. gpui can only rotate `svg()`, not a `Div`, so the
/// triangle is painted by hand — filled with the card's own plane, then stroked
/// down its two outer edges to continue the card's border around the point. The
/// base deliberately overhangs into the card by a pixel so the card's own border
/// doesn't draw a line across the beak's mouth.
fn beak(side: Beak, at: f32, card_left: f32, card_top: f32, card_w: f32, cx: &App) -> Div {
    let fill = crate::ui::design::focus(cx);
    let edge = crate::ui::design::line_2(cx);
    let (w, h) = match side {
        Beak::Left | Beak::Right => (BEAK_D + 1.5, BEAK_W),
        Beak::Top | Beak::Bottom => (BEAK_W, BEAK_D + 1.5),
    };
    let (x, y) = match side {
        Beak::Left => (card_left - BEAK_D, at - BEAK_W / 2.0),
        Beak::Right => (card_left + card_w - 1.5, at - BEAK_W / 2.0),
        Beak::Top => (at - BEAK_W / 2.0, card_top - BEAK_D),
        Beak::Bottom => (at - BEAK_W / 2.0, card_top - 1.5),
    };
    div()
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(w))
        .h(px(h))
        .child(canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                let o = bounds.origin;
                let ox = f32::from(o.x);
                let oy = f32::from(o.y);
                // tip, then the two base corners that meet the card edge.
                let (tip, a, b) = match side {
                    Beak::Left => ((ox, oy + BEAK_W / 2.0), (ox + w, oy), (ox + w, oy + BEAK_W)),
                    Beak::Right => ((ox + w, oy + BEAK_W / 2.0), (ox, oy), (ox, oy + BEAK_W)),
                    Beak::Top => ((ox + BEAK_W / 2.0, oy), (ox, oy + h), (ox + BEAK_W, oy + h)),
                    Beak::Bottom => ((ox + BEAK_W / 2.0, oy + h), (ox, oy), (ox + BEAK_W, oy)),
                };
                let pt = |p: (f32, f32)| gpui::point(px(p.0), px(p.1));
                let mut face = gpui::PathBuilder::fill();
                face.move_to(pt(a));
                face.line_to(pt(tip));
                face.line_to(pt(b));
                face.close();
                if let Ok(path) = face.build() {
                    window.paint_path(path, fill);
                }
                let mut rim = gpui::PathBuilder::stroke(px(1.0));
                rim.move_to(pt(a));
                rim.line_to(pt(tip));
                rim.line_to(pt(b));
                if let Ok(path) = rim.build() {
                    window.paint_path(path, edge);
                }
            },
        ))
}
/// Breathing room between the spotlight ring and the halo that softens it.
const HALO_SPREAD: f32 = 5.0;
/// The halo's breath. One slow, shallow cycle — the only motion in the tour, so
/// it has to read as alive rather than impatient. These three are the switch:
/// set `HALO_MIN == HALO_MAX` to hold it still.
const HALO_CYCLE: Duration = Duration::from_millis(2600);
const HALO_MIN: f32 = 0.18;
const HALO_MAX: f32 = 0.5;
/// Instruction-card width. Shared by the card and the placement math that has
/// to know where it will land, so the two can never disagree. Sized by the
/// footer rather than the prose — Previous + Exit tour + the longest continue
/// label ("Meet your first agent") is the widest row the card must hold, and
/// the card clips whatever it can't fit.
const CARD_W: f32 = 400.0;

/// The dimmed backdrop a tour step sits on.
fn scrim(shade: f32, cx: &App) -> Div {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(crate::ui::design::base(cx).opacity(shade))
}

/// The tour's elevated card — one plane, edge, radius, and shadow for every
/// step, so the welcome, provider, finale, waiting, and instruction cards read
/// as the same object moving through the tour.
fn tour_card(cx: &App) -> Div {
    v_flex()
        .occlude()
        .overflow_hidden()
        .rounded(crate::ui::design::r_lg())
        .border_1()
        .border_color(crate::ui::design::line_2(cx))
        .bg(crate::ui::design::focus(cx))
        .shadow(crate::ui::design::shadow())
}

/// A card's action tray. Separated from the body by a plane step rather than a
/// rule: `base` sits below `focus` on every shipped theme, so the tray recedes
/// by the same amount in light and dark without a stroke.
fn tour_footer(cx: &App) -> Div {
    h_flex()
        .items_center()
        .gap_2()
        .bg(crate::ui::design::base(cx))
}

/// The one action on a takeover screen. Same accent recipe as every primary in
/// the app — just given the room the moment deserves.
fn hero_cta(id: &'static str, label: &'static str, cx: &App) -> gpui_component::button::Button {
    style::primary_button(id, label, cx)
        .h(px(HERO_CTA_H))
        .px_5()
        .icon(IconName::ArrowRight)
}

/// The display line that opens a takeover screen — the tour's one hero voice.
fn hero_title(text: &'static str, cx: &App) -> Div {
    div()
        .text_size(crate::ui::design::text_display())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .line_height(gpui::relative(1.18))
        .text_color(crate::ui::design::t1(cx))
        .child(text)
}

/// The uppercase brand line above a card title.
fn eyebrow(text: &'static str, cx: &App) -> Div {
    div()
        .text_size(crate::ui::design::text_label())
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::ui::design::accent(cx))
        .child(text)
}

// ---- the welcome hub ----------------------------------------------------
//
// Choro's icon is already the argument the tour is making: a C wrapping four
// strands that converge into one bright node — many threads in, one thing out,
// nothing dropped. So the hero doesn't illustrate the idea beside the brand; it
// draws the brand, and the brand says it. Around it, the project's tools stream
// in, in the same visual language as the landing page's manifesto hub.
//
// This is the mark rendered flat and line-first, not the glossy 3D of the app
// icon — the identity in the app's own language. The purples are the brand ramp
// the spinner already uses: here Choro is Choro on every theme, the way a logo
// doesn't restyle itself per palette.
//
// Everything is ONE-SHOT. gpui only re-requests frames while an animation runs
// (`if !done { request_animation_frame() }`), so once the last pulse lands the
// hero stops asking for frames entirely and the screen costs nothing to sit on.
// A looping flow would repaint at refresh rate for as long as the card is open.

const HUB_W: f32 = WELCOME_W - 64.0;
const HUB_H: f32 = 224.0;
const HUB_MID_Y: f32 = 112.0;
/// The mark is authored in its own 112-unit box, then placed into hub space.
const MARK_SCALE: f32 = 0.786;
const MARK_OX: f32 = 244.0;
const MARK_OY: f32 = 68.0;

const MARK_CX: f32 = 52.0;
const MARK_CY: f32 = 56.0;
const MARK_R: f32 = 34.0;
const MARK_STROKE: f32 = 12.0;
/// Half the C's opening, centred due east — where the node sits.
const MARK_GAP_DEG: f32 = 46.0;
const STRAND_X0: f32 = 25.0;
const STRAND_Y: [f32; 4] = [45.0, 51.0, 61.0, 67.0];
const STRAND_W: f32 = 3.2;
const MARK_NODE_X: f32 = 69.0;
const MARK_NODE_R: f32 = 5.2;
/// The mark's trailing square, sitting just outside the C's mouth.
const MARK_SQUARE_X: f32 = 81.5;
const MARK_SQUARE: f32 = 7.5;
const SAMPLES: usize = 40;

/// Tool chips: four down each side, streaming into the mark.
const CHIP_H: f32 = 26.0;
const CHIP_ROWS: [f32; 4] = [32.0, 88.0, 144.0, 196.0];
/// The inner edge each column of chips aligns to — and where its stream starts.
const CHIP_EDGE_L: f32 = 108.0;
const CHIP_EDGE_R: f32 = 470.0;
/// Where the streams land on the mark. Both are derived from the mark itself
/// rather than eyeballed beside it: the left column meets the C's outer edge,
/// the right column runs into the trailing square and tucks under it (the square
/// paints after the canvas, so the tips disappear beneath it). Hard-coding these
/// left the right-hand streams stopping ~15px short, pointing at nothing.
const LAND_L: f32 = MARK_OX + (MARK_CX - MARK_R - MARK_STROKE / 2.0) * MARK_SCALE;
const LAND_R: f32 = MARK_OX + MARK_SQUARE_X * MARK_SCALE;
/// The chips arrive alternating side to side, so the hub fills evenly instead of
/// sweeping one flank and then the other. Position in the queue, per chip.
const ARRIVAL: [usize; 8] = [0, 2, 4, 6, 1, 3, 5, 7];
/// How much of a stream the travelling pulse occupies.
const PULSE_LEN: f32 = 0.07;

/// The pulse beats, named rather than inlined so [`HUB_MS`] can be derived from
/// them instead of guessed alongside them.
const QUEUE_STEP: f32 = 100.0;
const PULSE_START: f32 = 2150.0;
const PULSE_TRAVEL: f32 = 950.0;
const REST_START: f32 = 2050.0;
const REST_FADE: f32 = 600.0;
const CHIP_START: f32 = 2000.0;
const CHIP_FADE: f32 = 440.0;

/// A held breath before anything draws. The card lands, you read the greeting,
/// and *then* the brand assembles — without it the animation is half over before
/// your eye has arrived, which is no use to the one screen that has to land.
/// gpui's `Animation` has no delay, so the whole timeline shifts inside [`at`]
/// instead and every beat below keeps its own honest millisecond. A second is
/// the whole budget: long enough for the card to land and the eye to settle,
/// short enough that an empty hero still reads as anticipation rather than as
/// something that failed to load.
const HUB_DELAY: f32 = 1000.0;

/// The whole sequence — long enough to outlast its own last beat, plus a beat of
/// air. This is load-bearing: a one-shot animation clamps at `delta = 1.0` and
/// stops asking for frames, so anything still mid-flight at that instant freezes
/// on screen *permanently*. When `HUB_MS` was 3500 the last three pulses were
/// still travelling at the cutoff and stayed stranded on their streams forever.
const HUB_MS: f32 = HUB_DELAY + PULSE_START + QUEUE_STEP * 7.0 + PULSE_TRAVEL + 100.0;
const HUB_DRAW: Duration = Duration::from_millis(HUB_MS as u64);

fn at(ms: f32) -> f32 {
    (HUB_DELAY + ms) / HUB_MS
}

/// Progress of a beat running from `start_ms` to `end_ms`, smoothstepped.
fn beat(delta: f32, start_ms: f32, end_ms: f32) -> f32 {
    let t = ((delta - at(start_ms)) / (at(end_ms) - at(start_ms))).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear form — for the pulse, which should travel at a steady clip.
fn beat_linear(delta: f32, start_ms: f32, end_ms: f32) -> f32 {
    ((delta - at(start_ms)) / (at(end_ms) - at(start_ms))).clamp(0.0, 1.0)
}

fn hub_tools() -> [(lucide_icons::Icon, &'static str); 8] {
    use lucide_icons::Icon as L;
    [
        (L::Bot, "agents"),
        (L::BookOpen, "docs"),
        (L::ListTodo, "tasks"),
        (L::Code, "code"),
        (L::GitBranch, "git"),
        (L::Play, "scripts"),
        (L::Database, "database"),
        (L::Image, "designs"),
    ]
}

/// Each tool keeps the colour it carries everywhere else in the app.
fn tool_tint(index: usize, cx: &App) -> gpui::Hsla {
    match index {
        0 | 1 => crate::ui::design::accent(cx),
        2 | 4 => crate::ui::design::amber(cx),
        3 => crate::ui::design::sky(cx),
        5 | 6 => crate::ui::design::sage(cx),
        _ => crate::ui::design::rose(cx),
    }
}

/// Mark-space point into hub space.
fn m(p: (f32, f32)) -> (f32, f32) {
    (MARK_OX + p.0 * MARK_SCALE, MARK_OY + p.1 * MARK_SCALE)
}

fn cubic(p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> Vec<(f32, f32)> {
    (0..=SAMPLES)
        .map(|i| {
            let t = i as f32 / SAMPLES as f32;
            let u = 1.0 - t;
            let axis = |a: f32, b: f32, c: f32, d: f32| {
                u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
            };
            (axis(p0.0, p1.0, p2.0, p3.0), axis(p0.1, p1.1, p2.1, p3.1))
        })
        .collect()
}

/// Every polyline in the hero, flattened once and reused for the life of the
/// process. These were being rebuilt inside the paint closure — five fresh
/// allocations per frame, for geometry that never changes.
static HUB_GEOMETRY: std::sync::LazyLock<HubGeometry> = std::sync::LazyLock::new(|| {
    let arc = (0..=SAMPLES)
        .map(|i| {
            let t = i as f32 / SAMPLES as f32;
            // Written the way a hand writes a C: from the upper-right tip,
            // anticlockwise round to the lower-right.
            let a0 = (360.0 - MARK_GAP_DEG).to_radians();
            let a1 = MARK_GAP_DEG.to_radians();
            let a = a0 + (a1 - a0) * t;
            m((MARK_CX + MARK_R * a.cos(), MARK_CY + MARK_R * a.sin()))
        })
        .collect();
    let strands = STRAND_Y.map(|y0| {
        let end = (MARK_NODE_X - MARK_NODE_R, MARK_CY);
        cubic(
            m((STRAND_X0, y0)),
            m((STRAND_X0 + 23.0, y0)),
            m((50.0, MARK_CY)),
            m(end),
        )
    });
    let streams = std::array::from_fn(|i| {
        let y = CHIP_ROWS[i % 4];
        if i < 4 {
            cubic(
                (CHIP_EDGE_L, y),
                (CHIP_EDGE_L + 72.0, y),
                (LAND_L - 56.0, HUB_MID_Y),
                (LAND_L, HUB_MID_Y),
            )
        } else {
            cubic(
                (CHIP_EDGE_R, y),
                (CHIP_EDGE_R - 72.0, y),
                (LAND_R + 56.0, HUB_MID_Y),
                (LAND_R, HUB_MID_Y),
            )
        }
    });
    HubGeometry {
        arc,
        strands,
        streams,
    }
});

struct HubGeometry {
    arc: Vec<(f32, f32)>,
    strands: [Vec<(f32, f32)>; 4],
    streams: [Vec<(f32, f32)>; 8],
}

/// Strokes the slice of `pts` between two fractions of its own arc length. gpui
/// has no `stroke-dasharray`/`dashoffset`, so both the draw-on (`0..t`) and the
/// travelling pulse (`t-len..t`) are the same walk with different bounds.
fn draw_range(
    pts: &[(f32, f32)],
    from: f32,
    to: f32,
    width: f32,
    origin: (f32, f32),
    bg: impl Into<gpui::Background>,
    window: &mut Window,
) {
    let (from, to) = (from.clamp(0.0, 1.0), to.clamp(0.0, 1.0));
    if to - from < 1e-3 {
        return;
    }
    let dist = |a: (f32, f32), b: (f32, f32)| ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    let total: f32 = pts.windows(2).map(|w| dist(w[0], w[1])).sum();
    if total <= 0.0 {
        return;
    }
    let (a, b) = (from * total, to * total);
    let mut out: Vec<(f32, f32)> = Vec::new();
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let len = dist(w[0], w[1]);
        if len <= f32::EPSILON {
            continue;
        }
        if acc + len >= a && acc <= b {
            let lerp = |t: f32| {
                (
                    w[0].0 + (w[1].0 - w[0].0) * t,
                    w[0].1 + (w[1].1 - w[0].1) * t,
                )
            };
            if out.is_empty() {
                out.push(lerp(((a - acc) / len).clamp(0.0, 1.0)));
            }
            out.push(lerp(((b - acc) / len).clamp(0.0, 1.0)));
        }
        acc += len;
    }
    if out.len() < 2 {
        return;
    }
    let (ox, oy) = origin;
    let mut pb = gpui::PathBuilder::stroke(px(width));
    pb.move_to(gpui::point(px(ox + out[0].0), px(oy + out[0].1)));
    for q in &out[1..] {
        pb.line_to(gpui::point(px(ox + q.0), px(oy + q.1)));
    }
    if let Ok(path) = pb.build() {
        window.paint_path(path, bg);
    }
}

fn paint_hub(bounds: Bounds<Pixels>, delta: f32, line: gpui::Hsla, window: &mut Window) {
    let g = &*HUB_GEOMETRY;
    let origin = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    // The C, in the spinner's own gradient — lit from the top-left, deepening away.
    let body = gpui::linear_gradient(
        135.0,
        gpui::linear_color_stop(
            gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_BRIGHT),
            0.0,
        ),
        gpui::linear_color_stop(
            gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_DEEP),
            1.0,
        ),
    );
    draw_range(
        &g.arc,
        0.0,
        beat(delta, 0.0, 1150.0),
        MARK_STROKE * MARK_SCALE,
        origin,
        body,
        window,
    );
    let strand: gpui::Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_TRACK).into();
    for (i, pts) in g.strands.iter().enumerate() {
        let start = 520.0 + i as f32 * 90.0;
        draw_range(
            pts,
            0.0,
            beat(delta, start, start + 1000.0),
            STRAND_W * MARK_SCALE,
            origin,
            strand,
            window,
        );
    }
    // The resting connection, and the one pulse that travels it. The pulse runs
    // past the end and vanishes, leaving the quiet line behind — which is why
    // nothing has to keep moving.
    let pulse: gpui::Hsla = gpui::rgb(crate::ui::design::palette::SPINNER_SWEEP_BRIGHT).into();
    for (i, pts) in g.streams.iter().enumerate() {
        let queue = ARRIVAL[i] as f32;
        let rest = beat(
            delta,
            REST_START + queue * QUEUE_STEP,
            REST_START + REST_FADE + queue * QUEUE_STEP,
        );
        if rest > 0.0 {
            draw_range(
                pts,
                0.0,
                1.0,
                1.5,
                origin,
                line.opacity(0.45 * rest),
                window,
            );
        }
        let travel = beat_linear(
            delta,
            PULSE_START + queue * QUEUE_STEP,
            PULSE_START + PULSE_TRAVEL + queue * QUEUE_STEP,
        );
        if travel > 0.0 && travel < 1.0 {
            let head = travel * (1.0 + PULSE_LEN);
            draw_range(pts, head - PULSE_LEN, head, 2.6, origin, pulse, window);
        }
    }
}

/// One tool chip, faded in on its own beat.
fn hub_chip(index: usize, cx: &App) -> Div {
    let (icon, label) = hub_tools()[index];
    let tint = tool_tint(index, cx);
    let surface = crate::ui::design::surface(cx);
    let edge = crate::ui::design::line_2(cx);
    let ink = crate::ui::design::t2(cx);
    let left = index < 4;
    let queue = ARRIVAL[index] as f32;
    let row = CHIP_ROWS[index % 4];
    // gpui has no `translate(-50%, -50%)`, and a chip's width is its text's. So
    // each column is a full-width lane that aligns its chip to the inner edge —
    // which is exactly where that chip's stream begins.
    div()
        .absolute()
        .top(px(row - CHIP_H / 2.0))
        .flex()
        .when(left, |lane| {
            lane.left(px(0.)).w(px(CHIP_EDGE_L)).justify_end()
        })
        .when(!left, |lane| {
            lane.left(px(CHIP_EDGE_R))
                .w(px(HUB_W - CHIP_EDGE_R))
                .justify_start()
        })
        .child(div().with_animation(
            ("onboarding-hub-chip", index),
            Animation::new(HUB_DRAW),
            move |_, delta| {
                let a = beat(
                    delta,
                    CHIP_START + queue * QUEUE_STEP,
                    CHIP_START + CHIP_FADE + queue * QUEUE_STEP,
                );
                h_flex()
                    .h(px(CHIP_H))
                    .items_center()
                    .gap_1p5()
                    .px_2p5()
                    .rounded(crate::ui::design::r_sm())
                    .bg(surface.opacity(a))
                    .border_1()
                    .border_color(edge.opacity(a))
                    .text_size(crate::ui::design::text_label())
                    .text_color(ink.opacity(a))
                    .child(crate::ui::design::indicator::lucide_icon(
                        icon,
                        tint.opacity(a),
                        crate::ui::design::icon_sm(),
                    ))
                    .child(label)
            },
        ))
}

/// The welcome hero: the mark draws itself, then the project streams into it.
fn welcome_hub(cx: &App) -> Div {
    let node: gpui::Hsla = gpui::rgb(crate::ui::design::palette::MARK_NODE).into();
    let line = crate::ui::design::t3(cx);
    div()
        .relative()
        .w(px(HUB_W))
        .h(px(HUB_H))
        .flex_none()
        .child(
            canvas(|_, _, _| (), |_, _, _, _| ())
                .absolute()
                .inset_0()
                .with_animation(
                    "onboarding-hub",
                    Animation::new(HUB_DRAW),
                    move |_, delta| {
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| paint_hub(bounds, delta, line, window),
                        )
                        .absolute()
                        .inset_0()
                    },
                ),
        )
        // The node, its halo, and the mark's trailing square land last. Divs
        // rather than paint: a filled circle is just a rounded box.
        .child(div().absolute().with_animation(
            "onboarding-hub-node",
            Animation::new(HUB_DRAW),
            move |_, delta| {
                let a = beat(delta, 1720.0, 2100.0);
                let (nx, ny) = m((MARK_NODE_X, MARK_CY));
                let r = MARK_NODE_R * MARK_SCALE;
                let (sx, sy) = m((MARK_SQUARE_X, MARK_CY));
                let sq = MARK_SQUARE * MARK_SCALE;
                div()
                    .absolute()
                    .child(
                        div()
                            .absolute()
                            .left(px(nx - r * 2.3))
                            .top(px(ny - r * 2.3))
                            .size(px(r * 4.6))
                            .rounded_full()
                            .bg(node.opacity(0.16 * a)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(nx - r))
                            .top(px(ny - r))
                            .size(px(r * 2.0))
                            .rounded_full()
                            .bg(node.opacity(a)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(sx - sq / 2.0))
                            .top(px(sy - sq / 2.0))
                            .size(px(sq))
                            .rounded(px(2.2 * MARK_SCALE))
                            .bg(gpui::Hsla::from(gpui::rgb(
                                crate::ui::design::palette::SPINNER_TRACK,
                            ))
                            .opacity(a)),
                    )
            },
        ))
        .children((0..8).map(|i| hub_chip(i, cx)))
}

/// A stack tool's colour — matched to the same node's colour in the hero
/// constellation, so a tool you pick here glows the same hue when it lights up
/// on the page you build.
fn stack_tool_color(tool: StackTool, cx: &App) -> gpui::Hsla {
    use crate::ui::design as d;
    match tool {
        StackTool::Database | StackTool::Editor => d::sky(cx),
        StackTool::GitHub | StackTool::Issues => d::amber(cx),
        StackTool::Docs | StackTool::Agents => d::accent(cx),
        StackTool::Design => d::rose(cx),
        StackTool::Scripts => d::sage(cx),
    }
}

/// The real brand mark where the app ships one — GitHub's octocat, Claude's
/// spark, Figma's, Linear's, and Postgres from the bundled Devicon font — and a
/// clean Lucide glyph for the categories that have no single logo (Docs, Editor,
/// Scripts). All tint to the passed colour, so the grid still reads as one set.
fn stack_tool_mark(tool: StackTool, color: gpui::Hsla, size: Pixels) -> gpui::AnyElement {
    let svg = |path: &'static str| {
        gpui_component::Icon::empty()
            .path(path)
            .size(size)
            .text_color(color)
            .into_any_element()
    };
    match tool {
        StackTool::GitHub => Icon::new(IconName::GitHub)
            .size(size)
            .text_color(color)
            .into_any_element(),
        StackTool::Agents => svg("agent-icons/claude.svg"),
        StackTool::Issues => svg("brand/jira.svg"),
        // The Postgres elephant from the Devicon databases subset — the same
        // font the DB panel renders provider marks with.
        StackTool::Database => div()
            .font_family(crate::theme::DEVICON_FONT_FAMILY)
            .text_size(size)
            .line_height(gpui::relative(1.))
            .text_color(color)
            .child('\u{eaf5}'.to_string())
            .into_any_element(),
        _ => {
            crate::ui::design::indicator::lucide_icon(tool.glyph(), color, size).into_any_element()
        }
    }
}

/// The instruction card's glyph plate — the medallion's small sibling, same
/// accent-tint language, carrying the face of the step you're on.
fn step_plate(icon: lucide_icons::Icon, cx: &App) -> Div {
    div()
        .flex_none()
        .size(px(PLATE))
        .rounded(crate::ui::design::r_sm())
        .flex()
        .items_center()
        .justify_center()
        .bg(crate::ui::design::accent(cx).opacity(0.12))
        .border_1()
        .border_color(crate::ui::design::accent(cx).opacity(0.22))
        .child(crate::ui::design::indicator::lucide_icon(
            icon,
            crate::ui::design::accent(cx),
            crate::ui::design::icon_sm(),
        ))
}

/// The plate wash behind a provider's brand mark. Claude ships a colored glyph,
/// so its plate carries the same orange; the OpenAI and OpenCode marks are
/// monochrome and inherit `t1`, so theirs is a neutral wash of the same ink
/// rather than an invented brand color.
fn provider_tint(provider: ide_core::AgentKind, cx: &App) -> gpui::Hsla {
    match provider {
        ide_core::AgentKind::Claude => crate::ui::design::palette::claude_brand(),
        ide_core::AgentKind::Codex | ide_core::AgentKind::OpenCode => crate::ui::design::t1(cx),
    }
}

pub struct OnboardingTour {
    workspace: Entity<Workspace>,
    project_id: ProjectId,
    center: Entity<CenterArea>,
    right_panel: Entity<RightPanel>,
    agent_chats: Entity<AgentChatState>,
    manifest: OnboardingManifest,
    phase: Phase,
    provider_choices: Vec<OnboardingProviderChoice>,
    provider_connection_statuses: ProviderConnectionStatuses,
    stack: Vec<StackTool>,
    targets: HashMap<SpotlightTarget, Bounds<Pixels>>,
    active_agent: Option<Uuid>,
    agent_was_active: bool,
    last_agent_status: Option<AgentChatStatus>,
}

impl OnboardingTour {
    pub fn view(
        workspace: Entity<Workspace>,
        project_id: ProjectId,
        center: Entity<CenterArea>,
        right_panel: Entity<RightPanel>,
        agent_chats: Entity<AgentChatState>,
        cx: &mut App,
    ) -> Entity<Self> {
        let manifest = manifest().clone();
        let saved = crate::onboarding::load_progress().unwrap_or_default();
        let active_agent = saved
            .active_agent
            .as_deref()
            .and_then(|id| Uuid::parse_str(id).ok());
        let phase = Phase::restore(&saved.phase, active_agent.is_some());
        let provider_choices = if !saved.providers.is_empty() {
            saved
                .providers
                .iter()
                .filter_map(|provider| OnboardingProviderChoice::from_key(provider))
                .collect()
        } else if matches!(saved.provider.as_deref(), Some("all" | "both")) {
            OnboardingProviderChoice::ALL.to_vec()
        } else {
            saved
                .provider
                .as_deref()
                .and_then(OnboardingProviderChoice::from_key)
                .into_iter()
                .collect()
        };
        let stack = saved
            .stack
            .iter()
            .filter_map(|slug| StackTool::from_slug(slug))
            .collect();
        let observed_workspace = workspace.clone();
        let tour = cx.new(|cx| {
            cx.observe(&agent_chats, |this: &mut Self, _, cx| {
                this.sync_agent_state(cx)
            })
            .detach();
            cx.observe(&observed_workspace, |this: &mut Self, _, cx| {
                this.restore_surface(cx);
                cx.notify();
            })
            .detach();
            Self {
                workspace,
                project_id,
                center,
                right_panel,
                agent_chats,
                manifest,
                phase,
                provider_choices,
                provider_connection_statuses: ProviderConnectionStatuses::default(),
                stack,
                targets: HashMap::new(),
                active_agent,
                agent_was_active: saved.agent_was_active,
                last_agent_status: None,
            }
        });
        tour.update(cx, |tour, cx| {
            tour.restore_surface(cx);
            tour.refresh_provider_connection_statuses(cx);
        });
        tour
    }

    fn refresh_provider_connection_statuses(&mut self, cx: &mut Context<Self>) {
        self.provider_connection_statuses = ProviderConnectionStatuses::default();
        cx.spawn(async move |this, cx| {
            let statuses = cx
                .background_executor()
                .spawn(async move { detect_provider_connection_statuses() })
                .await;
            this.update(cx, |this, cx| {
                this.provider_connection_statuses = statuses;
                if this.provider_choices.is_empty() {
                    this.provider_choices = OnboardingProviderChoice::ALL
                        .into_iter()
                        .filter(|provider| {
                            statuses.for_provider(*provider) == ProviderConnectionStatus::Connected
                        })
                        .collect();
                    this.apply_default_provider(cx);
                    this.persist_progress();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn is_active_project(&self, cx: &App) -> bool {
        self.workspace.read(cx).active == Some(self.project_id)
    }

    fn restore_surface(&mut self, cx: &mut Context<Self>) {
        if !self.is_active_project(cx) {
            return;
        }
        match self.phase {
            Phase::DocsSeen => self.center.update(cx, |center, cx| center.show_docs(cx)),
            Phase::TasksNav | Phase::TaskImplement => {
                self.center.update(cx, |center, cx| center.show_tasks(cx))
            }
            Phase::FirstResult
            | Phase::WaitFirstAgent
            | Phase::WaitTaskAgent
            | Phase::Ship
            | Phase::RunScript => {
                self.center.update(cx, |center, cx| center.show_agents(cx));
                self.right_panel.update(cx, |panel, cx| panel.show_git(cx));
            }
            _ => {}
        }
        self.sync_agent_state(cx);
    }

    fn persist_progress(&self) {
        if !crate::onboarding::enabled() {
            return;
        }
        let progress = OnboardingProgress {
            phase: self.phase.key().to_string(),
            provider: default_provider_choice(&self.provider_choices)
                .map(OnboardingProviderChoice::key),
            providers: self
                .provider_choices
                .iter()
                .map(|provider| provider.key())
                .collect(),
            stack: self
                .stack
                .iter()
                .map(|tool| tool.slug().to_string())
                .collect(),
            active_agent: self.active_agent.map(|id| id.to_string()),
            agent_was_active: self.agent_was_active,
        };
        if let Err(error) = crate::onboarding::save_progress(&progress) {
            eprintln!("failed to save Choro onboarding progress: {error:#}");
        }
    }

    fn apply_default_provider(&self, cx: &mut Context<Self>) {
        let Some(provider) = default_provider_choice(&self.provider_choices) else {
            return;
        };
        let generation_provider = match provider {
            OnboardingProviderChoice::Claude => ide_core::AgentKind::Claude,
            OnboardingProviderChoice::Codex => ide_core::AgentKind::Codex,
            OnboardingProviderChoice::OpenCode => ide_core::AgentKind::OpenCode,
        };
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_generation_agent(
                ide_core::config::GenerationAgent::for_provider(generation_provider),
                cx,
            );
            workspace.set_new_agent_defaults(
                ide_core::config::NewAgentDefaults::for_provider(generation_provider),
                cx,
            );
            workspace.save_now();
        });
    }

    fn register_target(
        &mut self,
        target: SpotlightTarget,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self.targets.get(&target) != Some(&bounds) {
            self.targets.insert(target, bounds);
            if self.phase.target() == Some(target) {
                cx.notify();
            }
        }
    }

    fn set_phase(&mut self, phase: Phase, cx: &mut Context<Self>) {
        if self.phase == phase {
            return;
        }
        self.phase = phase;
        self.persist_progress();
        cx.notify();
    }

    /// Persist the stack picks for the page to read. Order preserved as tapped.
    fn write_stack_file(&self) {
        let slugs: Vec<&str> = self.stack.iter().map(|t| t.slug()).collect();
        crate::onboarding::write_stack(&slugs);
    }

    fn handle_event(&mut self, event: OnboardingEvent, cx: &mut Context<Self>) {
        match event {
            OnboardingEvent::Start if self.phase == Phase::Welcome => {
                self.set_phase(Phase::Stack, cx)
            }
            OnboardingEvent::StackToggled(tool) if self.phase == Phase::Stack => {
                if let Some(i) = self.stack.iter().position(|t| *t == tool) {
                    self.stack.remove(i);
                } else {
                    self.stack.push(tool);
                }
                self.persist_progress();
                cx.notify();
            }
            OnboardingEvent::StackContinue if self.phase == Phase::Stack => {
                self.write_stack_file();
                self.set_phase(Phase::ProviderChoice, cx)
            }
            OnboardingEvent::ProviderSelected(provider) if self.phase == Phase::ProviderChoice => {
                if let Some(index) = self
                    .provider_choices
                    .iter()
                    .position(|selected| *selected == provider)
                {
                    self.provider_choices.remove(index);
                } else {
                    self.provider_choices.push(provider);
                }
                self.apply_default_provider(cx);
                self.persist_progress();
                cx.notify();
            }
            OnboardingEvent::ProviderContinue
                if self.phase == Phase::ProviderChoice && !self.provider_choices.is_empty() =>
            {
                self.set_phase(Phase::Map, cx)
            }
            OnboardingEvent::Back => match self.phase {
                Phase::Stack => self.set_phase(Phase::Welcome, cx),
                Phase::ProviderChoice => self.set_phase(Phase::Stack, cx),
                Phase::Map => self.set_phase(Phase::ProviderChoice, cx),
                Phase::DocsSeen => self.set_phase(Phase::DocsNav, cx),
                Phase::TaskImplement => self.set_phase(Phase::TasksNav, cx),
                Phase::TaskSend => {
                    self.center.update(cx, |center, cx| center.show_tasks(cx));
                    self.set_phase(Phase::TaskImplement, cx);
                }
                _ => {}
            },
            OnboardingEvent::OrientationNext if self.phase == Phase::Map => {
                self.set_phase(Phase::NewAgent, cx)
            }
            OnboardingEvent::FirstResultNext if self.phase == Phase::FirstResult => {
                self.set_phase(Phase::DocsNav, cx)
            }
            // Docs is read, not driven. Pressing Implement here parked you in a
            // composer full of brief with a send button, while the tour told you
            // not to send — the obvious action and the asked-for action pulling
            // apart, which is the same fault that had people tapping Exit. The
            // handoff gets demonstrated once, on the task, where it follows
            // through to something.
            OnboardingEvent::DocsSeenNext if self.phase == Phase::DocsSeen => {
                self.set_phase(Phase::TasksNav, cx)
            }
            OnboardingEvent::NewAgentOpened if self.phase == Phase::NewAgent => {
                self.set_phase(Phase::FirstSend, cx)
            }
            OnboardingEvent::AgentStarted { id, source } => {
                let next = match (self.phase, source) {
                    (Phase::FirstSend, AgentSource::FirstPrompt) => Some(Phase::WaitFirstAgent),
                    (Phase::TaskSend, AgentSource::Task) => Some(Phase::WaitTaskAgent),
                    _ => None,
                };
                if let Some(next) = next {
                    self.active_agent = Some(id);
                    self.agent_was_active = false;
                    self.last_agent_status = None;
                    self.set_phase(next, cx);
                    self.sync_agent_state(cx);
                }
            }
            OnboardingEvent::DocsOpened if self.phase == Phase::DocsNav => {
                self.set_phase(Phase::DocsSeen, cx)
            }
            OnboardingEvent::TasksOpened if self.phase == Phase::TasksNav => {
                self.set_phase(Phase::TaskImplement, cx)
            }
            OnboardingEvent::TaskImplementOpened if self.phase == Phase::TaskImplement => {
                self.set_phase(Phase::TaskSend, cx)
            }
            OnboardingEvent::ShipOpened if self.phase == Phase::Ship => {
                self.set_phase(Phase::ShipGenerate, cx)
            }
            OnboardingEvent::ShipPreparing if self.phase == Phase::ShipGenerate => {
                self.set_phase(Phase::WaitShipContent, cx)
            }
            OnboardingEvent::ShipPrepared if self.phase == Phase::WaitShipContent => {
                self.set_phase(Phase::ShipCommit, cx)
            }
            OnboardingEvent::ShipCommitting if self.phase == Phase::ShipCommit => {
                self.set_phase(Phase::WaitShip, cx)
            }
            OnboardingEvent::ShipCompleted
                if matches!(self.phase, Phase::ShipCommit | Phase::WaitShip) =>
            {
                self.set_phase(Phase::ShipResult, cx)
            }
            OnboardingEvent::ShipResultNext if self.phase == Phase::ShipResult => {
                self.set_phase(Phase::RunScript, cx)
            }
            // The preview needs a beat of its own. Going straight to the finale
            // put the end screen on top of the very thing the tour spent five
            // minutes building — you never got to look at it.
            OnboardingEvent::ScriptStarted if self.phase == Phase::RunScript => {
                let center = self.center.clone();
                let url = self.manifest.project.preview.url.clone();
                self.set_phase(Phase::PreviewLive, cx);
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(650))
                        .await;
                    center
                        .update(cx, |center, cx| center.show_onboarding_preview(&url, cx))
                        .ok();
                })
                .detach();
            }
            // Navigate away *before* the finale, not after. The preview is a
            // native WKWebView layered above ALL gpui content — `center_render`
            // only suppresses it for real dialogs, and the tour's overlay isn't
            // one. So the finale rendered behind it: the phase advanced, the card
            // existed, and the screen didn't change. Leaving Docs/Designs drops
            // the intent and tears the webview down, which is what makes the end
            // screen visible at all.
            // The finished page IS the end screen — the toolkit rundown lives
            // in it now, so there's nothing a tile grid would add. "Start
            // working" docks under the live page; pressing it hands the project
            // back (which tears the webview down) and ends the tour.
            // "Start working" hands the project back (closing the preview), then
            // leaves one last, quiet pointer: add your own project. No card
            // chrome, no exit — any click dismisses it, and the click that
            // matters is the one that starts real work.
            OnboardingEvent::StartWorking if self.phase == Phase::PreviewLive => {
                if let Err(error) = crate::onboarding::mark_completed() {
                    eprintln!("failed to record Choro onboarding completion: {error:#}");
                }
                self.center.update(cx, |center, cx| center.show_agents(cx));
                self.set_phase(Phase::AddProject, cx)
            }
            OnboardingEvent::Exit => {
                if let Err(error) = crate::onboarding::mark_completed() {
                    eprintln!("failed to record Choro onboarding completion: {error:#}");
                }
                // Same on the way out early — but only from the steps that are
                // actually showing the preview. Exiting from step one shouldn't
                // yank you somewhere you never asked to go.
                if self.phase == Phase::PreviewLive {
                    self.center.update(cx, |center, cx| center.show_agents(cx));
                }
                self.set_phase(Phase::Finished, cx)
            }
            _ => {}
        }
    }

    fn sync_agent_state(&mut self, cx: &mut Context<Self>) {
        let Some(agent_id) = self.active_agent else {
            return;
        };
        let Some(status) = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.status)
        else {
            return;
        };
        self.last_agent_status = Some(status);
        let became_active = !self.agent_was_active
            && matches!(
                status,
                AgentChatStatus::Running | AgentChatStatus::Cancelling
            );
        if became_active {
            self.agent_was_active = true;
            self.persist_progress();
        }

        match self.phase {
            Phase::WaitFirstAgent if self.agent_was_active && status == AgentChatStatus::Idle => {
                self.active_agent = None;
                self.set_phase(Phase::FirstResult, cx);
            }
            Phase::WaitTaskAgent if self.agent_was_active && status == AgentChatStatus::Idle => {
                self.center.update(cx, |center, cx| center.show_agents(cx));
                self.right_panel.update(cx, |panel, cx| panel.show_git(cx));
                self.active_agent = None;
                self.set_phase(Phase::Ship, cx);
            }
            _ => {}
        }
    }

    fn instruction(&self) -> (&'static str, &'static str, &'static str) {
        match self.phase {
            Phase::Map => (
                "Everything starts with a project",
                "Projects on the left, their tools on the right, the work in the middle. Every product gets its own home and its own agents — and they keep working while you’re off somewhere else.",
                "The map",
            ),
            Phase::NewAgent => (
                "Let’s put an agent to work",
                "Everything you’re about to make — agents, Docs, Tasks, code, Git — lives inside Choro Playground. Start one and watch.",
                // A verb, like every other action step. This read "New agent" —
                // a label, not an instruction — so the first thing the tour ever
                // asks of anyone was the one card that never asked.
                "Tap New Agent",
            ),
            Phase::FirstSend => (
                "The brief is already written",
                "A hero, three cards, a footer. Small on purpose — send it and watch where the work lands.",
                "Send it",
            ),
            Phase::FirstResult => (
                "You just built a page",
                "The result stays in the conversation, and Choro logged every file the agent touched — additions, deletions, the full diff. Have a look.",
                "Your first build",
            ),
            // Docs and Tasks are one idea with two doors: a source, and an
            // Implement that hands the whole thing over with its context. A Doc
            // is for when you're still thinking it through; a task is for when
            // someone already decided. Same move either way.
            Phase::DocsNav => (
                "When you need to think it through",
                "Some work needs specing first — the shape of a feature, the decisions, the bits you’re unsure about. That’s a Doc.",
                "Open Docs",
            ),
            Phase::DocsSeen => (
                "A Doc is a brief",
                "Write it once, properly. Implement then hands the whole thing to an agent — every decision, every caveat, nothing re-explained. A task from your board works exactly the same way.",
                "This is a Doc",
            ),
            Phase::TasksNav => (
                "When someone already decided",
                "The other kind of work arrives already written — you jotted it on the board, or it landed from Jira, Linear, ClickUp, or Asana. Same job to do.",
                "Open Tasks",
            ),
            Phase::TaskImplement => (
                "Same move, different door",
                "Implement again — and the task’s description, status, and source go with it. The agent stays linked to the ticket while it works.",
                "Hit Implement",
            ),
            Phase::TaskSend => (
                "See exactly what it gets",
                "Source, description, status, latest details — the whole brief is sitting in the composer. Read it, then let it fly.",
                "Send it",
            ),
            Phase::Ship => (
                "Time to make it real",
                "The work is done and going nowhere. Ship turns it into a branch, a commit, and a pull request — right here, without a terminal.",
                "Ship it",
            ),
            Phase::ShipGenerate => (
                // Not "nobody likes naming branches" — naming a branch takes
                // four seconds. The commit message and the PR description are
                // the bits people actually put off, and Choro writes those too.
                "The paperwork writes itself",
                "Choro reads the actual diff, then writes the branch, the commit, the PR title and its description — and does the Git work too. You just read it.",
                "Generate",
            ),
            Phase::ShipCommit => (
                "Your words, if you want them",
                "Branch, commit, PR title, description — all still editable. When they read right, one action commits, pushes, and opens the PR.",
                "Send the lot",
            ),
            Phase::ShipResult => (
                "Nothing gets lost",
                "The task it came from, the branch it opened, the PR it pushed, the files it changed, the script still serving it — all lit, all still attached. That’s the whole idea.",
                "All connected",
            ),
            Phase::PreviewLive => (
                "There it is — running",
                "That green control is your project, actually live. Leave, come back, it’s still there — with its terminal and every line of output.",
                "Preview is live",
            ),
            Phase::RunScript => (
                "Now watch it breathe",
                "Your project’s saved commands — previews, servers, tests, builds. Preview is ready to go. Press it and watch the thing you made come alive.",
                "Run Preview",
            ),
            _ => ("", "", ""),
        }
    }

    /// The one live line for a wait. The explanatory second line these used to
    /// carry ("the board opens when page two is up") is gone: the step list now
    /// shows what's next by name, so restating it was the same fact twice.
    fn waiting_status(&self) -> &'static str {
        match self.phase {
            Phase::WaitFirstAgent => "Making page one…",
            Phase::WaitTaskAgent => "Building the task…",
            Phase::WaitShipContent => "Reading the diff…",
            Phase::WaitShip => "Opening the PR…",
            _ => "Working…",
        }
    }

    /// The step ribbon. Three tiers rather than two: done steps hold the accent
    /// quietly, the step you are on carries it at full strength, and the rest
    /// stay neutral — so the ribbon says *where you are*, not only how far.
    fn render_progress(&self, cx: &App) -> impl IntoElement {
        let current = self.phase.progress();
        h_flex().w_full().gap_1().children((1..=STEPS).map(|index| {
            div()
                .h(px(2.))
                .flex_1()
                .rounded_full()
                .bg(match index.cmp(&current) {
                    std::cmp::Ordering::Less => crate::ui::design::accent(cx).opacity(0.4),
                    std::cmp::Ordering::Equal => crate::ui::design::accent(cx),
                    std::cmp::Ordering::Greater => crate::ui::design::line_2(cx),
                })
        }))
    }

    fn render_exit(&self, cx: &mut Context<Self>) -> impl IntoElement {
        style::ghost_button_compact("onboarding-exit", "Exit tour")
            .text_color(crate::ui::design::t4(cx))
            .on_click(cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)))
    }

    /// Exit, as an actual control. In the cards it can be a ghost — it sits in a
    /// tray beside other buttons, so its shape is obvious. Loose in the sidebar
    /// with no card around it, bare text just reads as another label; the way
    /// out of the tour has to look like the way out. `chip_dropdown_variant` is
    /// the app's neutral filled control for exactly this plane: a `control_raised`
    /// fill that lifts off the panel, no resting stroke.
    fn render_exit_control(&self, cx: &mut Context<Self>) -> impl IntoElement {
        style::secondary_button_compact("onboarding-exit-sidebar", "Exit tour")
            .custom(style::chip_dropdown_variant(cx))
            .on_click(cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)))
    }

    fn render_back(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        style::ghost_button_compact(id, label)
            .icon(IconName::ArrowLeft)
            .on_click(cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Back, cx)))
    }

    fn render_welcome(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        scrim(SHADE_MODAL, cx)
            .child(
                tour_card(cx)
                    .w(px(WELCOME_W))
                    .child(
                        v_flex()
                            .gap_5()
                            .p_8()
                            // A welcome screen should greet you. The old line
                            // ("the AI product builder workspace") was a
                            // billboard tagline; the title below already says
                            // what Choro is, so this gets to say hello.
                            .child(eyebrow("WELCOME TO CHORO", cx))
                            // The brand draws itself, then the whole project
                            // streams into it. This is a welcome screen before
                            // it's a tour.
                            .child(welcome_hub(cx))
                            .child(hero_title(
                                "Build your products. Keep the whole story.",
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.55))
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Agents write the code. Choro keeps everything around it — your projects, specs, tasks, Git, and the running app — in one calm place. You never lose the thread, and you always decide what ships."),
                            )
                            .child(
                                h_flex()
                                    .items_start()
                                    .gap_2()
                                    .text_size(crate::ui::design::text_ui())
                                    .line_height(gpui::relative(1.45))
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(
                                        Icon::new(IconName::FolderClosed)
                                            .size(crate::ui::design::icon_sm())
                                            .flex_none(),
                                    )
                                    // Wraps instead of clipping: this line ran
                                    // off the card's edge mid-word.
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .child("Your private Playground · completely separate from your Choro workspace"),
                                    ),
                            ),
                    )
                    .child(
                        tour_footer(cx)
                            .px_8()
                            .py_4()
                            .child(self.render_exit(cx))
                            .child(div().flex_1())
                            .child(
                                hero_cta("onboarding-start", "Build something", cx).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.handle_event(OnboardingEvent::Start, cx)
                                    }),
                                ),
                            ),
                    ),
            )
            .into_any_element()
    }

    // (stack tile helpers live at module scope, below this impl block)

    /// "What do you use?" — the tools they already juggle. Multi-select, and
    /// deliberately unexplained: it just asks, then the page they build pulls
    /// their picks into one place. Sits before the agent question, which stays
    /// for the later steps that need a concrete provider.
    fn render_stack(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let plane = crate::ui::design::focus(cx);
        let tile = |tool: StackTool| {
            let on = self.stack.contains(&tool);
            let tint = stack_tool_color(tool, cx);
            div()
                .id(("stack-tool", tool as usize))
                .relative()
                .flex()
                .items_center()
                .gap_3()
                .p_3()
                .rounded(crate::ui::design::r_md())
                .border_1()
                // Fill-first: a quiet raised fill when idle, an accent-tinted
                // fill + visible edge when picked. The check on the right is the
                // unmistakable "selected", so the whole tile doesn't have to shout.
                .bg(if on {
                    tint.opacity(0.11)
                } else {
                    crate::ui::design::control_on(plane, cx)
                })
                .border_color(if on {
                    tint.opacity(0.55)
                } else {
                    crate::ui::design::control_line(cx)
                })
                .cursor_pointer()
                .when(!on, |t| {
                    t.hover(|t| t.bg(crate::ui::design::control_on_hover(plane, cx)))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.handle_event(OnboardingEvent::StackToggled(tool), cx)
                }))
                .child(
                    div()
                        .flex_none()
                        .size(px(36.))
                        .rounded(crate::ui::design::r_sm())
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(tint.opacity(if on { 0.16 } else { 0.10 }))
                        .child(stack_tool_mark(tool, tint, px(18.))),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t1(cx))
                                .child(tool.label()),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .truncate()
                                .text_color(crate::ui::design::t3(cx))
                                .child(tool.examples()),
                        ),
                )
                .child(if on {
                    div()
                        .flex_none()
                        .size(px(18.))
                        .rounded_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(crate::ui::design::accent(cx))
                        .child(
                            Icon::new(IconName::Check)
                                .size(px(11.))
                                .text_color(crate::ui::design::on_accent(cx)),
                        )
                        .into_any_element()
                } else {
                    // A held slot so idle and picked tiles keep the same width.
                    div().flex_none().size(px(18.)).into_any_element()
                })
        };

        scrim(SHADE_MODAL, cx)
            .child(
                tour_card(cx)
                    .w(px(520.))
                    .child(
                        v_flex()
                            .gap_4()
                            .p_6()
                            .child(eyebrow("YOUR STACK", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("What do you use to build?"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.55))
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Tap the ones you reach for. Pick as many as you like."),
                            )
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(2)
                                    .gap_2()
                                    .children(StackTool::ALL.map(tile)),
                            ),
                    )
                    .child(
                        tour_footer(cx)
                            .px_6()
                            .py_3()
                            .child(self.render_exit(cx))
                            .child(div().flex_1())
                            .child(self.render_back("onboarding-stack-back", "Back", cx))
                            .child(
                                style::primary_button_compact(
                                    "onboarding-stack-continue",
                                    "Continue",
                                    cx,
                                )
                                .icon(IconName::ArrowRight)
                                .disabled(self.stack.is_empty())
                                .on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.handle_event(OnboardingEvent::StackContinue, cx)
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// One agent, as a brand-forward card: plate, name, status and the Default
    /// slot stacked and centred, with the tick in the corner. The card commits
    /// to being a card — an earlier pass laid the same parts out as a row inside
    /// this button, and since a gpui `Button` centres its children, the contents
    /// bunched in the middle of a full-width row with dead air either side.
    fn render_provider_card(
        &self,
        id: &'static str,
        provider: OnboardingProviderChoice,
        agent_kind: ide_core::AgentKind,
        name: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.provider_choices.contains(&provider);
        let is_default = default_provider_choice(&self.provider_choices) == Some(provider);
        let (status_label, status_color) = match self
            .provider_connection_statuses
            .for_provider(provider)
        {
            ProviderConnectionStatus::Checking => ("Checking…", crate::ui::design::sky(cx)),
            ProviderConnectionStatus::Connected => ("Connected", crate::ui::design::sage(cx)),
            ProviderConnectionStatus::NeedsSignIn => {
                ("Sign in needed", crate::ui::design::amber(cx))
            }
            ProviderConnectionStatus::NotInstalled => ("Not installed", crate::ui::design::t3(cx)),
            ProviderConnectionStatus::Unavailable => {
                ("Status unavailable", crate::ui::design::t3(cx))
            }
        };
        let tint = provider_tint(agent_kind, cx);

        style::dialog_choice_card_button(id, selected, cx)
            .h(px(PROVIDER_CARD_H))
            .child(
                div()
                    .relative()
                    .w_full()
                    .h_full()
                    .px_3()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_2p5()
                    .child(
                        // The brand mark on its own plate, seated the way the
                        // stack step seats its tool glyphs.
                        div()
                            .flex_none()
                            .size(px(38.))
                            .rounded(crate::ui::design::r_md())
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(tint.opacity(if selected { 0.16 } else { 0.10 }))
                            .text_color(crate::ui::design::t1(cx))
                            .child(
                                crate::ui::center::provider_brand_icon(agent_kind).size(px(20.)),
                            ),
                    )
                    .child(
                        v_flex()
                            .items_center()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(name),
                            )
                            .child(crate::ui::design::indicator::status(
                                status_label,
                                status_color,
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .flex_none()
                            .h(px(PROVIDER_BADGE_H))
                            .flex()
                            .items_center()
                            .children(is_default.then(|| {
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(crate::ui::design::accent_soft(cx))
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::accent(cx))
                                    .child("Default")
                            })),
                    )
                    // The tick sits in the corner rather than in the stack: it
                    // is the card's state, not another thing to read. Only the
                    // picked cards draw one — an empty ring on every card is
                    // three more circles competing with three brand glyphs.
                    .children(selected.then(|| {
                        div()
                            .absolute()
                            .top_2()
                            .right_2()
                            .size(px(16.))
                            .rounded_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(crate::ui::design::accent(cx))
                            .child(
                                Icon::new(IconName::Check)
                                    .size(px(10.))
                                    .text_color(crate::ui::design::on_accent(cx)),
                            )
                    })),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.handle_event(OnboardingEvent::ProviderSelected(provider), cx)
            }))
    }

    fn render_provider_choice(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        scrim(SHADE_MODAL, cx)
            .child(
                tour_card(cx)
                    .w(px(560.))
                    .child(
                        v_flex()
                            .gap_4()
                            .p_6()
                            .child(eyebrow("MAKE CHORO YOURS", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Which agents do you build with?"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.55))
                                    .text_color(crate::ui::design::t2(cx))
                                    // Says what the Default badge below means, so
                                    // the badge never appears unexplained.
                                    .child("Choro wraps around the agents you already trust. Pick every one you use — the first becomes your default."),
                            )
                            .child(
                                div()
                                    .grid()
                                    .grid_cols(3)
                                    .w_full()
                                    .gap_2p5()
                                    .child(self.render_provider_card(
                                        "onboarding-provider-claude",
                                        OnboardingProviderChoice::Claude,
                                        ide_core::AgentKind::Claude,
                                        "Claude Code",
                                        cx,
                                    ))
                                    .child(self.render_provider_card(
                                        "onboarding-provider-codex",
                                        OnboardingProviderChoice::Codex,
                                        ide_core::AgentKind::Codex,
                                        "Codex",
                                        cx,
                                    ))
                                    .child(self.render_provider_card(
                                        "onboarding-provider-opencode",
                                        OnboardingProviderChoice::OpenCode,
                                        ide_core::AgentKind::OpenCode,
                                        "OpenCode",
                                        cx,
                                    )),
                            ),
                    )
                    .child(
                        tour_footer(cx)
                            .px_6()
                            .py_3()
                            .child(self.render_exit(cx))
                            .child(div().flex_1())
                            .child(self.render_back(
                                "onboarding-provider-back",
                                "Back",
                                cx,
                            ))
                            .child(
                                style::primary_button_compact(
                                    "onboarding-provider-continue",
                                    "Continue",
                                    cx,
                                )
                                .icon(IconName::ArrowRight)
                                .disabled(self.provider_choices.is_empty())
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.handle_event(OnboardingEvent::ProviderContinue, cx)
                                })),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// The tour's sidebar heading, in the sidebar's own voice — `project_list`'s
    /// `.sect` recipe verbatim, so it reads as a sibling of PROJECTS.
    fn render_section_header(&self, cx: &App) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_1p5()
            .px_2p5()
            .child(
                Icon::new(IconName::ChevronDown)
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t4(cx))
                    .child(format!(
                        "YOUR TOUR · {} OF {}",
                        self.phase.progress().clamp(1, STEPS),
                        STEPS
                    )),
            )
    }

    /// The chapters with their state — done steps tick and recede, the step
    /// you're on carries the accent and the live status, the rest wait quietly.
    fn render_step_list(&self, failed: bool, status: &'static str, cx: &App) -> impl IntoElement {
        let current = self.phase.progress();
        v_flex()
            .w_full()
            .gap_1p5()
            .px_2p5()
            .children(STEP_NAMES.iter().enumerate().map(|(i, name)| {
                let step = i + 1;
                let state = step.cmp(&current);
                let done = state == std::cmp::Ordering::Less;
                let here = state == std::cmp::Ordering::Equal;
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_none()
                                    .size(crate::ui::design::icon_sm())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .when(done, |slot| {
                                        slot.child(
                                            Icon::new(IconName::Check)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(
                                                    crate::ui::design::accent(cx).opacity(0.55),
                                                ),
                                        )
                                    })
                                    .when(here && !failed, |slot| {
                                        slot.child(
                                            div()
                                                .size(px(6.))
                                                .rounded_full()
                                                .bg(crate::ui::design::accent(cx)),
                                        )
                                    })
                                    .when(here && failed, |slot| {
                                        slot.child(
                                            Icon::new(IconName::TriangleAlert)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::rose(cx)),
                                        )
                                    })
                                    .when(!done && !here, |slot| {
                                        slot.child(
                                            div()
                                                .size(px(6.))
                                                .rounded_full()
                                                .border_1()
                                                .border_color(crate::ui::design::line_2(cx)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(crate::ui::design::text_ui())
                                    .when(here, |label| {
                                        label.font_weight(gpui::FontWeight::SEMIBOLD)
                                    })
                                    .text_color(if here {
                                        crate::ui::design::t1(cx)
                                    } else if done {
                                        crate::ui::design::t3(cx)
                                    } else {
                                        crate::ui::design::t4(cx)
                                    })
                                    .child(*name),
                            ),
                    )
                    .when(here, |row| {
                        row.child(
                            div()
                                .pl(px(crate::ui::design::ICON_SM + 8.0))
                                .text_size(crate::ui::design::text_label())
                                .line_height(gpui::relative(1.4))
                                .text_color(if failed {
                                    crate::ui::design::rose(cx)
                                } else {
                                    crate::ui::design::t3(cx)
                                })
                                .child(if failed {
                                    "Needs you — review the error, then retry."
                                } else {
                                    status
                                }),
                        )
                    })
            }))
    }

    /// The final step. No scrim, no card — the finished page fills the screen,
    /// and the only chrome is a bar docked in the strip designs.rs reserved at
    /// the bottom of the preview (below the WKWebView, so gpui can paint it). The
    /// page carries its own toolkit rundown, so there is nothing a tile grid
    /// would add; the last thing anyone sees is what they just built.
    fn render_preview_finish(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(zone) = self
            .targets
            .get(&SpotlightTarget::PreviewActionZone)
            .copied()
        else {
            // The strip hasn't reported its rect yet — let the page show through
            // untouched until it does.
            return div().into_any_element();
        };
        // Everything but the live page and this bar goes dark. On its own the
        // finish step left the whole app lit — the Open / Edit / Use-in-Agent
        // header, the sidebar, the right panel, the rail — a dozen things to tap
        // instead of the one that matters. Shade it all; keep the preview and the
        // action bar. (The WKWebView paints above the shade anyway; this dims the
        // gpui chrome around it.)
        let preview = self
            .targets
            .get(&SpotlightTarget::AssetBody)
            .copied()
            .unwrap_or(zone);
        let shade = crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT);
        let margin = SPOTLIGHT_MARGIN;
        let ring_left = (f32::from(preview.left()) - margin).max(0.0);
        let ring_top = (f32::from(preview.top()) - margin).max(0.0);
        let ring_w = f32::from(preview.size.width) + margin * 2.0;
        let ring_h = f32::from(preview.size.height) + margin * 2.0;
        let bar = div()
            .absolute()
            .left(zone.left())
            .top(zone.top())
            .w(zone.size.width)
            .h(zone.size.height)
            .occlude()
            .flex()
            .items_center()
            .gap_4()
            .px_6()
            .border_t_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::focus(cx))
            .shadow(crate::ui::design::shadow())
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(eyebrow("YOU’RE READY", cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("You built and shipped a real page. It’s live above."),
                    ),
            )
            .child(self.render_exit(cx))
            .child(
                hero_cta("onboarding-start-working", "Start working", cx).on_click(cx.listener(
                    |this, _, _, cx| this.handle_event(OnboardingEvent::StartWorking, cx),
                )),
            );

        div()
            .absolute()
            .inset_0()
            .children(shade_holes(&[preview, zone], window.bounds().size, shade))
            // A quiet accent frame so the eye lands on the page, not the shade.
            .child(
                div()
                    .absolute()
                    .left(px(ring_left))
                    .top(px(ring_top))
                    .w(px(ring_w))
                    .h(px(ring_h))
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(crate::ui::design::accent(cx).opacity(0.35)),
            )
            .child(bar)
            .into_any_element()
    }

    /// The last thing the tour does: point, once, at Add Project. No card
    /// chrome, no exit button, no ribbon — the tour is over, this is just a
    /// hand on the shoulder toward the one thing that starts real work. Any
    /// click anywhere dismisses it; the click that lands on Add Project both
    /// dismisses and opens the real flow (wired at the sidebar).
    fn render_add_project(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(hole) = self.targets.get(&SpotlightTarget::AddProject).copied() else {
            // Row hasn't reported its rect yet — dismiss-on-any-click, no cutout.
            return div()
                .absolute()
                .inset_0()
                .id("onboarding-addproject-catch")
                .occlude()
                .bg(crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT))
                .on_click(
                    cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)),
                )
                .into_any_element();
        };
        let screen = window.bounds().size;
        let (sw, sh) = (f32::from(screen.width), f32::from(screen.height));
        let m = SPOTLIGHT_MARGIN;
        let l = (f32::from(hole.left()) - m).max(0.0);
        let t = (f32::from(hole.top()) - m).max(0.0);
        let r = (f32::from(hole.right()) + m).min(sw);
        let b = (f32::from(hole.bottom()) + m).min(sh);
        let shade = crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT);
        // Each shade panel is a click-catcher — clicking the dimmed app just
        // ends the tour, so nothing traps them here.
        let panel = |id: &'static str, x: f32, y: f32, w: f32, h: f32| {
            div()
                .absolute()
                .id(id)
                .left(px(x))
                .top(px(y))
                .w(px(w.max(0.0)))
                .h(px(h.max(0.0)))
                .occlude()
                .bg(shade)
                .on_click(
                    cx.listener(|this, _, _, cx| this.handle_event(OnboardingEvent::Exit, cx)),
                )
        };

        div()
            .absolute()
            .inset_0()
            .child(panel("op-top", 0.0, 0.0, sw, t))
            .child(panel("op-bottom", 0.0, b, sw, sh - b))
            .child(panel("op-left", 0.0, t, l, b - t))
            .child(panel("op-right", r, t, sw - r, b - t))
            // The row keeps a soft frame so it reads as the target, not a gap.
            .child(
                div()
                    .absolute()
                    .left(px(l))
                    .top(px(t))
                    .w(px(r - l))
                    .h(px(b - t))
                    .rounded(crate::ui::design::r_sm())
                    .border_1()
                    .border_color(crate::ui::design::accent(cx).opacity(0.55)),
            )
            .child(beacon((r, 0.5 * (t + b)), crate::ui::design::accent(cx)))
            // The pointer card, to the right of the row.
            .child(
                tour_card(cx)
                    .absolute()
                    .left(px(r + 18.0))
                    .top(px(t))
                    .w(px(280.))
                    .rounded(crate::ui::design::r_md())
                    .child(
                        v_flex()
                            .gap_2()
                            .p_4()
                            .child(eyebrow("ONE MORE THING", cx))
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_title())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child("Add your first project"),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .line_height(gpui::relative(1.5))
                                    .text_color(crate::ui::design::t2(cx))
                                    .child("Point Choro at a repo on your machine, and everything you just saw is yours for real."),
                            ),
                    ),
            )
            .into_any_element()
    }

    /// A label pinned over a region, centred in it by a lane the size of the
    /// region itself — gpui has no `translate(-50%, -50%)`, and a chip's width is
    /// its text's. `low` drops it toward the bottom, for the one region big
    /// enough that its centre is where the card goes.
    fn map_pin(
        &self,
        target: SpotlightTarget,
        label: &'static str,
        low: bool,
        cx: &App,
    ) -> Option<gpui::AnyElement> {
        let b = self.targets.get(&target).copied()?;
        Some(
            div()
                .absolute()
                .left(b.left())
                .top(b.top())
                .w(b.size.width)
                .h(b.size.height)
                .flex()
                .justify_center()
                .when(low, |lane| lane.items_end().pb_12())
                .when(!low, |lane| lane.items_center())
                .child(
                    // On a fully dimmed screen these are the only legible thing
                    // on it, so they read at body size and lift off the shade —
                    // at label size they were easy to skim straight past.
                    div()
                        .flex_none()
                        .px_3()
                        .py_1p5()
                        .rounded(crate::ui::design::r_sm())
                        .bg(crate::ui::design::focus(cx))
                        .border_1()
                        .border_color(crate::ui::design::accent(cx).opacity(0.55))
                        .shadow(crate::ui::design::shadow())
                        .text_size(crate::ui::design::text_ui())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::accent(cx))
                        .child(label),
                )
                .into_any_element(),
        )
    }

    /// The map. Three cards used to walk you past the sidebar, then the rail,
    /// then the middle — sequencing something that isn't a sequence. Left,
    /// middle, right is one shape you take in at a glance, and you can only see
    /// a layout by seeing all of it at once. So: dim everything, name each
    /// region, say it once.
    fn render_map(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let screen = window.bounds().size;
        let work = self.targets.get(&SpotlightTarget::WorkArea).copied();
        let card_left = work
            .map(|w| f32::from(w.left()) + (f32::from(w.size.width) - CARD_W) / 2.0)
            .unwrap_or((f32::from(screen.width) - CARD_W) / 2.0)
            .clamp(14.0, (f32::from(screen.width) - CARD_W - 14.0).max(14.0));
        let card_top = ((f32::from(screen.height) - MAP_CARD_H) / 2.0).max(14.0);
        scrim(SHADE_SPOTLIGHT, cx)
            .children(self.map_pin(
                SpotlightTarget::ProjectSidebar,
                "your projects + agents",
                false,
                cx,
            ))
            .children(self.map_pin(
                SpotlightTarget::ProjectTools,
                "the project’s tools",
                false,
                cx,
            ))
            .children(self.map_pin(SpotlightTarget::WorkArea, "the work happens here", true, cx))
            .child(self.render_instruction_card(px(card_left), px(card_top), CARD_W, cx))
            .into_any_element()
    }

    fn render_waiting(&self, window: &Window, cx: &mut Context<Self>) -> gpui::AnyElement {
        let (left, width, bottom) = self
            .targets
            .get(&SpotlightTarget::ProjectSidebar)
            .map(|sidebar| {
                let inset = 8.0;
                (
                    px(f32::from(sidebar.left()) + inset),
                    px((f32::from(sidebar.size.width) - inset * 2.0).max(210.0)),
                    px(
                        (f32::from(window.bounds().size.height) - f32::from(sidebar.bottom())
                            + inset)
                            .max(inset),
                    ),
                )
            })
            .unwrap_or((px(10.), px(280.), px(10.)));
        let failed = self.last_agent_status == Some(AgentChatStatus::Failed);
        // Not a card on the sidebar — a section *of* it. Nothing is being asked
        // of you while an agent works, so the tour stops presenting itself as a
        // notification and just sits in the project alongside Projects, wearing
        // the same section header the sidebar already uses.
        div()
            .absolute()
            .left(left)
            .bottom(bottom)
            .w(width)
            .occlude()
            .child(
                v_flex()
                    .gap_2()
                    // The one hairline in the tour. Everywhere else a plane step
                    // does this job, but here the tour butts straight into
                    // unrelated sidebar content with no surface of its own to
                    // separate them.
                    .child(div().h(px(1.)).mx_2().bg(crate::ui::design::line(cx)))
                    .child(self.render_section_header(cx))
                    // All six chapters: ticked where they're done, lit where you
                    // are, quiet where they're still to come. The live status
                    // hangs off the step it belongs to.
                    .child(self.render_step_list(failed, self.waiting_status(), cx))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_end()
                            .px_2p5()
                            .child(self.render_exit_control(cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_instruction_card(
        &self,
        left: Pixels,
        top: Pixels,
        width: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (title, body, action) = self.instruction();
        let has_continue = self.phase.has_continue();
        let continue_label = match self.phase {
            Phase::Map => "Meet your first agent",
            Phase::FirstResult => "Next: Docs",
            Phase::DocsSeen => "Show me with a task",
            Phase::ShipResult => "Next: Run project",
            _ => "Next",
        };
        let continue_event = match self.phase {
            Phase::FirstResult => OnboardingEvent::FirstResultNext,
            Phase::DocsSeen => OnboardingEvent::DocsSeenNext,
            Phase::ShipResult => OnboardingEvent::ShipResultNext,
            _ => OnboardingEvent::OrientationNext,
        };
        tour_card(cx)
            .absolute()
            .left(left)
            .top(top)
            .w(px(width))
            .rounded(crate::ui::design::r_md())
            .child(
                v_flex()
                    .gap_3()
                    .p_4()
                    // Always present, empty through the orientation cards: it
                    // sets the tour's length up front, and the card no longer
                    // changes shape when the count starts.
                    .child(self.render_progress(cx))
                    // The plate leads the eyebrow and nothing else indents. The
                    // card has one left edge — ribbon, label, title, body all
                    // start on it. Stacking the title beside the plate gave the
                    // card two competing edges, which is what read as crooked.
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(step_plate(self.phase.glyph(), cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_label())
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::accent(cx))
                                    .child(action.to_uppercase()),
                            ),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_title())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .line_height(gpui::relative(1.3))
                            .text_color(crate::ui::design::t1(cx))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_body())
                            .line_height(gpui::relative(1.5))
                            .text_color(crate::ui::design::t2(cx))
                            .child(body),
                    ),
            )
            .child(
                tour_footer(cx)
                    .px_4()
                    .py_2()
                    // Exit leads, quiet and left. It used to sit after the
                    // spacer — which put the most destructive control in the
                    // bottom-right primary slot on every step that has no Next,
                    // i.e. most of them. Someone in a test tapped it because it
                    // was the only button on the card. A way out should be
                    // findable, never the default.
                    .child(self.render_exit(cx))
                    .child(div().flex_1())
                    .when(self.phase.can_go_back(), |footer| {
                        footer.child(self.render_back(
                            "onboarding-instruction-previous",
                            "Previous",
                            cx,
                        ))
                    })
                    .when(has_continue, |footer| {
                        footer.child(
                            style::primary_button_compact(
                                "onboarding-guided-next",
                                continue_label,
                                cx,
                            )
                            .icon(IconName::ArrowRight)
                            .on_click(cx.listener(
                                move |this, _, _, cx| this.handle_event(continue_event, cx),
                            )),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_spotlight(
        &self,
        reveals: &[Bounds<Pixels>],
        focus: Bounds<Pixels>,
        secondary_focus: Option<Bounds<Pixels>>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let screen = window.bounds().size;
        let margin = SPOTLIGHT_MARGIN;
        let left = (f32::from(focus.left()) - margin).max(0.0);
        let top = (f32::from(focus.top()) - margin).max(0.0);
        let right = (f32::from(focus.right()) + margin).min(f32::from(screen.width));
        let bottom = (f32::from(focus.bottom()) + margin).min(f32::from(screen.height));
        let width = (right - left).max(1.0);
        let height = (bottom - top).max(1.0);
        let shade = crate::ui::design::base(cx).opacity(SHADE_SPOTLIGHT);
        let secondary_ring = secondary_focus.map(|secondary| {
            let secondary_left = (f32::from(secondary.left()) - margin).max(0.0);
            let secondary_top = (f32::from(secondary.top()) - margin).max(0.0);
            let secondary_right =
                (f32::from(secondary.right()) + margin).min(f32::from(screen.width));
            let secondary_bottom =
                (f32::from(secondary.bottom()) + margin).min(f32::from(screen.height));
            div()
                .absolute()
                .left(px(secondary_left))
                .top(px(secondary_top))
                .w(px((secondary_right - secondary_left).max(1.0)))
                .h(px((secondary_bottom - secondary_top).max(1.0)))
                .rounded(crate::ui::design::r_sm())
                .border_1()
                // Quieter than the primary ring — it is context for the action,
                // not the action itself.
                .border_color(crate::ui::design::accent(cx).opacity(0.45))
                .into_any_element()
        });

        let gap = 18.0;
        let screen_width = f32::from(screen.width);
        let screen_height = f32::from(screen.height);
        // A native Doc webview paints above GPUI overlays. While that webview is
        // open, keep the instruction card wholly inside the right sidebar so no
        // part of its copy or controls can disappear behind the document.
        let doc_sidebar_card = matches!(self.phase, Phase::DocsSeen | Phase::TasksNav)
            .then(|| {
                let tools = self.targets.get(&SpotlightTarget::ProjectTools)?;
                let rail_left = self
                    .targets
                    .get(&SpotlightTarget::TasksNav)
                    .map(|rail| f32::from(rail.left()))
                    .unwrap_or_else(|| f32::from(tools.right()));
                let card_left = f32::from(tools.left()) + 14.0;
                let card_right = rail_left - gap;
                (card_right > card_left + 220.0).then_some((card_left, card_right - card_left))
            })
            .flatten();
        let card_width = doc_sidebar_card
            .map(|(_, width)| width.min(CARD_W))
            .unwrap_or(CARD_W);
        let card_height = if doc_sidebar_card.is_some() {
            260.0
        } else {
            220.0
        };
        let prefer_left = self.phase == Phase::Ship;
        // The side is now part of the answer, not just the coordinates: the beak
        // has to sit on whichever edge of the card faces the target.
        let (card_left, card_top, beak_side) = if let Some((sidebar_left, _)) = doc_sidebar_card {
            (
                sidebar_left,
                top.min(screen_height - card_height - 14.0).max(14.0),
                if self.phase == Phase::DocsSeen {
                    Beak::Left
                } else {
                    Beak::Right
                },
            )
        } else if prefer_left && left >= card_width + gap + 14.0 {
            (
                left - card_width - gap,
                top.min(screen_height - card_height - 14.0),
                Beak::Right,
            )
        } else if right + gap + card_width <= screen_width - 14.0 {
            (
                right + gap,
                top.min(screen_height - card_height - 14.0),
                Beak::Left,
            )
        } else if left >= card_width + gap + 14.0 {
            (
                left - card_width - gap,
                top.min(screen_height - card_height - 14.0),
                Beak::Right,
            )
        } else if bottom + gap + card_height <= screen_height - 14.0 {
            (
                left.min(screen_width - card_width - 14.0).max(14.0),
                bottom + gap,
                Beak::Top,
            )
        } else {
            (
                left.min(screen_width - card_width - 14.0).max(14.0),
                (top - card_height - gap).max(14.0),
                Beak::Bottom,
            )
        };
        let beak_at = match beak_side {
            // Track the target's centre, but stay inside the card's own span.
            // The clamp is deliberately conservative: `card_height` above is an
            // estimate, and a beak that slid off a shorter card would read as a
            // stray triangle floating in the shade.
            Beak::Left | Beak::Right => {
                (0.5 * (top + bottom)).clamp(card_top + BEAK_INSET, card_top + BEAK_SAFE_SPAN)
            }
            Beak::Top | Beak::Bottom => (0.5 * (left + right))
                .clamp(card_left + BEAK_INSET, card_left + card_width - BEAK_INSET),
        };

        div()
            .absolute()
            .inset_0()
            .children(shade_holes(reveals, screen, shade))
            .children(secondary_ring)
            // A soft outer halo carries the ring's glow into the shade instead
            // of a hard drop shadow, which only muddied the revealed UI. It
            // breathes — the one moving thing on screen, sitting exactly where
            // the eye is supposed to go. Slow and shallow on purpose; a tour
            // that pulses hard reads as needy.
            .child({
                let glow = crate::ui::design::accent(cx);
                div()
                    .absolute()
                    .left(px(left - HALO_SPREAD))
                    .top(px(top - HALO_SPREAD))
                    .w(px(width + HALO_SPREAD * 2.0))
                    .h(px(height + HALO_SPREAD * 2.0))
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(glow.opacity(HALO_MIN))
                    .with_animation(
                        "onboarding-halo",
                        Animation::new(HALO_CYCLE).repeat(),
                        move |halo, delta| {
                            // A sine keeps the loop seamless — it returns to
                            // exactly where it started, so the repeat has no
                            // visible seam the way a linear ramp would.
                            let wave = 0.5 + 0.5 * (delta * std::f32::consts::TAU).sin();
                            halo.border_color(glow.opacity(HALO_MIN + (HALO_MAX - HALO_MIN) * wave))
                        },
                    )
            })
            .child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(top))
                    .w(px(width))
                    .h(px(height))
                    .rounded(crate::ui::design::r_sm())
                    .border_2()
                    .border_color(crate::ui::design::accent(cx)),
            )
            // On the target's edge that faces the card, never over its middle:
            // the eye travels card → target, so the beacon sits on that path.
            // A dot in the centre would cover the very word it's asking you to
            // press, and on a corner it lands wherever the eye isn't.
            .children((!self.phase.has_continue()).then(|| {
                let at = match beak_side {
                    Beak::Left => (right, top + height / 2.0),
                    Beak::Right => (left, top + height / 2.0),
                    Beak::Top => (left + width / 2.0, bottom),
                    Beak::Bottom => (left + width / 2.0, top),
                };
                beacon(at, crate::ui::design::accent(cx))
            }))
            .child(self.render_instruction_card(px(card_left), px(card_top), card_width, cx))
            // After the card, so its fill paints over the card's own border.
            .child(beak(
                beak_side, beak_at, card_left, card_top, card_width, cx,
            ))
            .into_any_element()
    }
}

impl Render for OnboardingTour {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.phase == Phase::Finished || !self.is_active_project(cx) {
            return div().into_any_element();
        }
        if self.phase == Phase::Welcome {
            return self.render_welcome(cx);
        }
        if self.phase == Phase::Stack {
            return self.render_stack(cx);
        }
        if self.phase == Phase::ProviderChoice {
            return self.render_provider_choice(cx);
        }
        if self.phase.is_waiting() {
            return self.render_waiting(window, cx);
        }
        // Before the `target()` lookup below: the map deliberately has no single
        // focus, so it must not fall through to the no-target fallback.
        if self.phase == Phase::Map {
            return self.render_map(window, cx);
        }
        if self.phase == Phase::AddProject {
            return self.render_add_project(window, cx);
        }
        // The live page shows through untouched; the only tour chrome is a bar
        // docked under it (designs.rs reserved the strip). No scrim, no card —
        // the page is the reward, not something to dim.
        if self.phase == Phase::PreviewLive {
            return self.render_preview_finish(window, cx);
        }
        let Some(focus) = self
            .phase
            .target()
            .and_then(|target| self.targets.get(&target).copied())
        else {
            return scrim(SHADE_SPOTLIGHT, cx).into_any_element();
        };
        let reveal_target = match self.phase {
            Phase::TaskSend => Some(SpotlightTarget::Composer),
            Phase::ShipGenerate | Phase::ShipCommit => Some(SpotlightTarget::ShipDialog),
            Phase::FirstSend | Phase::TaskImplement => Some(SpotlightTarget::WorkArea),
            _ => None,
        };
        let reveal = reveal_target
            .and_then(|target| self.targets.get(&target).copied())
            .unwrap_or(focus);
        // "Nothing gets lost" is the one claim the tour can *show* instead of
        // asserting: light every connected piece at once and let the shade make
        // the argument. The task and PR under the agent title, the ship result
        // in the timeline, the changed files in Git, the script still serving —
        // four corners of the window, one thread, all lit together.
        let mut reveals = vec![reveal];
        if self.phase == Phase::ShipResult {
            reveals = [
                self.targets.get(&SpotlightTarget::AgentContext).copied(),
                self.targets.get(&SpotlightTarget::ShipResult).copied(),
                self.targets.get(&SpotlightTarget::GitPanel).copied(),
                self.targets.get(&SpotlightTarget::RunScript).copied(),
            ]
            .into_iter()
            .flatten()
            .collect();
            if reveals.is_empty() {
                reveals.push(focus);
            }
        }
        let secondary_focus = match self.phase {
            Phase::PreviewLive => self.targets.get(&SpotlightTarget::RunScript).copied(),
            Phase::ShipResult => self.targets.get(&SpotlightTarget::AgentContext).copied(),
            _ => None,
        };
        self.render_spotlight(&reveals, focus, secondary_focus, window, cx)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::{
        claude_connection_status, default_provider_choice, opencode_connection_status,
        OnboardingProviderChoice, Phase, ProviderConnectionStatus,
    };

    fn output(success: bool, stdout: &str) -> std::process::Output {
        std::process::Output {
            status: std::process::ExitStatus::from_raw(if success { 0 } else { 1 << 8 }),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    #[test]
    fn provider_auth_output_is_classified_without_exposing_account_details() {
        assert_eq!(
            claude_connection_status(&output(
                true,
                r#"{"loggedIn": true, "email": "private@example.com"}"#,
            )),
            ProviderConnectionStatus::Connected
        );
        assert_eq!(
            claude_connection_status(&output(true, r#"{"loggedIn": false}"#)),
            ProviderConnectionStatus::NeedsSignIn
        );
        assert_eq!(
            opencode_connection_status(&output(true, "2 credentials")),
            ProviderConnectionStatus::Connected
        );
        assert_eq!(
            opencode_connection_status(&output(true, "0 credentials")),
            ProviderConnectionStatus::NeedsSignIn
        );
    }

    #[test]
    fn selected_provider_priority_is_codex_then_claude_then_opencode() {
        use OnboardingProviderChoice::{Claude, Codex, OpenCode};

        assert_eq!(default_provider_choice(&[Claude, OpenCode]), Some(Claude));
        assert_eq!(
            default_provider_choice(&[OpenCode, Codex, Claude]),
            Some(Codex)
        );
        assert_eq!(default_provider_choice(&[OpenCode]), Some(OpenCode));
        assert_eq!(default_provider_choice(&[]), None);
    }

    #[test]
    fn connected_context_message_disappears_when_the_tour_is_finished() {
        assert!(Phase::ShipResult.shows_connected_context_message());
        assert!(Phase::RunScript.shows_connected_context_message());
        assert!(Phase::PreviewLive.shows_connected_context_message());
        assert!(!Phase::Finished.shows_connected_context_message());
    }

    #[test]
    fn interrupted_composers_and_ship_dialogs_restore_to_safe_entry_points() {
        assert_eq!(Phase::restore("first_send", false), Phase::NewAgent);
        assert_eq!(Phase::restore("task_send", false), Phase::TaskImplement);
        assert_eq!(Phase::restore("ship_commit", false), Phase::Ship);
        assert_eq!(Phase::restore("preview_live", false), Phase::RunScript);
    }

    #[test]
    fn waiting_agents_restore_only_when_the_agent_id_was_saved() {
        assert_eq!(
            Phase::restore("wait_first_agent", true),
            Phase::WaitFirstAgent
        );
        assert_eq!(Phase::restore("wait_first_agent", false), Phase::Welcome);
        assert_eq!(
            Phase::restore("wait_task_agent", true),
            Phase::WaitTaskAgent
        );
        assert_eq!(Phase::restore("wait_task_agent", false), Phase::Welcome);
    }
}
