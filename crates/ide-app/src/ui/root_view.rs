mod chrome;
mod settings;

mod shutdown;

use shutdown::ShutdownState;

use gpui::{
    div, prelude::FluentBuilder, px, svg, App, AppContext, Context, DragMoveEvent, Entity,
    FocusHandle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex, Disableable, Icon, IconName, PixelsExt, Selectable, Sizable, Theme, ThemeMode,
    WindowExt,
};
use ide_core::config::ThemeMode as ConfigTheme;
use ide_core::git::BranchInfo;

use crate::actions::{
    CloseTab, NavigateBack, NavigateForward, NewAgentChat, NewTerminal, NextOpenItem, OpenCommands,
    OpenContentSearch, OpenFolder, OpenProjectSearch, OpenSettings, PreviousOpenItem, QuickAddTask,
    QuitApplication, SaveFile, StopCurrentAgent, ToggleAgentPlanMode, ToggleFocusMode,
    ToggleLeftPanel, TogglePreview, ToggleRightPanel, ToggleTerminalArea, ViewAgents, ViewCode,
    ViewDb, ViewDesign, ViewDesigns, ViewDocs, ViewFiles, ViewServices, ViewSplit, ViewTasks,
    ViewTerminal,
};
use crate::remote::dto::RemoteEvent;
use crate::state::{
    AgentActivityCache, AgentCapabilityCacheFile, AgentChatState, AgentRecords, DesignsState,
    DocAssistantState, DocsState, GitStates, PenpotState, ServicesState, TasksState,
    TerminalManager, Workspace,
};
use crate::ui::agents_panel::AgentsPanel;
use crate::ui::branch_icon::branch_icon;
use crate::ui::center::preset_bar::PresetBar;
use crate::ui::center::{CenterArea, CenterMode, ProjectActivity};
use crate::ui::command_palette::CommandPalette;
use crate::ui::content_search::ContentSearch;
use crate::ui::db::db_panel::DbPanel;
use crate::ui::designs_panel::DesignsPanel;
use crate::ui::docs_panel::DocsPanel;
use crate::ui::files::file_tree::FileTree;
use crate::ui::git::git_panel::GitPanel;
use crate::ui::logo_spinner::logo_spinner;
use crate::ui::onboarding::OnboardingTour;
use crate::ui::project_list::ProjectList;
use crate::ui::project_search::ProjectSearch;
use crate::ui::project_visuals::project_icon_element;
use crate::ui::right_panel::RightPanel;
use crate::ui::settings::SettingsView;
use crate::ui::style;

fn apply_theme(theme: ConfigTheme, window: Option<&mut Window>, cx: &mut App) {
    let mode = match theme {
        ConfigTheme::Light => ThemeMode::Light,
        _ => ThemeMode::Dark,
    };
    Theme::change(mode, window, cx);
    crate::theme::apply_ui_font(cx);
}

fn apply_configured_theme(
    theme: ConfigTheme,
    theme_name: Option<&str>,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    if theme_name
        .filter(|name| crate::theme::apply_named(name, cx))
        .is_some()
    {
        return;
    }
    // Saved theme is unknown (e.g. an old install naming a removed theme) —
    // land on the signature Choro theme rather than a bare Default.
    if crate::theme::apply_named(crate::theme::SIGNATURE_THEME, cx) {
        return;
    }
    apply_theme(theme, window, cx);
}

const LEFT_PANEL_MIN: f32 = 180.0;
const LEFT_PANEL_MAX: f32 = 420.0;
const RIGHT_PANEL_MAX: f32 = 640.0;
const TITLE_BRANCH_PICKER_LIMIT: usize = 10;

fn branch_relative_time(unix_secs: i64) -> String {
    if unix_secs <= 0 {
        return String::new();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let delta = (now - unix_secs).max(0);
    match delta {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", delta / 60),
        3600..=86_399 => format!("{}h ago", delta / 3600),
        86_400..=2_591_999 => format!("{}d ago", delta / 86_400),
        _ => format!("{}mo ago", delta / 2_592_000),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SidebarResizeSide {
    Left,
    Right,
}

#[derive(Clone)]
struct SidebarResizeHandle(SidebarResizeSide);

impl Render for SidebarResizeHandle {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct SidebarResizeState {
    side: SidebarResizeSide,
    start_x: f32,
    start_width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SidebarVisibility {
    left: bool,
    right: bool,
}

#[derive(Default)]
struct FocusModeState {
    restore: Option<SidebarVisibility>,
}

impl FocusModeState {
    fn is_active(&self) -> bool {
        self.restore.is_some()
    }

    fn enter(&mut self, show_left: &mut bool, show_right: &mut bool) {
        if self.is_active() {
            return;
        }
        self.restore = Some(SidebarVisibility {
            left: *show_left,
            right: *show_right,
        });
        *show_left = false;
        *show_right = false;
    }

    fn exit(&mut self, show_left: &mut bool, show_right: &mut bool) -> bool {
        let Some(restore) = self.restore.take() else {
            return false;
        };
        *show_left = restore.left;
        *show_right = restore.right;
        true
    }

    fn toggle(&mut self, show_left: &mut bool, show_right: &mut bool) {
        if !self.exit(show_left, show_right) {
            self.enter(show_left, show_right);
        }
    }
}

/// The main window: title bar over a 3-pane layout (projects | center | git).
pub struct RootView {
    workspace: Entity<Workspace>,
    terminals: Entity<TerminalManager>,
    tasks: Entity<TasksState>,
    docs: Entity<DocsState>,
    penpot: Entity<PenpotState>,
    git_states: Entity<GitStates>,
    agents: Entity<AgentRecords>,
    agent_chats: Entity<AgentChatState>,
    project_list: Entity<ProjectList>,
    center: Entity<CenterArea>,
    title_preset_bar: Entity<PresetBar>,
    right_panel: Entity<RightPanel>,
    root_focus: FocusHandle,
    show_left: bool,
    show_right: bool,
    focus_mode: FocusModeState,
    sidebar_resize: Option<SidebarResizeState>,
    title_branch_hovered: bool,
    title_branch_bounds: Option<gpui::Bounds<gpui::Pixels>>,
    title_branch_query: Entity<InputState>,
    title_branch_expanded: bool,
    /// When set, Settings is shown as a dedicated full-screen route over the app.
    settings_view: Option<Entity<SettingsView>>,
    remote_auth: crate::remote::RemoteAuth,
    remote_relay_identity: crate::remote::RelayIdentity,
    remote_relay_control: crate::remote::RelayControl,
    remote_connected_devices: usize,
    shutdown_state: ShutdownState,
    onboarding: Option<Entity<OnboardingTour>>,
}

impl RootView {
    pub fn view(window: &mut Window, cx: &mut App) -> Entity<Self> {
        let workspace = cx.new(|_| Workspace::load());
        let startup_capability_cwd = {
            let workspace = workspace.read(cx);
            workspace
                .active_project()
                .or_else(|| workspace.projects.first())
                .map(|project| project.path.display().to_string())
                .unwrap_or_else(|| {
                    std::env::current_dir()
                        .unwrap_or_default()
                        .display()
                        .to_string()
                })
        };
        cx.spawn(async move |cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    AgentCapabilityCacheFile::refresh_from_runtime(&startup_capability_cwd)
                })
                .await;
            if let Err(error) = result {
                eprintln!("failed to refresh agent capabilities on startup: {error}");
            }
        })
        .detach();
        let (theme, theme_name) = {
            let workspace = workspace.read(cx);
            (workspace.theme, workspace.theme_name.clone())
        };
        apply_configured_theme(theme, theme_name.as_deref(), Some(window), cx);

        let terminals = cx.new(|_| TerminalManager::new());
        let agents = cx.new(|_| AgentRecords::load());
        let agent_chats = cx.new(|_| AgentChatState::new());
        let agent_activity =
            AgentActivityCache::view(agents.clone(), agent_chats.clone(), terminals.clone(), cx);
        let doc_assistants = cx.new(|_| DocAssistantState::load());
        let git_states = cx.new(|cx| GitStates::new(workspace.clone(), cx));
        let docs = DocsState::view(workspace.clone(), cx);
        let designs = DesignsState::view(workspace.clone(), cx);
        let tasks = TasksState::view(workspace.clone(), cx);
        let services = ServicesState::view(workspace.clone(), cx);
        let penpot = PenpotState::view(cx);
        penpot.update(cx, |penpot, cx| penpot.ensure_auto_provisioned(cx));
        let center = CenterArea::view(
            workspace.clone(),
            terminals.clone(),
            agents.clone(),
            agent_chats.clone(),
            git_states.clone(),
            docs.clone(),
            designs.clone(),
            tasks.clone(),
            services.clone(),
            doc_assistants,
            penpot.clone(),
            window,
            cx,
        );
        center.update(cx, |center, cx| center.refresh_open_code_models(false, cx));
        let remote_server = crate::remote::start_remote_server();
        let remote_address = remote_server.address;
        let remote_events = remote_server.events.clone();
        let remote_presence = remote_server.presence;
        let remote_relay_states = remote_server.relay_states;
        let remote_auth = remote_server.auth.clone();
        let remote_relay_identity = remote_server.relay_identity.clone();
        let remote_relay_control = remote_server.relay_control.clone();
        let remote_commands = remote_server.commands;
        let remote_center = center.downgrade();
        cx.spawn(async move |cx| {
            while let Ok(command) = remote_commands.recv().await {
                if remote_center
                    .update(cx, |center, cx| center.handle_remote_command(command, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        eprintln!("Choro Remote host API configured at http://{remote_address}");
        let project_list = ProjectList::view(
            workspace.clone(),
            git_states.clone(),
            terminals.clone(),
            agent_chats.clone(),
            agents.clone(),
            agent_activity.clone(),
            center.downgrade(),
            cx,
        );
        let agents_panel = AgentsPanel::view(
            workspace.clone(),
            terminals.clone(),
            agent_chats.clone(),
            agents.clone(),
            agent_activity,
            center.downgrade(),
            cx,
        );
        let title_preset_bar = PresetBar::view(
            workspace.clone(),
            terminals.clone(),
            center.downgrade(),
            true,
            true,
            cx,
        );
        let git_panel = GitPanel::view(
            workspace.clone(),
            git_states.clone(),
            agents.clone(),
            center.downgrade(),
            window,
            cx,
        );
        let file_tree = FileTree::view(
            workspace.clone(),
            git_states.clone(),
            agents.clone(),
            center.clone(),
            cx,
        );
        let db_panel = DbPanel::view(workspace.clone(), center.downgrade(), cx);
        let docs_panel = DocsPanel::view(workspace.clone(), docs.clone(), center.downgrade(), cx);
        let designs_panel =
            DesignsPanel::view(workspace.clone(), designs, center.downgrade(), window, cx);
        let tasks_panel = crate::ui::tasks_panel::TasksPanel::view(
            workspace.clone(),
            tasks.clone(),
            center.downgrade(),
            cx,
        );
        let services_panel = crate::ui::services_panel::ServicesPanel::view(
            workspace.clone(),
            services,
            center.downgrade(),
            cx,
        );
        let penpot_panel = crate::ui::penpot_panel::PenpotPanel::view(
            workspace.clone(),
            penpot.clone(),
            center.downgrade(),
            cx,
        );
        let right_panel = RightPanel::view(
            git_panel,
            file_tree,
            agents_panel,
            db_panel,
            docs_panel,
            designs_panel,
            tasks_panel,
            services_panel,
            penpot_panel,
            center.clone(),
            cx,
        );
        let onboarding = if crate::onboarding::enabled() {
            let playground_root = crate::onboarding::playground_root();
            workspace
                .read(cx)
                .projects
                .iter()
                .find(|project| project.path == playground_root)
                .map(|project| project.id)
                .map(|project_id| {
                    let tour = OnboardingTour::view(
                        workspace.clone(),
                        project_id,
                        center.clone(),
                        right_panel.clone(),
                        agent_chats.clone(),
                        cx,
                    );
                    crate::ui::onboarding::install_global(tour.clone(), cx);
                    tour
                })
        } else {
            None
        };
        let root_focus = cx.focus_handle();
        root_focus.focus(window);
        let title_branch_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search branches"));

        let view = cx.new(|cx| {
            let workspace_remote_events = remote_events.clone();
            cx.observe(&workspace, move |_: &mut Self, _, cx| {
                let _ = workspace_remote_events.send(RemoteEvent::HostSnapshotChanged);
                cx.notify();
            })
            .detach();
            cx.observe(&center, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.observe(&git_states, |_: &mut Self, _, cx| cx.notify())
                .detach();
            let chat_remote_events = remote_events.clone();
            cx.observe(&agent_chats, move |this: &mut Self, _, cx| {
                let _ = chat_remote_events.send(RemoteEvent::HostSnapshotChanged);
                this.update_dock_badge(cx);
                cx.notify();
            })
            .detach();
            let agent_remote_events = remote_events.clone();
            cx.observe(&agents, move |_: &mut Self, _, cx| {
                let _ = agent_remote_events.send(RemoteEvent::HostSnapshotChanged);
                cx.notify();
            })
            .detach();
            cx.observe(&penpot, |_: &mut Self, _, cx| cx.notify())
                .detach();
            cx.subscribe(&title_branch_query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            Self {
                workspace,
                terminals,
                tasks,
                docs,
                penpot,
                git_states,
                agents,
                agent_chats,
                project_list,
                center,
                title_preset_bar,
                right_panel,
                root_focus,
                show_left: true,
                show_right: true,
                focus_mode: FocusModeState::default(),
                sidebar_resize: None,
                title_branch_hovered: false,
                title_branch_bounds: None,
                title_branch_query,
                title_branch_expanded: false,
                settings_view: None,
                remote_auth,
                remote_relay_identity,
                remote_relay_control,
                remote_connected_devices: 0,
                shutdown_state: ShutdownState::Idle,
                onboarding,
            }
        });
        view.update(cx, |this, cx| this.update_dock_badge(cx));

        let remote_presence_root = view.downgrade();
        cx.spawn(async move |cx| {
            while let Ok(connected_devices) = remote_presence.recv().await {
                if remote_presence_root
                    .update(cx, |this, cx| {
                        this.remote_connected_devices = connected_devices;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let remote_state_root = view.downgrade();
        cx.spawn(async move |cx| {
            while remote_relay_states.recv().await.is_ok() {
                if remote_state_root
                    .update(cx, |this, cx| {
                        if let Some(settings) = this.settings_view.clone() {
                            settings.update(cx, |_, cx| cx.notify());
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();

        let root = view.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            root.update(cx, |this, cx| this.handle_close_request(cx))
                .unwrap_or(true)
        });

        // Catch up on external changes (commits, checkouts) when the app regains focus.
        view.update(cx, |_, cx| {
            cx.observe_window_activation(window, |this: &mut Self, window, cx| {
                if window.is_window_active() {
                    this.git_states
                        .update(cx, |states, cx| states.refresh_all(cx));
                    this.center
                        .update(cx, |center, cx| center.refresh_open_code_models(true, cx));
                    if window.focused(cx).is_none() {
                        this.root_focus.focus(window);
                    }
                }
            })
            .detach();
        });

        // Headless debug: verify the dialog layer after the window is fully
        // set up (root view installed), without needing a click.
        if std::env::var("CHORO_DEBUG_DIALOG").is_ok()
            || std::env::var("MYIDE_DEBUG_DIALOG").is_ok()
        {
            let handle = window.window_handle();
            let workspace = view.read(cx).workspace.clone();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(2))
                    .await;
                handle
                    .update(cx, |_, window, cx| {
                        crate::ui::preset_editor::PresetEditor::open(workspace, window, cx);
                        eprintln!("debug: dialog active = {}", window.has_active_dialog(cx));
                    })
                    .ok();
            })
            .detach();
        }

        view
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (left_size, right_size) = {
            let panels = &self.workspace.read(cx).panels;
            (
                px(panels.left),
                px(panels.right.max(crate::ui::design::RIGHT_SIDEBAR_MIN_W)),
            )
        };

        // Overlay layers (dialogs, sheets, notifications) are NOT rendered by
        // gpui_component::Root automatically — the app root must include them.
        let sheet_layer = gpui_component::Root::render_sheet_layer(window, cx);
        let dialog_layer = gpui_component::Root::render_dialog_layer(window, cx);
        let notification_layer = gpui_component::Root::render_notification_layer(window, cx);
        let settings_screen = self
            .settings_view
            .clone()
            .map(|view| self.render_settings_screen(view, cx));
        let activity = self.center.read(cx).activity();
        let show_context_panel =
            self.show_right && activity != crate::ui::center::ProjectActivity::Design;

        // Project activity navigation is a single product treatment: a
        // full-height rail to the right of the Git panel. Keeping it fixed
        // makes the workspace predictable and leaves the title bar for project
        // identity, scripts, and Run.
        let focus_mode_active = self.focus_mode.is_active();
        let nav_rail_right = (!focus_mode_active).then(|| {
            self.nav_rail(true, !show_context_panel, cx)
                .into_any_element()
        });
        let scripts_center = false;
        let remote_connected = self.remote_connected_devices > 0;

        // The right-side pieces (resize handle + panel + rail). In Right-rail
        // mode they lift out of the body into a full-height column beside the
        // header (`right_column`); in every other layout they stay inside the
        // body (`body_right`).
        let is_rail_right = true;
        let right_group = h_flex()
            .relative()
            .flex_none()
            .h_full()
            .when(show_context_panel, |g| {
                g.child(self.resize_handle(SidebarResizeSide::Right, cx))
            })
            .child(
                div()
                    .relative()
                    .when(show_context_panel, |layout| {
                        layout
                            .w(right_size)
                            .h_full()
                            .flex_none()
                            .bg(crate::ui::design::nav(cx))
                            .child(
                                v_flex()
                                    .size_full()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .child(self.right_panel.clone()),
                                    )
                                    // In right-rail mode Settings lives at the bottom
                                    // of the rail; other layouts keep it here.
                                    .when(!is_rail_right, |col| {
                                        col.child(self.settings_footer(cx))
                                    }),
                            )
                    })
                    .child(crate::ui::onboarding::target_marker(
                        crate::ui::onboarding::SpotlightTarget::GitPanel,
                        cx,
                    ))
                    .when(!show_context_panel, |layout| layout.hidden()),
            )
            .children(nav_rail_right)
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::ProjectTools,
                cx,
            ))
            .into_any_element();
        let (body_right, right_column) = if is_rail_right {
            (None, Some(right_group))
        } else {
            (Some(right_group), None)
        };

        // The header spans only the center column now — the sidebar runs
        // full-height beside it. When the sidebar is open the macOS traffic
        // lights sit over the sidebar's top zone, so the header drops its
        // window-control padding; when it's collapsed the header reaches the
        // window edge, keeps that padding, and shows a reopen toggle.
        // A plain, full-width header (not gpui-component's TitleBar) so it fills
        // the center edge-to-edge and we fully control the traffic-light padding.
        // It's still a window drag region.
        let show_left = self.show_left;
        let header_bar = h_flex()
            .id("center-header")
            .w_full()
            .h(crate::ui::design::header_h())
            .flex_none()
            .items_center()
            .px(crate::ui::design::header_edge_inset_x())
            // Same fill as the middle/center screen (not the sidebars), with just
            // the hairline divider underneath.
            .bg(crate::ui::design::base(cx))
            .border_b_1()
            .border_color(style::hairline(cx))
            .window_control_area(gpui::WindowControlArea::Drag)
            .when(!show_left, |bar| bar.pl(px(76.)))
            .when(!show_left && !focus_mode_active, |bar| {
                bar.child(
                    style::header_icon_button("reopen-left-sidebar", IconName::PanelLeftOpen, cx)
                        .tooltip("Show sidebar")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_left = true;
                            cx.notify();
                        })),
                )
            })
            .child(
                h_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .pl_1()
                    .pr_2()
                    .gap_1p5()
                    .items_center()
                    .child(self.project_branch_label(cx)),
            )
            .child(
                div()
                    .id("project-script-toolbar-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_x_scroll()
                    .child(
                        h_flex()
                            .w_full()
                            .h_full()
                            .items_center()
                            .when(scripts_center, |row| row.justify_center())
                            .when(!scripts_center, |row| row.justify_end())
                            .child(
                                h_flex()
                                    .h_full()
                                    .items_center()
                                    .gap_1()
                                    .child(self.title_preset_bar.clone())
                                    .when(remote_connected, |row| {
                                        row.child(
                                            div()
                                                .id("header-remote-status")
                                                .size(px(28.))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .rounded(crate::ui::design::r_xs())
                                                .cursor_pointer()
                                                .hover(|chip| {
                                                    chip.bg(crate::ui::design::surface_2(cx)
                                                        .opacity(0.84))
                                                })
                                                .child(
                                                    Icon::empty()
                                                        .path("icons/phone.svg")
                                                        .size(px(16.))
                                                        .text_color(crate::ui::design::sage(cx)),
                                                )
                                                .tooltip(move |window, cx| {
                                                    gpui_component::tooltip::Tooltip::new(
                                                        "iPhone connected — open Remote settings",
                                                    )
                                                    .build(window, cx)
                                                })
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_remote_settings(window, cx);
                                                })),
                                        )
                                    }),
                            ),
                    ),
            )
            .when(focus_mode_active, |bar| {
                bar.child(
                    style::header_icon_button("toggle-focus-mode", IconName::WindowMaximize, cx)
                        .selected(true)
                        .tooltip("Exit Focus Mode (⌘F)")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.focus_mode
                                .toggle(&mut this.show_left, &mut this.show_right);
                            cx.notify();
                        })),
                )
            })
            .into_any_element();

        div()
            .relative()
            .size_full()
            // WKWebView is a native AppKit child and keeps first-responder
            // ownership after interaction. Any click that reaches GPUI is a
            // click back into Choro, so return keyboard ownership before the
            // target control applies its own FocusHandle.
            .capture_any_mouse_down(|event, window, _| {
                if event.button == MouseButton::Left {
                    crate::ui::center::restore_native_web_preview_focus(window);
                }
            })
            .child(
                v_flex()
                    .size_full()
                    .bg(crate::ui::design::base(cx))
                    .track_focus(&self.root_focus)
                    .on_action(cx.listener(|this, _: &NewTerminal, window, cx| {
                        this.center
                            .update(cx, |center, cx| center.spawn_shell(window, cx));
                    }))
                    .on_action(cx.listener(|this, _: &CloseTab, window, cx| {
                        this.center
                            .update(cx, |center, cx| center.close_selected(window, cx));
                    }))
                    .on_action(cx.listener(|this, _: &SaveFile, window, cx| {
                        this.center
                            .update(cx, |center, cx| center.save_active(window, cx));
                    }))
                    .on_action(cx.listener(|this, _: &QuitApplication, _, cx| {
                        this.handle_close_request(cx);
                    }))
                    .on_action(cx.listener(|this, _: &OpenFolder, _, cx| {
                        this.workspace
                            .update(cx, |workspace, cx| workspace.open_folder_dialog(cx));
                    }))
                    .on_action(cx.listener(|this, _: &OpenProjectSearch, window, cx| {
                        ProjectSearch::open(
                            this.workspace.clone(),
                            this.center.clone(),
                            this.agents.clone(),
                            this.docs.clone(),
                            this.tasks.clone(),
                            window,
                            cx,
                        );
                    }))
                    .on_action(cx.listener(|this, _: &OpenCommands, window, cx| {
                        CommandPalette::open(this.root_focus.clone(), window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &OpenContentSearch, window, cx| {
                        ContentSearch::open(this.workspace.clone(), this.center.clone(), window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &QuickAddTask, window, cx| {
                        crate::ui::quick_task::QuickTaskModal::open(
                            this.workspace.clone(),
                            this.tasks.clone(),
                            window,
                            cx,
                        );
                    }))
                    .on_action(cx.listener(|this, _: &ToggleLeftPanel, _, cx| {
                        this.focus_mode
                            .exit(&mut this.show_left, &mut this.show_right);
                        this.show_left = !this.show_left;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &ToggleRightPanel, _, cx| {
                        this.focus_mode
                            .exit(&mut this.show_left, &mut this.show_right);
                        this.show_right = !this.show_right;
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &ToggleFocusMode, _, cx| {
                        this.focus_mode
                            .toggle(&mut this.show_left, &mut this.show_right);
                        cx.notify();
                    }))
                    .on_action(cx.listener(|this, _: &NavigateBack, _, cx| {
                        this.center.update(cx, |center, cx| center.go_back(cx));
                    }))
                    .on_action(cx.listener(|this, _: &NavigateForward, _, cx| {
                        this.center.update(cx, |center, cx| center.go_forward(cx));
                    }))
                    .on_action(cx.listener(|this, _: &TogglePreview, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.toggle_project_preview(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ToggleTerminalArea, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.toggle_terminal_area(cx));
                    }))
                    .on_action(cx.listener(|this, _: &NextOpenItem, window, cx| {
                        this.center
                            .update(cx, |center, cx| center.cycle_open_item(1, window, cx));
                    }))
                    .on_action(cx.listener(|this, _: &PreviousOpenItem, window, cx| {
                        this.center
                            .update(cx, |center, cx| center.cycle_open_item(-1, window, cx));
                    }))
                    .on_action(cx.listener(|this, _: &StopCurrentAgent, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.stop_selected_agent(cx));
                    }))
                    .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                        this.toggle_settings(window, cx);
                    }))
                    .on_action(cx.listener(|this, _: &ViewCode, _, cx| {
                        this.center.update(cx, |center, cx| center.show_code(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewSplit, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.set_view_mode(CenterMode::Split, cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewFiles, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.set_view_mode(CenterMode::Files, cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewTerminal, _, cx| {
                        this.center.update(cx, |center, cx| {
                            center.set_view_mode(CenterMode::Terminal, cx)
                        });
                    }))
                    .on_action(cx.listener(|this, _: &ViewAgents, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.show_agents(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewTasks, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.show_tasks(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewDb, _, cx| {
                        this.center.update(cx, |center, cx| center.show_db(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewDocs, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.show_docs(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewDesigns, _, cx| {
                        this.center.update(cx, |center, cx| {
                            center.set_context_mode(crate::ui::center::ContextMode::Designs, cx)
                        });
                    }))
                    .on_action(cx.listener(|this, _: &ViewDesign, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.show_design(cx));
                    }))
                    .on_action(cx.listener(|this, _: &ViewServices, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.show_services(cx));
                    }))
                    .on_action(cx.listener(|this, _: &NewAgentChat, window, cx| {
                        this.right_panel
                            .update(cx, |panel, cx| panel.open_new_agent(window, cx));
                    }))
                    .on_action(cx.listener(|this, _: &ToggleAgentPlanMode, _, cx| {
                        this.center
                            .update(cx, |center, cx| center.toggle_selected_agent_plan_mode(cx));
                    }))
                    .child(
                        h_flex()
                            .size_full()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .h_full()
                                    .child(
                        h_flex()
                            .flex_1()
                            .min_h(px(0.))
                            .w_full()
                            .child(
                                div()
                                    .relative()
                                    .when(self.show_left, |layout| {
                                        layout
                                            .w(left_size)
                                            .h_full()
                                            .flex_none()
                                            .bg(crate::ui::design::nav(cx))
                                            .child(
                                                v_flex()
                                                    .size_full()
                                                    // Top zone: clears the macOS traffic lights
                                                    // (window top-left) and carries the center's
                                                    // back/forward, since the header no longer sits
                                                    // above the sidebar. Toggle the sidebar with the
                                                    // keyboard shortcut.
                                                    .child(
                                                        h_flex()
                                                            .h(px(36.))
                                                            .w_full()
                                                            .px_2()
                                                            .items_center()
                                                            .justify_end()
                                                            .child(self.nav_history_buttons(cx)),
                                                    )
                                                    .child({
                                                        let right_panel = self.right_panel.clone();
                                                        let section_workspace =
                                                            self.workspace.clone();
                                                        let project_workspace =
                                                            self.workspace.clone();
                                                        let center_for_my_tasks =
                                                            self.center.clone();
                                                        v_flex()
                                                            .w_full()
                                                            .px_2()
                                                            .pt_2()
                                                            .pb_1()
                                                            .gap_1()
                                                            .child(
                                                                h_flex()
                                                                    .w_full()
                                                                    .gap_1()
                                                                    .items_center()
                                                                    .child(
                                                                        h_flex()
                                                                            .id(
                                                                                "left-sidebar-new-agent",
                                                                            )
                                                                            .flex_1()
                                                                            .min_w(px(0.))
                                                                            .relative()
                                                                            .h(px(32.))
                                                                            .px_3()
                                                                            .gap_2()
                                                                            .items_center()
                                                                            .rounded(crate::ui::design::r_sm())
                                                                            .cursor_pointer()
                                                                            .hover(|row| {
                                                                                row.bg(
                                                                                    crate::ui::design::surface(cx),
                                                                                )
                                                                            })
                                                                            .tooltip(|window, cx| {
                                                                                gpui_component::tooltip::Tooltip::new(
                                                                                    "New agent",
                                                                                )
                                                                                .build(window, cx)
                                                                            })
                                                                            .child(
                                                                                Icon::new(IconName::Bot)
                                                                                    .size(crate::ui::design::icon())
                                                                                    .text_color(
                                                                                        crate::ui::design::t3(cx),
                                                                                    ),
                                                                            )
                                                                            .child(
                                                                                div()
                                                                                    .min_w(px(0.))
                                                                                    .text_size(crate::ui::design::text_body())
                                                                                    .font_weight(
                                                                                        gpui::FontWeight::NORMAL,
                                                                                    )
                                                                                    .text_color(
                                                                                        crate::ui::design::t2(cx),
                                                                                    )
                                                                                    .truncate()
                                                                                    .child("New Agent"),
                                                                            )
                                                                            .child(
                                                                                crate::ui::onboarding::target_marker(
                                                                                    crate::ui::onboarding::SpotlightTarget::NewAgent,
                                                                                    cx,
                                                                                ),
                                                                            )
                                                                            .on_click(
                                                                                move |_, window, cx| {
                                                                                    right_panel.update(
                                                                                        cx,
                                                                                        |panel, cx| {
                                                                                            panel
                                                                                                .open_new_agent(
                                                                                                    window, cx,
                                                                                                );
                                                                                        },
                                                                                    );
                                                                                },
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        Button::new(
                                                                            "left-sidebar-add-section",
                                                                        )
                                                                        .ghost()
                                                                        .xsmall()
                                                                        .h(crate::ui::design::control_h())
                                                                        .child(
                                                                            svg()
                                                                                .path(
                                                                                    "icons/add-row.svg",
                                                                                )
                                                                                .size(px(15.))
                                                                                .text_color(
                                                                                    crate::ui::design::t3(cx),
                                                                                ),
                                                                        )
                                                                        .tooltip("Add section")
                                                                        .on_click(
                                                                            move |_, window, cx| {
                                                                                ProjectList::open_section_name_dialog(
                                                                                    section_workspace.clone(),
                                                                                    None,
                                                                                    "".into(),
                                                                                    window,
                                                                                    cx,
                                                                                );
                                                                            },
                                                                        ),
                                                                    )
                                                            )
                                                            .child(
                                                                h_flex()
                                                                    .id("left-sidebar-my-tasks")
                                                                    .w_full()
                                                                    .h(px(32.))
                                                                    .px_3()
                                                                    .gap_2()
                                                                    .items_center()
                                                                    .rounded(crate::ui::design::r_sm())
                                                                    .cursor_pointer()
                                                                    .hover(|row| {
                                                                        row.bg(
                                                                            crate::ui::design::surface(cx),
                                                                        )
                                                                    })
                                                                    .tooltip(|window, cx| {
                                                                        gpui_component::tooltip::Tooltip::new(
                                                                            "My tasks across all projects",
                                                                        )
                                                                        .build(window, cx)
                                                                    })
                                                                    .child(
                                                                        Icon::new(IconName::CircleCheck)
                                                                            .size(crate::ui::design::icon())
                                                                            .text_color(
                                                                                crate::ui::design::t3(cx),
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .min_w(px(0.))
                                                                            .text_size(crate::ui::design::text_body())
                                                                            .font_weight(
                                                                                gpui::FontWeight::NORMAL,
                                                                            )
                                                                            .text_color(
                                                                                crate::ui::design::t2(cx),
                                                                            )
                                                                            .truncate()
                                                                            .child("My Tasks"),
                                                                    )
                                                                    .on_click(move |_, _, cx| {
                                                                        center_for_my_tasks.update(
                                                                            cx,
                                                                            |center, cx| {
                                                                                center.show_my_tasks(cx)
                                                                            },
                                                                        );
                                                                    }),
                                                            )
                                                            .child(
                                                                h_flex()
                                                                    .id("left-sidebar-add-project")
                                                                    .w_full()
                                                                    .h(px(32.))
                                                                    .px_3()
                                                                    .gap_2()
                                                                    .items_center()
                                                                    .rounded(crate::ui::design::r_sm())
                                                                    .cursor_pointer()
                                                                    .hover(|row| {
                                                                        row.bg(
                                                                            crate::ui::design::surface(cx),
                                                                        )
                                                                    })
                                                                    .tooltip(|window, cx| {
                                                                        gpui_component::tooltip::Tooltip::new(
                                                                            "Add project",
                                                                        )
                                                                        .build(window, cx)
                                                                    })
                                                                .child(
                                                                    Icon::new(IconName::FolderOpen)
                                                                        .size(crate::ui::design::icon())
                                                                        .text_color(
                                                                            crate::ui::design::t3(cx),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .min_w(px(0.))
                                                                        .text_size(crate::ui::design::text_body())
                                                                        .font_weight(
                                                                            gpui::FontWeight::NORMAL,
                                                                        )
                                                                        .text_color(
                                                                            crate::ui::design::t2(cx),
                                                                        )
                                                                        .truncate()
                                                                        .child("Add Project"),
                                                                )
                                                                // The tour's final pointer targets this row; the
                                                                // marker reports its bounds so it can be framed.
                                                                .child(crate::ui::onboarding::target_marker(
                                                                    crate::ui::onboarding::SpotlightTarget::AddProject,
                                                                    cx,
                                                                ))
                                                                .on_click(move |_, _, cx| {
                                                                    // Clicking Add Project is how the tour ends —
                                                                    // one click both dismisses the pointer and opens
                                                                    // the real add-project flow.
                                                                    if crate::ui::onboarding::finishing_at_add_project(cx) {
                                                                        crate::ui::onboarding::emit(
                                                                            crate::ui::onboarding::OnboardingEvent::Exit,
                                                                            cx,
                                                                        );
                                                                    }
                                                                    project_workspace.update(cx, |workspace, cx| {
                                                                        workspace.open_folder_dialog(cx)
                                                                    });
                                                                }),
                                                            )
                                                    })
                                                    .child(
                                                        div()
                                                            .flex_1()
                                                            .min_h(px(0.))
                                                            .child(self.project_list.clone()),
                                                    ),
                                            )
                                    })
                                    .child(crate::ui::onboarding::target_marker(
                                        crate::ui::onboarding::SpotlightTarget::ProjectSidebar,
                                        cx,
                                    ))
                                    .when(!self.show_left, |layout| layout.hidden()),
                            )
                            .when(self.show_left, |layout| {
                                layout.child(self.resize_handle(SidebarResizeSide::Left, cx))
                            })
                            .child(
                                v_flex()
                                    .relative()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .h_full()
                                    .child(header_bar)
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .w_full()
                                            .child(self.center.clone()),
                                    )
                                    .child(crate::ui::onboarding::target_marker(
                                        crate::ui::onboarding::SpotlightTarget::WorkArea,
                                        cx,
                                    )),
                            )
                            .children(body_right)
                            )
                            )
                            .children(right_column)
                    ),
            )
            .when_some(self.render_title_branch_overlay(cx), |root, overlay| {
                root.child(overlay)
            })
            .when_some(settings_screen, |root, screen| root.child(screen))
            .children(sheet_layer)
            .children(dialog_layer)
            .children(notification_layer)
            .when_some(self.onboarding.clone(), |root, onboarding| {
                root.child(onboarding)
            })
            .when_some(self.render_shutdown_overlay(cx), |root, overlay| {
                root.child(overlay)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_mode_hides_both_sidebars_and_restores_the_exact_layout() {
        let mut focus = FocusModeState::default();
        let mut show_left = false;
        let mut show_right = true;

        focus.toggle(&mut show_left, &mut show_right);
        assert!(focus.is_active());
        assert!(!show_left);
        assert!(!show_right);

        focus.toggle(&mut show_left, &mut show_right);
        assert!(!focus.is_active());
        assert!(!show_left);
        assert!(show_right);
    }

    #[test]
    fn exiting_inactive_focus_mode_does_not_change_sidebars() {
        let mut focus = FocusModeState::default();
        let mut show_left = true;
        let mut show_right = false;

        assert!(!focus.exit(&mut show_left, &mut show_right));
        assert!(show_left);
        assert!(!show_right);
    }
}
