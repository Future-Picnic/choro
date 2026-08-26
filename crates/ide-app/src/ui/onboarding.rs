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
use ide_core::config::default_pinned_project_activities;
use ide_core::{ProjectActivityId, ProjectId};
use uuid::Uuid;

use crate::onboarding::{manifest, OnboardingManifest, OnboardingProgress};
use crate::state::agent_chat::AgentChatStatus;
use crate::state::{AgentChatState, Workspace};
use crate::ui::center::CenterArea;
use crate::ui::right_panel::RightPanel;
use crate::ui::style;

mod provider;
mod view;
mod visuals;

pub use provider::OnboardingProviderChoice;
use provider::{
    default_provider_choice, detect_provider_connection_statuses, ProviderConnectionStatus,
    ProviderConnectionStatuses,
};
pub use visuals::target_marker;
use visuals::*;

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

    /// Turn the onboarding stack choices into the activity defaults inherited
    /// by this playground and every project the user adds afterward.
    fn apply_stack_activity_defaults(&self, cx: &mut Context<Self>) {
        let mut activities = default_pinned_project_activities();
        if self.stack.contains(&StackTool::Database) {
            activities.push(ProjectActivityId::Db);
        }
        if self.stack.contains(&StackTool::Design) {
            // Onboarding's broad "Design" choice covers both the connected
            // Penpot workspace and the project's saved visual references.
            activities.push(ProjectActivityId::Design);
            activities.push(ProjectActivityId::Assets);
        }
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_default_project_activities(activities.clone(), cx);
            workspace.save_now();
        });
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
                self.apply_stack_activity_defaults(cx);
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
}

#[cfg(test)]
mod tests {
    use std::os::unix::process::ExitStatusExt;

    use super::provider::{claude_connection_status, opencode_connection_status};
    use super::{
        default_provider_choice, OnboardingProviderChoice, Phase, ProviderConnectionStatus,
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
