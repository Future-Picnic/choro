use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gpui::{
    div, prelude::FluentBuilder, px, uniform_list, App, AppContext, Context, Entity, FontWeight,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, WeakEntity, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu as _, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex, Icon, IconName, Sizable, WindowExt,
};
use ide_core::{agents, AgentRecord, AgentRuntimeKind, AgentStatus, ProjectId, ProjectSectionId};
use uuid::Uuid;

use crate::state::agent_chat::AgentChatStatus;
use crate::state::agents::AgentRecordsEvent;
use crate::state::{
    AgentActivityCache, AgentChatState, AgentRecords, GitStates, TerminalManager, Workspace,
};
use crate::ui::center::CenterArea;
use crate::ui::logo_spinner::logo_spinner;
use crate::ui::project_visuals::{
    project_color_options, project_icon, project_icon_color, project_icon_element,
    project_icon_glyph, project_icon_options, POPULAR_PROJECT_ICONS,
};
use crate::ui::style;

/// Everything a sidebar row displays for one project.
#[derive(Clone)]
struct RowInfo {
    ix: usize,
    id: ProjectId,
    name: SharedString,
    path: SharedString,
    icon: String,
    icon_color: String,
    section_id: Option<ProjectSectionId>,
    is_favorite: bool,
    is_active: bool,
    /// Files with uncommitted changes.
    changes: usize,
    /// Names of script presets currently running.
    scripts: Vec<SharedString>,
    /// Any live agent terminal actively writing for this project.
    agents_working: bool,
    /// Agents waiting for user attention.
    agents_waiting: usize,
    /// App-owned agents currently in the manual In progress lane.
    in_progress_agents: Vec<AgentRecord>,
}

#[derive(Clone, Copy)]
enum SidebarSectionKind {
    Favorites,
    Projects,
    Custom(ProjectSectionId),
}

struct SidebarSection {
    ix: usize,
    label: SharedString,
    kind: SidebarSectionKind,
    collapsed: bool,
    rows: Vec<RowInfo>,
}

#[derive(Clone)]
struct DragProjectSection {
    id: ProjectSectionId,
    label: SharedString,
}

impl Render for DragProjectSection {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .px_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.45))
            .bg(crate::ui::design::focus(cx))
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t1(cx))
            .child(Icon::new(IconName::Menu).size(crate::ui::design::icon_sm()))
            .child(self.label.clone())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectAgentRuntime {
    NotStarted,
    Working,
    Waiting,
    Open,
    Idle,
    Ended,
}

fn compact_relative_time(updated_at: SystemTime) -> SharedString {
    let secs = SystemTime::now()
        .duration_since(updated_at)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60).into(),
        3600..=86_399 => format!("{}h", secs / 3600).into(),
        86_400..=2_591_999 => format!("{}d", secs / 86_400).into(),
        _ => format!("{}mo", secs / 2_592_000).into(),
    }
}

struct ProjectIconDialog {
    workspace: Entity<Workspace>,
    project: ProjectId,
    project_name: SharedString,
    selected_icon: String,
    selected_color: String,
    search_input: Entity<InputState>,
}

impl ProjectIconDialog {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let icon = self.selected_icon.clone();
        let color = self.selected_color.clone();
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_project_icon(self.project, icon, cx);
            workspace.set_project_icon_color(self.project, color, cx);
        });
        window.close_dialog(cx);
    }
}

impl Render for ProjectIconDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview_color = project_icon_color(&self.selected_color, cx);
        let query = self.search_input.read(cx).value().trim().to_lowercase();
        let all_icons = project_icon_options();
        let matches_query = |id: &str| query.split_whitespace().all(|term| id.contains(term));
        let matching_count = if query.is_empty() {
            POPULAR_PROJECT_ICONS.len()
        } else {
            all_icons
                .iter()
                .filter(|option| matches_query(&option.id))
                .count()
        };
        const MAX_VISIBLE_RESULTS: usize = 240;
        let visible_icons = if query.is_empty() {
            POPULAR_PROJECT_ICONS
                .iter()
                .filter_map(|id| all_icons.iter().find(|option| option.id == *id))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            all_icons
                .iter()
                .filter(|option| matches_query(&option.id))
                .take(MAX_VISIBLE_RESULTS)
                .cloned()
                .collect::<Vec<_>>()
        };
        let section_label = if query.is_empty() {
            format!("POPULAR · {} ICONS AVAILABLE", all_icons.len())
        } else {
            format!("{matching_count} RESULTS")
        };
        const ICONS_PER_ROW: usize = 6;
        const ICON_ROW_HEIGHT: f32 = 50.;
        const MAX_VISIBLE_ROWS: usize = 5;
        let result_rows = visible_icons.len().div_ceil(ICONS_PER_ROW);
        let result_height = px((result_rows.min(MAX_VISIBLE_ROWS) as f32) * ICON_ROW_HEIGHT);
        let visible_icons = Arc::new(visible_icons);
        let selected_icon = self.selected_icon.clone();
        let dialog = cx.entity().downgrade();
        let result_list_key = query.bytes().fold(0_u64, |hash, byte| {
            hash.wrapping_mul(31).wrapping_add(u64::from(byte))
        });
        let result_list_id = ("project-icon-results", result_list_key);

        v_flex()
            .w_full()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .items_center()
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.45))
                    .bg(crate::ui::design::surface_2(cx).opacity(0.28))
                    .px_3()
                    .py_2()
                    .child(
                        div()
                            .size(px(48.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(project_icon_element(
                                &self.selected_icon,
                                &self.selected_color,
                                px(48.),
                                px(24.),
                                cx,
                            )),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_body())
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(self.project_name.clone()),
                    ),
            )
            .child(Input::new(&self.search_input).cleanable(true))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child(section_label),
                    )
                    .when(result_rows == 0, |section| {
                        section.child(
                            div()
                                .h(px(42.))
                                .flex()
                                .items_center()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No icons found"),
                        )
                    })
                    .when(result_rows > 0, |section| {
                        section.child(
                            uniform_list(result_list_id, result_rows, {
                                let visible_icons = visible_icons.clone();
                                let selected_icon = selected_icon.clone();
                                let dialog = dialog.clone();
                                move |visible_rows, _, cx| {
                                    visible_rows
                                        .map(|row_ix| {
                                            let start = row_ix * ICONS_PER_ROW;
                                            let end =
                                                (start + ICONS_PER_ROW).min(visible_icons.len());
                                            h_flex()
                                                .h(px(ICON_ROW_HEIGHT))
                                                .gap_1p5()
                                                .items_start()
                                                .children(
                                                    visible_icons[start..end]
                                                        .iter()
                                                        .enumerate()
                                                        .map(|(column_ix, option)| {
                                                            let ix = start + column_ix;
                                                            let selected =
                                                                selected_icon == option.id;
                                                            let icon_id = option.id.clone();
                                                            let label = SharedString::from(
                                                                option.label.clone(),
                                                            );
                                                            let dialog = dialog.clone();
                                                            div()
                                                                .id(("project-icon-option", ix))
                                                                .w(px(54.))
                                                                .h(px(42.))
                                                                .rounded(crate::ui::design::r_sm())
                                                                .border_1()
                                                                .border_color(if selected {
                                                                    crate::ui::design::accent(cx)
                                                                        .opacity(0.75)
                                                                } else {
                                                                    crate::ui::design::line(cx)
                                                                        .opacity(0.35)
                                                                })
                                                                .bg(if selected {
                                                                    crate::ui::design::accent(cx)
                                                                        .opacity(0.13)
                                                                } else {
                                                                    gpui::transparent_black()
                                                                })
                                                                .flex()
                                                                .items_center()
                                                                .justify_center()
                                                                .cursor_pointer()
                                                                .hover(|tile| {
                                                                    tile.bg(
                                                                    crate::ui::design::surface_2(
                                                                        cx,
                                                                    )
                                                                    .opacity(0.42),
                                                                )
                                                                })
                                                                .tooltip(move |window, cx| {
                                                                    Tooltip::new(label.clone())
                                                                        .build(window, cx)
                                                                })
                                                                .child(project_icon_glyph(
                                                                    option.icon,
                                                                    preview_color,
                                                                    crate::ui::design::icon(),
                                                                ))
                                                                .on_click(move |_, _, cx| {
                                                                    dialog
                                                                        .update(cx, |this, cx| {
                                                                            this.selected_icon =
                                                                                icon_id.clone();
                                                                            cx.notify();
                                                                        })
                                                                        .ok();
                                                                })
                                                        }),
                                                )
                                        })
                                        .collect::<Vec<_>>()
                                }
                            })
                            .w_full()
                            .h(result_height),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t3(cx))
                            .child("COLOR"),
                    )
                    .child(
                        h_flex().gap_2().flex_wrap().children(
                            project_color_options()
                                .into_iter()
                                .enumerate()
                                .map(|(ix, option)| {
                                    let selected = self.selected_color == option.id;
                                    let color_id = option.id.to_string();
                                    let swatch =
                                        option.color.unwrap_or_else(|| crate::ui::design::t3(cx));
                                    div()
                                        .id(("project-color-option", ix))
                                        .size(px(28.))
                                        .rounded_full()
                                        .border_1()
                                        .border_color(if selected {
                                            crate::ui::design::t1(cx)
                                        } else {
                                            crate::ui::design::line(cx).opacity(0.45)
                                        })
                                        .bg(swatch)
                                        .tooltip(move |window, cx| {
                                            Tooltip::new(option.label).build(window, cx)
                                        })
                                        .cursor_pointer()
                                        .hover(|tile| tile.opacity(0.82))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.selected_color = color_id.clone();
                                            cx.notify();
                                        }))
                                }),
                        ),
                    ),
            )
    }
}

/// Left panel: the list of managed projects with live status indicators.
pub struct ProjectList {
    workspace: Entity<Workspace>,
    git_states: Entity<GitStates>,
    terminals: Entity<TerminalManager>,
    agent_chats: Entity<AgentChatState>,
    agents: Entity<AgentRecords>,
    agent_activity: Entity<AgentActivityCache>,
    center: WeakEntity<CenterArea>,
    hovered_project: Option<ProjectId>,
    hovered_section: Option<ProjectSectionId>,
    /// Row whose actions menu is open. The trigger only renders while the row is
    /// hovered, and moving the pointer into the menu ends that hover — so the
    /// open menu keeps its trigger mounted until it is dismissed.
    menu_project: Option<ProjectId>,
    menu_section: Option<ProjectSectionId>,
    expanded_agent_lists: HashSet<ProjectId>,
    /// Agent opened from the attention section; kept visible there until another
    /// agent is opened, so the row doesn't vanish under the click.
    attention_pinned: Option<Uuid>,
    hovered_attention: Option<Uuid>,
    /// Waiting agent ids seen on the last refresh; `None` until the first scan.
    /// A newly waiting agent auto-expands a collapsed attention section, but the
    /// baseline scan at startup respects the persisted collapse.
    known_waiting: Option<HashSet<Uuid>>,
}

impl ProjectList {
    fn save_project_name(
        workspace: Entity<Workspace>,
        id: ProjectId,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let name = input.read(cx).value().trim().to_string();
        workspace.update(cx, |workspace, cx| {
            workspace.rename_project(id, name, cx);
        });
        window.close_dialog(cx);
    }

    fn save_section_name(
        workspace: Entity<Workspace>,
        id: Option<ProjectSectionId>,
        input: Entity<InputState>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let name = input.read(cx).value().trim().to_string();
        workspace.update(cx, |workspace, cx| match id {
            Some(id) => workspace.rename_section(id, name, cx),
            None => {
                workspace.add_section(name, cx);
            }
        });
        window.close_dialog(cx);
    }

    pub(crate) fn open_section_name_dialog(
        workspace: Entity<Workspace>,
        id: Option<ProjectSectionId>,
        current_name: SharedString,
        window: &mut Window,
        cx: &mut App,
    ) {
        let title = if id.is_some() {
            "Rename Section"
        } else {
            "Add Section"
        };
        let action_label = if id.is_some() { "Rename" } else { "Add" };
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Section name")
                .default_value(current_name.to_string())
        });
        input.update(cx, |input, cx| input.focus(window, cx));

        window.open_dialog(cx, move |dialog, _, _| {
            let save_input = input.clone();
            let save_workspace = workspace.clone();
            let enter_input = input.clone();
            let enter_workspace = workspace.clone();
            dialog
                .title(title)
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(Input::new(&input))
                        .capture_key_down(move |event, window, cx| {
                            if event.keystroke.key == "enter" {
                                cx.stop_propagation();
                                Self::save_section_name(
                                    enter_workspace.clone(),
                                    id,
                                    enter_input.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                )
                .footer(move |_, _, _, cx| {
                    let input = save_input.clone();
                    let workspace = save_workspace.clone();
                    vec![
                        Button::new("save-section-name")
                            .primary()
                            .label(action_label)
                            .on_click(move |_, window, cx| {
                                Self::save_section_name(
                                    workspace.clone(),
                                    id,
                                    input.clone(),
                                    window,
                                    cx,
                                );
                            }),
                        Button::new("cancel-section-name")
                            .custom(crate::ui::style::dialog_neutral_variant(cx))
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    pub fn view(
        workspace: Entity<Workspace>,
        git_states: Entity<GitStates>,
        terminals: Entity<TerminalManager>,
        agent_chats: Entity<AgentChatState>,
        agents: Entity<AgentRecords>,
        agent_activity: Entity<AgentActivityCache>,
        center: WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&git_states, |_, _, cx| cx.notify()).detach();
            cx.observe(&terminals, |this: &mut Self, _, cx| {
                this.refresh_attention_autoexpand(cx);
                cx.notify();
            })
            .detach();
            cx.observe(&agent_chats, |this: &mut Self, _, cx| {
                this.refresh_attention_autoexpand(cx);
                cx.notify();
            })
            .detach();
            cx.observe(&agents, |this: &mut Self, _, cx| {
                this.refresh_attention_autoexpand(cx);
                cx.notify();
            })
            .detach();
            cx.subscribe(
                &agents,
                |this: &mut Self, agents, event: &AgentRecordsEvent, cx| {
                    if matches!(event, AgentRecordsEvent::SelectionChanged) {
                        let selected = this.workspace.read(cx).active.and_then(|project| {
                            agents.read(cx).explicitly_selected_agent_id(project)
                        });
                        if selected.is_some_and(|selected| Some(selected) != this.attention_pinned)
                        {
                            this.attention_pinned = None;
                        }
                    }
                },
            )
            .detach();
            cx.observe(&agent_activity, |this: &mut Self, _, cx| {
                this.refresh_attention_autoexpand(cx);
                cx.notify();
            })
            .detach();
            Self {
                workspace,
                git_states,
                terminals,
                agent_chats,
                agents,
                agent_activity,
                center,
                hovered_project: None,
                hovered_section: None,
                menu_project: None,
                menu_section: None,
                expanded_agent_lists: HashSet::new(),
                attention_pinned: None,
                hovered_attention: None,
                known_waiting: None,
            }
        })
    }

    fn agent_activity(
        &self,
        project: ProjectId,
        records: &[AgentRecord],
        cx: &App,
    ) -> (bool, usize) {
        let mut working = false;
        let mut waiting = 0;
        for agent in records.iter().filter(|agent| agent.project_id == project) {
            match self.runtime_for_agent(project, agent, cx) {
                ProjectAgentRuntime::Working => working = true,
                ProjectAgentRuntime::Waiting => waiting += 1,
                _ => {}
            }
        }
        (working, waiting)
    }

    fn collect_rows(&self, cx: &App) -> Vec<RowInfo> {
        let state = self.workspace.read(cx);
        let git_states = self.git_states.read(cx);
        let terminals = self.terminals.read(cx);
        let agent_records: Vec<AgentRecord> = state
            .projects
            .iter()
            .flat_map(|project| self.agents.read(cx).records_for_project(project.id))
            .collect();
        state
            .projects
            .iter()
            .enumerate()
            .map(|(ix, p)| {
                let git = git_states.get(p.id);
                let changes = git
                    .map(|git| {
                        let git = git.read(cx);
                        if git.is_repo {
                            let snapshot = git.snapshot.as_ref();
                            snapshot.map(|s| s.entries.len()).unwrap_or(0)
                        } else {
                            0
                        }
                    })
                    .unwrap_or(0);
                let (agents_working, agents_waiting) =
                    self.agent_activity(p.id, &agent_records, cx);
                let in_progress_agents = agent_records
                    .iter()
                    .filter(|agent| {
                        agent.project_id == p.id && agent.status == AgentStatus::InProgress
                    })
                    .cloned()
                    .collect();
                RowInfo {
                    ix,
                    id: p.id,
                    name: SharedString::from(p.name.clone()),
                    path: SharedString::from(p.path.display().to_string()),
                    icon: p.icon.clone(),
                    icon_color: p.icon_color.clone(),
                    section_id: p.section_id,
                    is_favorite: p.is_favorite,
                    is_active: state.active == Some(p.id),
                    changes,
                    scripts: terminals.running_scripts(p.id),
                    agents_working,
                    agents_waiting,
                    in_progress_agents,
                }
            })
            .collect()
    }
    fn open_rename_dialog(
        workspace: Entity<Workspace>,
        id: ProjectId,
        current_name: SharedString,
        path: SharedString,
        window: &mut Window,
        cx: &mut App,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Project name")
                .default_value(current_name.to_string())
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let path_color = crate::ui::design::t3(cx);

        window.open_dialog(cx, move |dialog, _, _| {
            let save_input = input.clone();
            let save_workspace = workspace.clone();
            let enter_input = input.clone();
            let enter_workspace = workspace.clone();
            dialog
                .title("Rename Project")
                .child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(Input::new(&input))
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(path_color)
                                .child(path.clone()),
                        )
                        .capture_key_down(move |event, window, cx| {
                            if event.keystroke.key == "enter" {
                                cx.stop_propagation();
                                Self::save_project_name(
                                    enter_workspace.clone(),
                                    id,
                                    enter_input.clone(),
                                    window,
                                    cx,
                                );
                            }
                        }),
                )
                .footer(move |_, _, _, cx| {
                    let input = save_input.clone();
                    let workspace = save_workspace.clone();
                    vec![
                        Button::new("save-project-name")
                            .primary()
                            .label("Rename")
                            .on_click(move |_, window, cx| {
                                Self::save_project_name(
                                    workspace.clone(),
                                    id,
                                    input.clone(),
                                    window,
                                    cx,
                                );
                            }),
                        Button::new("cancel-project-name")
                            .custom(crate::ui::style::dialog_neutral_variant(cx))
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ]
                })
        });
    }

    fn open_project_icon_dialog(
        workspace: Entity<Workspace>,
        id: ProjectId,
        current_name: SharedString,
        icon: String,
        icon_color: String,
        window: &mut Window,
        cx: &mut App,
    ) {
        let search_text = crate::ui::design::t1(cx);
        let search_placeholder = crate::ui::design::t3(cx);
        let search_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Search 1,700+ icons")
                .text_color(search_text)
                .placeholder_color(search_placeholder)
        });
        let dialog = cx.new(|cx| {
            cx.subscribe(&search_input, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            ProjectIconDialog {
                workspace,
                project: id,
                project_name: current_name,
                selected_icon: icon,
                selected_color: icon_color,
                search_input,
            }
        });
        let footer_dialog = dialog.clone();

        window.open_dialog(cx, move |dialog_view, _, _| {
            let save_dialog = footer_dialog.clone();
            dialog_view
                .title("Customize Project Icon")
                .w(px(440.))
                .child(footer_dialog.clone())
                .footer(move |_, _, _, cx| {
                    let save_dialog = save_dialog.clone();
                    vec![
                        Button::new("cancel-project-icon")
                            .small()
                            .custom(crate::ui::style::dialog_neutral_variant(cx))
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        Button::new("save-project-icon")
                            .primary()
                            .small()
                            .label("Done")
                            .on_click(move |_, window, cx| {
                                save_dialog.update(cx, |dialog, cx| dialog.save(window, cx));
                            }),
                    ]
                })
        });
    }

    fn render_agent_indicator(
        &self,
        row: &RowInfo,
        project_collapsed: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if row.agents_waiting > 0 {
            return div()
                .flex_none()
                .size(px(6.))
                .rounded_full()
                .bg(crate::ui::design::amber(cx))
                .into_any_element();
        }

        if row.agents_working && project_collapsed {
            return logo_spinner(16., "project-row-logo", row.ix, crate::ui::design::t3(cx));
        }

        div().into_any_element()
    }

    fn runtime_for_agent(
        &self,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &App,
    ) -> ProjectAgentRuntime {
        if agent.runtime == AgentRuntimeKind::Chat {
            let (status, session_id, last_activity_at) = self
                .agent_chats
                .read(cx)
                .session(agent.id)
                .map(|session| {
                    (
                        Some(session.status),
                        session
                            .chat_session_id
                            .clone()
                            .or_else(|| session.cli_session_id.clone()),
                        Some(session.last_activity_at),
                    )
                })
                .unwrap_or((None, None, None));
            return match status {
                Some(AgentChatStatus::Running | AgentChatStatus::Cancelling) => {
                    ProjectAgentRuntime::Working
                }
                Some(AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady) => {
                    ProjectAgentRuntime::Waiting
                }
                Some(AgentChatStatus::Failed) => ProjectAgentRuntime::Ended,
                _ if agent.started_at.is_some() => {
                    let manager = self.terminals.read(cx);
                    if let Some(session_id) = session_id
                        .as_deref()
                        .or(agent.chat_session_id.as_deref())
                        .or(agent.cli_session_id.as_deref())
                    {
                        let updated_at =
                            self.agent_activity
                                .read(cx)
                                .updated_at(agent.id)
                                .or_else(|| {
                                    last_activity_at
                                        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
                                });
                        if let Some(updated_at) = updated_at {
                            let working = std::time::SystemTime::now()
                                .duration_since(updated_at)
                                .map(|age| age < agents::WORKING_WINDOW)
                                .unwrap_or(false);
                            if working {
                                return ProjectAgentRuntime::Working;
                            }
                            if !manager.attention_suppressed(session_id, updated_at) {
                                return ProjectAgentRuntime::Waiting;
                            }
                        }
                    }
                    ProjectAgentRuntime::Idle
                }
                _ => ProjectAgentRuntime::NotStarted,
            };
        }

        let manager = self.terminals.read(cx);
        if let Some(session) = manager.agent_record_session(project, agent.id) {
            if session.exited {
                return ProjectAgentRuntime::Ended;
            }

            let session_id = session
                .agent_session_id
                .as_deref()
                .or(agent.cli_session_id.as_deref());
            if let Some(session_id) = session_id {
                if let Some(updated_at) = self.agent_activity.read(cx).updated_at(agent.id) {
                    let working = std::time::SystemTime::now()
                        .duration_since(updated_at)
                        .map(|age| age < agents::WORKING_WINDOW)
                        .unwrap_or(false);
                    if working {
                        return ProjectAgentRuntime::Working;
                    }
                    if !manager.attention_suppressed(session_id, updated_at) {
                        return ProjectAgentRuntime::Waiting;
                    }
                }
            }

            return ProjectAgentRuntime::Open;
        }

        if agent.started_at.is_some() || agent.cli_session_id.is_some() {
            ProjectAgentRuntime::Idle
        } else {
            ProjectAgentRuntime::NotStarted
        }
    }

    fn agent_activity_label(
        &self,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &Context<Self>,
    ) -> Option<SharedString> {
        let live_chat_time = (agent.runtime == AgentRuntimeKind::Chat)
            .then(|| {
                self.agent_chats
                    .read(cx)
                    .session(agent.id)
                    .map(|session| UNIX_EPOCH + Duration::from_secs(session.last_activity_at))
            })
            .flatten();

        let manager = self.terminals.read(cx);
        let transcript_time = manager
            .agent_record_session(project, agent.id)
            .and_then(|session| {
                session
                    .agent_session_id
                    .as_deref()
                    .or(agent.chat_session_id.as_deref())
                    .or(agent.cli_session_id.as_deref())
            })
            .or(agent.chat_session_id.as_deref())
            .or(agent.cli_session_id.as_deref())
            .and_then(|_| self.agent_activity.read(cx).updated_at(agent.id));

        let fallback_time = if agent.updated_at > 0 {
            Some(UNIX_EPOCH + Duration::from_secs(agent.updated_at))
        } else {
            agent
                .started_at
                .map(|started_at| UNIX_EPOCH + Duration::from_secs(started_at))
        };
        live_chat_time
            .or(transcript_time)
            .or(fallback_time)
            .map(compact_relative_time)
    }

    fn open_agent(
        &mut self,
        project: ProjectId,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.attention_pinned != Some(agent_id) {
            self.attention_pinned = None;
        }
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        self.agents
            .update(cx, |agents, cx| agents.select(project, agent_id, cx));
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_agent(agent_id, window, cx);
            });
        }
    }

    fn open_new_agent_for_project(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_new_agent_composer(window, cx);
            });
        }
    }

    fn select_or_toggle_project(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        self.workspace.update(cx, |workspace, cx| {
            if workspace.active == Some(project) {
                workspace.toggle_project_expanded(project, cx);
            } else {
                workspace.set_active(project, cx);
            }
        });
    }

    fn render_project_agent(
        &self,
        row_ix: usize,
        agent_ix: usize,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let runtime = self.runtime_for_agent(project, agent, cx);
        let waiting = runtime == ProjectAgentRuntime::Waiting;
        let active_project = self.workspace.read(cx).active;
        let selected = active_project == Some(project)
            && self.agents.read(cx).explicitly_selected_agent_id(project) == Some(agent_id);
        let warning = crate::ui::design::amber(cx);
        let activity_label = self.agent_activity_label(project, agent, cx);

        h_flex()
            .id(("project-agent-row", row_ix * 1000 + agent_ix))
            .w_full()
            .pl(px(34.))
            .pr_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .when(selected, |row| row.bg(crate::ui::design::surface_2(cx)))
            .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.5)))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_agent(project, agent_id, window, cx);
            }))
            // A Solo wears its fork before the name, sky-marked like everywhere.
            .when(agent.is_solo(), |row| {
                row.child(crate::ui::design::indicator::solo_icon(
                    crate::ui::design::sky(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    // Sidebar agent row (`.agent`): 13px, muted `t2` by default so the
                    // list reads calm; only the selected/waiting row lifts to `t1`.
                    .text_size(crate::ui::design::text_head())
                    .font_weight(if waiting {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if waiting {
                        warning
                    } else if selected {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .truncate()
                    .child(SharedString::from(agent.title.clone())),
            )
            .when(runtime == ProjectAgentRuntime::Working, |row| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(34.))
                        .flex()
                        .justify_end()
                        .child(logo_spinner(
                            16.,
                            "project-agent-logo",
                            row_ix * 1000 + agent_ix,
                            crate::ui::design::t3(cx),
                        )),
                )
            })
            .when(waiting, |row| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(34.))
                        .flex()
                        .justify_end()
                        .items_center()
                        .child(div().size(px(6.)).rounded_full().bg(warning)),
                )
            })
            .when(runtime != ProjectAgentRuntime::Working && !waiting, |row| {
                row.when_some(activity_label, |row, label| {
                    row.child(
                        div()
                            .flex_none()
                            .w(px(34.))
                            .flex()
                            .justify_end()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(label),
                    )
                })
            })
            .into_any_element()
    }

    fn render_project_agents_section(
        &self,
        row_ix: usize,
        project: ProjectId,
        agents: &[AgentRecord],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        const PREVIEW_LIMIT: usize = 5;
        let expanded = self.expanded_agent_lists.contains(&project);
        let visible_count = if expanded {
            agents.len()
        } else {
            agents.len().min(PREVIEW_LIMIT)
        };
        let hidden_count = agents.len().saturating_sub(PREVIEW_LIMIT);

        v_flex()
            .mt_0p5()
            .gap_0p5()
            .children(
                agents
                    .iter()
                    .enumerate()
                    .take(visible_count)
                    .map(|(agent_ix, agent)| {
                        self.render_project_agent(row_ix, agent_ix, project, agent, cx)
                    })
                    .collect::<Vec<_>>(),
            )
            .when(agents.len() > PREVIEW_LIMIT, |section| {
                section.child(
                    h_flex()
                        .id(("project-agent-list-toggle", row_ix))
                        .w_full()
                        .pl(px(34.))
                        .pr_2()
                        .py_1()
                        .gap_1p5()
                        .items_center()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.32)))
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(crate::ui::design::icon_sm()),
                        )
                        .child(if expanded {
                            "Show fewer agents".to_string()
                        } else {
                            format!("Show {hidden_count} more agents")
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !this.expanded_agent_lists.remove(&project) {
                                this.expanded_agent_lists.insert(project);
                            }
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    fn collect_attention_agents(
        &self,
        cx: &App,
    ) -> Vec<(ProjectId, SharedString, String, AgentRecord)> {
        let state = self.workspace.read(cx);
        let mut waiting = Vec::new();
        for project in &state.projects {
            let project_name = SharedString::from(project.name.clone());
            for agent in self.agents.read(cx).records_for_project(project.id) {
                let pinned = self.attention_pinned == Some(agent.id);
                if pinned
                    || self.runtime_for_agent(project.id, &agent, cx)
                        == ProjectAgentRuntime::Waiting
                {
                    waiting.push((
                        project.id,
                        project_name.clone(),
                        project.icon.clone(),
                        agent,
                    ));
                }
            }
        }
        waiting
    }

    /// Auto-expand a collapsed attention section when an agent starts waiting
    /// that wasn't waiting on the previous scan. The first scan only records
    /// the baseline so a persisted collapse survives app launch.
    fn refresh_attention_autoexpand(&mut self, cx: &mut Context<Self>) {
        let waiting: HashSet<Uuid> = self
            .collect_attention_agents(cx)
            .iter()
            .filter(|(_, _, _, agent)| self.attention_pinned != Some(agent.id))
            .map(|(_, _, _, agent)| agent.id)
            .collect();
        if let Some(known) = &self.known_waiting {
            let has_new = waiting.iter().any(|id| !known.contains(id));
            if has_new && self.workspace.read(cx).attention_collapsed {
                self.workspace.update(cx, |workspace, cx| {
                    workspace.toggle_attention_collapsed(cx);
                });
            }
        }
        self.known_waiting = Some(waiting);
    }

    fn render_attention_agent(
        &self,
        ix: usize,
        project: ProjectId,
        project_name: SharedString,
        project_icon_id: &str,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let warning = crate::ui::design::amber(cx);
        let selected = self.attention_pinned == Some(agent_id);
        let hovered = self.hovered_attention == Some(agent_id);

        h_flex()
            .id(("attention-agent-row", ix))
            .w_full()
            .pl(px(34.))
            .pr_2()
            .py_1()
            .gap_2()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .when(selected, |row| row.bg(crate::ui::design::surface_2(cx)))
            .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.5)))
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                this.hovered_attention = if *hovered {
                    Some(agent_id)
                } else if this.hovered_attention == Some(agent_id) {
                    None
                } else {
                    this.hovered_attention
                };
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.attention_pinned = Some(agent_id);
                this.open_agent(project, agent_id, window, cx);
            }))
            .when(agent.is_solo(), |row| {
                row.child(crate::ui::design::indicator::solo_icon(
                    crate::ui::design::sky(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_head())
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(warning)
                    .truncate()
                    .child(SharedString::from(agent.title.clone())),
            )
            .when(hovered, |row| {
                row.child(
                    div()
                        .flex_none()
                        .max_w(px(110.))
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t4(cx))
                        .truncate()
                        .child(project_name),
                )
            })
            .child(project_icon_glyph(
                project_icon(project_icon_id),
                warning,
                px(13.),
            ))
            .into_any_element()
    }

    fn render_attention_section(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let waiting = self.collect_attention_agents(cx);
        if waiting.is_empty() {
            return None;
        }

        let collapsed = self.workspace.read(cx).attention_collapsed;

        Some(
            v_flex()
                .w_full()
                .gap_0p5()
                .mb_3()
                .child(
                    h_flex()
                        .id("attention-section-header")
                        .w_full()
                        .h(px(30.))
                        .px_2()
                        .gap_1()
                        .items_center()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.28)))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.workspace.update(cx, |workspace, cx| {
                                workspace.toggle_attention_collapsed(cx);
                            });
                        }))
                        .child(
                            Icon::new(if collapsed {
                                IconName::ChevronRight
                            } else {
                                IconName::ChevronDown
                            })
                            .size(crate::ui::design::icon_sm())
                            .text_color(crate::ui::design::t4(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                // Same `.sect` treatment as the other sidebar headers.
                                .text_size(crate::ui::design::text_label())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t4(cx))
                                .child("NEEDS ATTENTION"),
                        )
                        .when(collapsed, |header| {
                            header.child(
                                div()
                                    .flex_none()
                                    .size(px(6.))
                                    .rounded_full()
                                    .bg(crate::ui::design::amber(cx)),
                            )
                        }),
                )
                .when(!collapsed, |section| {
                    section.children(
                        waiting
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (project, project_name, project_icon_id, agent))| {
                                self.render_attention_agent(
                                    ix,
                                    project,
                                    project_name,
                                    &project_icon_id,
                                    &agent,
                                    cx,
                                )
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .into_any_element(),
        )
    }

    fn build_project_menu(
        mut menu: PopupMenu,
        window: &mut Window,
        cx: &mut Context<PopupMenu>,
        workspace: Entity<Workspace>,
        row: RowInfo,
    ) -> PopupMenu {
        let id = row.id;
        let rename_name = row.name.clone();
        let rename_path = row.path.clone();
        let icon_name = row.name.clone();
        let current_icon = row.icon.clone();
        let current_icon_color = row.icon_color.clone();
        let open_with_path = row.path.clone();
        let copy_project_path = row.path.to_string();
        let is_favorite = row.is_favorite;
        let current_section = row.section_id;
        let sections = workspace.read(cx).project_sections.clone();

        menu = menu.item(
            PopupMenuItem::new(if is_favorite {
                "Remove from Favorites"
            } else {
                "Mark as Favorite"
            })
            .icon(if is_favorite {
                IconName::StarOff
            } else {
                IconName::Star
            })
            .on_click({
                let workspace = workspace.clone();
                move |_, _, cx| {
                    workspace.update(cx, |workspace, cx| {
                        workspace.set_project_favorite(id, !is_favorite, cx);
                    });
                }
            }),
        );

        menu = menu.submenu("Move to Section", window, cx, {
            let workspace = workspace.clone();
            move |mut submenu, _, _| {
                submenu = submenu.item(
                    PopupMenuItem::new("Projects")
                        .icon(IconName::Folder)
                        .checked(!is_favorite && current_section.is_none())
                        .on_click({
                            let workspace = workspace.clone();
                            move |_, _, cx| {
                                workspace.update(cx, |workspace, cx| {
                                    workspace.move_project_to_section(id, None, cx);
                                });
                            }
                        }),
                );
                for section in sections.clone() {
                    let section_id = section.id;
                    submenu = submenu.item(
                        PopupMenuItem::new(section.name)
                            .icon(IconName::FolderClosed)
                            .checked(!is_favorite && current_section == Some(section_id))
                            .on_click({
                                let workspace = workspace.clone();
                                move |_, _, cx| {
                                    workspace.update(cx, |workspace, cx| {
                                        workspace.move_project_to_section(id, Some(section_id), cx);
                                    });
                                }
                            }),
                    );
                }
                submenu
            }
        });

        menu.separator()
            .item(
                PopupMenuItem::new("Rename")
                    .icon(IconName::ALargeSmall)
                    .on_click({
                        let workspace = workspace.clone();
                        move |_, window, cx| {
                            Self::open_rename_dialog(
                                workspace.clone(),
                                id,
                                rename_name.clone(),
                                rename_path.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
            )
            .item(
                PopupMenuItem::new("Customize Icon")
                    .icon(IconName::Palette)
                    .on_click({
                        let workspace = workspace.clone();
                        move |_, window, cx| {
                            Self::open_project_icon_dialog(
                                workspace.clone(),
                                id,
                                icon_name.clone(),
                                current_icon.clone(),
                                current_icon_color.clone(),
                                window,
                                cx,
                            );
                        }
                    }),
            )
            .separator()
            .submenu("Open With", window, cx, {
                let path = open_with_path.clone();
                move |mut submenu, _, _| {
                    for app in crate::open_with::available_apps() {
                        let path = path.clone();
                        submenu =
                            submenu.item(PopupMenuItem::new(app.label).on_click(move |_, _, _| {
                                crate::open_with::open_in(app.app, path.as_ref());
                            }));
                    }
                    submenu
                }
            })
            .item(
                PopupMenuItem::new("Copy Project Path")
                    .icon(IconName::Copy)
                    .on_click(move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                            copy_project_path.clone(),
                        ));
                    }),
            )
            .separator()
            .item(
                PopupMenuItem::new("Remove from list")
                    .icon(Icon::new(IconName::Close).text_color(crate::ui::design::rose(cx)))
                    .on_click({
                        let workspace = workspace.clone();
                        move |_, _, cx| {
                            workspace.update(cx, |workspace, cx| {
                                workspace.remove_project(id, cx);
                            });
                        }
                    }),
            )
    }

    fn toggle_sidebar_section(
        workspace: &mut Workspace,
        kind: SidebarSectionKind,
        cx: &mut Context<Workspace>,
    ) {
        match kind {
            SidebarSectionKind::Favorites => workspace.toggle_favorites_collapsed(cx),
            SidebarSectionKind::Projects => workspace.toggle_projects_collapsed(cx),
            SidebarSectionKind::Custom(id) => workspace.toggle_section_collapsed(id, cx),
        }
    }

    fn render_section_header(
        &self,
        section: &SidebarSection,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let kind = section.kind;
        let label = section.label.clone();
        let rename_label = section.label.clone();
        let collapsed = section.collapsed;
        let custom_section_id = match kind {
            SidebarSectionKind::Custom(id) => Some(id),
            SidebarSectionKind::Favorites | SidebarSectionKind::Projects => None,
        };
        let show_actions = custom_section_id.is_some_and(|section_id| {
            self.hovered_section == Some(section_id) || self.menu_section == Some(section_id)
        });
        let workspace = self.workspace.clone();
        let rename_workspace = self.workspace.clone();
        let delete_workspace = self.workspace.clone();
        let reorder_workspace = self.workspace.clone();
        let (can_move_up, can_move_down) = custom_section_id
            .and_then(|section_id| {
                let sections = &self.workspace.read(cx).project_sections;
                sections
                    .iter()
                    .position(|section| section.id == section_id)
                    .map(|ix| (ix > 0, ix + 1 < sections.len()))
            })
            .unwrap_or((false, false));

        let header = h_flex()
            .id(("project-section-header", section.ix))
            .w_full()
            .h(px(30.))
            .px_2()
            .gap_1()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.28)))
            .on_hover(cx.listener(move |this, hovered, _, cx| {
                let Some(section_id) = custom_section_id else {
                    return;
                };
                if *hovered {
                    this.hovered_section = Some(section_id);
                } else if this.hovered_section == Some(section_id) {
                    this.hovered_section = None;
                }
                cx.notify();
            }))
            .on_click(cx.listener(move |_, _, _, cx| {
                workspace.update(cx, |workspace, cx| {
                    Self::toggle_sidebar_section(workspace, kind, cx);
                });
            }))
            .child(
                Icon::new(if collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                })
                .size(crate::ui::design::icon_sm())
                .text_color(crate::ui::design::t4(cx)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    // The design's `.sect`: muted `t4`, uppercase, semibold.
                    .text_size(crate::ui::design::text_label())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t4(cx))
                    .truncate()
                    .child(SharedString::from(label.to_uppercase())),
            );

        match kind {
            SidebarSectionKind::Custom(section_id) => header
                .on_drag(
                    DragProjectSection {
                        id: section_id,
                        label: section.label.clone(),
                    },
                    |drag, _, _, cx| {
                        cx.stop_propagation();
                        cx.new(|_| drag.clone())
                    },
                )
                .drag_over::<DragProjectSection>(move |style, drag, _, cx| {
                    if drag.id == section_id {
                        style
                    } else {
                        style
                            .border_t_2()
                            .border_color(crate::ui::design::accent(cx))
                    }
                })
                .on_drop(cx.listener(move |this, drag: &DragProjectSection, _, cx| {
                    if drag.id == section_id {
                        return;
                    }
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.move_section_before(drag.id, section_id, cx);
                    });
                }))
                .when(show_actions, |header| {
                    header.child(
                        Button::new(("project-section-actions", section.ix))
                            .ghost()
                            .xsmall()
                            .icon(
                                Icon::new(IconName::Ellipsis)
                                    .size(crate::ui::design::icon())
                                    .text_color(crate::ui::design::t3(cx)),
                            )
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .dropdown_menu(move |menu, _window, _| {
                                menu.item(
                                    PopupMenuItem::new("Rename Section")
                                        .icon(IconName::ALargeSmall)
                                        .on_click({
                                            let workspace = rename_workspace.clone();
                                            let label = rename_label.clone();
                                            move |_, window, cx| {
                                                Self::open_section_name_dialog(
                                                    workspace.clone(),
                                                    Some(section_id),
                                                    label.clone(),
                                                    window,
                                                    cx,
                                                );
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Move Up")
                                        .icon(IconName::ArrowUp)
                                        .disabled(!can_move_up)
                                        .on_click({
                                            let workspace = reorder_workspace.clone();
                                            move |_, _, cx| {
                                                workspace.update(cx, |workspace, cx| {
                                                    workspace.move_section_up(section_id, cx);
                                                });
                                            }
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Move Down")
                                        .icon(IconName::ArrowDown)
                                        .disabled(!can_move_down)
                                        .on_click({
                                            let workspace = reorder_workspace.clone();
                                            move |_, _, cx| {
                                                workspace.update(cx, |workspace, cx| {
                                                    workspace.move_section_down(section_id, cx);
                                                });
                                            }
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Delete Section")
                                        .icon(IconName::Delete)
                                        .on_click({
                                            let workspace = delete_workspace.clone();
                                            move |_, _, cx| {
                                                workspace.update(cx, |workspace, cx| {
                                                    workspace.delete_section(section_id, cx);
                                                });
                                            }
                                        }),
                                )
                            })
                            // The trigger only renders while the row is hovered;
                            // hold it in place for as long as its menu is open.
                            .on_open_change(cx.listener(move |this, open: &bool, _, cx| {
                                this.menu_section = open.then_some(section_id);
                                cx.notify();
                            })),
                    )
                })
                .context_menu({
                    let workspace = self.workspace.clone();
                    let reorder_workspace = self.workspace.clone();
                    let label = section.label.clone();
                    move |menu, _window, _| {
                        menu.item(
                            PopupMenuItem::new("Rename Section")
                                .icon(IconName::ALargeSmall)
                                .on_click({
                                    let workspace = workspace.clone();
                                    let label = label.clone();
                                    move |_, window, cx| {
                                        Self::open_section_name_dialog(
                                            workspace.clone(),
                                            Some(section_id),
                                            label.clone(),
                                            window,
                                            cx,
                                        );
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Move Up")
                                .icon(IconName::ArrowUp)
                                .disabled(!can_move_up)
                                .on_click({
                                    let workspace = reorder_workspace.clone();
                                    move |_, _, cx| {
                                        workspace.update(cx, |workspace, cx| {
                                            workspace.move_section_up(section_id, cx);
                                        });
                                    }
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Move Down")
                                .icon(IconName::ArrowDown)
                                .disabled(!can_move_down)
                                .on_click({
                                    let workspace = reorder_workspace.clone();
                                    move |_, _, cx| {
                                        workspace.update(cx, |workspace, cx| {
                                            workspace.move_section_down(section_id, cx);
                                        });
                                    }
                                }),
                        )
                        .separator()
                        .item(
                            PopupMenuItem::new("Delete Section")
                                .icon(IconName::Delete)
                                .on_click({
                                    let workspace = workspace.clone();
                                    move |_, _, cx| {
                                        workspace.update(cx, |workspace, cx| {
                                            workspace.delete_section(section_id, cx);
                                        });
                                    }
                                }),
                        )
                    }
                })
                .into_any_element(),
            SidebarSectionKind::Favorites | SidebarSectionKind::Projects => {
                header.into_any_element()
            }
        }
    }

    fn render_sidebar_section(
        &self,
        section: SidebarSection,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let collapsed = section.collapsed;
        v_flex()
            .id(("project-sidebar-section", section.ix))
            .w_full()
            .gap_0p5()
            .when(section.ix > 0, |section| section.mt_4())
            .child(self.render_section_header(&section, cx))
            .when(!collapsed, |column| {
                column.children(
                    section
                        .rows
                        .into_iter()
                        .map(|row| self.render_row(row, cx))
                        .collect::<Vec<_>>(),
                )
            })
            .into_any_element()
    }

    fn render_row(&self, row: RowInfo, cx: &mut Context<Self>) -> impl IntoElement {
        let workspace = self.workspace.clone();
        let dropdown_workspace = self.workspace.clone();
        let context_workspace = self.workspace.clone();
        let id = row.id;
        let has_changes = row.changes > 0;
        let has_scripts = !row.scripts.is_empty();
        let scripts = row.scripts.clone();
        let path = row.path.clone();
        let dropdown_row = row.clone();
        let context_row = row.clone();
        let collapsed = !self.workspace.read(cx).expanded_projects.contains(&id);
        let hovered = self.hovered_project == Some(id);
        let show_actions = hovered || row.is_active || self.menu_project == Some(id);
        let has_in_progress_agents = !row.in_progress_agents.is_empty();
        let agents_section =
            self.render_project_agents_section(row.ix, id, &row.in_progress_agents, cx);

        v_flex()
            .id(("project-row", row.ix))
            .w_full()
            .gap_0p5()
            .when(row.ix > 0, |row| row.pt_2())
            .child(
                v_flex()
                    .id(("project-row-card", row.ix))
                    .w_full()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .bg(if row.is_active {
                        crate::ui::design::surface_2(cx)
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(|r| r.bg(crate::ui::design::surface_2(cx).opacity(0.5)))
                    .on_hover(cx.listener(move |this, hovered, _, cx| {
                        this.hovered_project = if *hovered { Some(id) } else { None };
                        cx.notify();
                    }))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        workspace.update(cx, |workspace, cx| workspace.set_active(id, cx));
                    }))
                    .child(
                        h_flex()
                            .id(("project-row-header", row.ix))
                            .w_full()
                            .gap_1p5()
                            .items_center()
                            .child(
                                div()
                                    .id(("project-icon", row.ix))
                                    .w(px(22.))
                                    .h(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.select_or_toggle_project(id, cx);
                                    }))
                                    .child(project_icon_element(
                                        &row.icon,
                                        &row.icon_color,
                                        px(22.),
                                        px(16.),
                                        cx,
                                    )),
                            )
                            .child(
                                h_flex()
                                    .id(("project-name", row.ix))
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .items_center()
                                    .cursor_pointer()
                                    .tooltip({
                                        let path = path.clone();
                                        move |window, cx| {
                                            Tooltip::new(path.clone()).build(window, cx)
                                        }
                                    })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        this.select_or_toggle_project(id, cx);
                                    }))
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            // Sidebar project row (`.proj`): 13px, medium.
                                            .text_size(crate::ui::design::text_head())
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(crate::ui::design::t1(cx))
                                            .truncate()
                                            .child(row.name.clone()),
                                    )
                                    .when(has_changes, |name| {
                                        // Two-tone ±: sage plus over rose minus, dimmed a
                                        // step so this always-on indicator stays behind
                                        // the amber attention dot in visual priority.
                                        name.child(
                                            v_flex()
                                                .flex_none()
                                                .items_center()
                                                .child(
                                                    div()
                                                        .text_size(px(10.))
                                                        .line_height(px(6.))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(
                                                            crate::ui::design::sage(cx)
                                                                .opacity(0.75),
                                                        )
                                                        .child("+"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(10.))
                                                        .line_height(px(5.))
                                                        .font_weight(FontWeight::SEMIBOLD)
                                                        .text_color(
                                                            crate::ui::design::rose(cx)
                                                                .opacity(0.75),
                                                        )
                                                        .child("−"),
                                                ),
                                        )
                                    })
                                    .when(hovered, |name| {
                                        name.child(
                                            style::icon_button(
                                                ("project-collapse", row.ix),
                                                if collapsed {
                                                    IconName::ChevronRight
                                                } else {
                                                    IconName::ChevronDown
                                                },
                                                cx,
                                            )
                                            .tooltip(if collapsed {
                                                "Expand project"
                                            } else {
                                                "Collapse project"
                                            })
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    this.workspace.update(cx, |workspace, cx| {
                                                        workspace.toggle_project_expanded(id, cx);
                                                    });
                                                }),
                                            ),
                                        )
                                    }),
                            )
                            .when(
                                collapsed && (row.agents_working || row.agents_waiting > 0),
                                |line| line.child(self.render_agent_indicator(&row, collapsed, cx)),
                            )
                            .when(show_actions, |line| {
                                line.child(
                                    style::icon_button(
                                        ("project-create-agent", row.ix),
                                        IconName::Plus,
                                        cx,
                                    )
                                    .tooltip("Create agent")
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            cx.stop_propagation();
                                            this.open_new_agent_for_project(id, window, cx);
                                        },
                                    )),
                                )
                                .child(
                                    style::icon_button(
                                        ("project-actions", row.ix),
                                        IconName::Ellipsis,
                                        cx,
                                    )
                                    .dropdown_menu(move |menu, window, cx| {
                                        Self::build_project_menu(
                                            menu,
                                            window,
                                            cx,
                                            dropdown_workspace.clone(),
                                            dropdown_row.clone(),
                                        )
                                    })
                                    // Keep the trigger mounted while its menu is
                                    // open, so leaving the row can't take the
                                    // menu down mid-selection.
                                    .on_open_change(
                                        cx.listener(move |this, open: &bool, _, cx| {
                                            this.menu_project = open.then_some(id);
                                            cx.notify();
                                        }),
                                    ),
                                )
                            }),
                    )
                    .when(has_scripts, |card| {
                        let running = crate::ui::design::sage(cx);
                        // Real scripts first; Solo lane runs after them, sky
                        // colored and reduced to just the Solo's name.
                        let (solos, real): (Vec<_>, Vec<_>) =
                            scripts.into_iter().partition(|script| {
                                crate::state::terminals::solo_script_slug(script.as_ref()).is_some()
                            });
                        card.child(
                            h_flex()
                                .pl(px(24.))
                                .gap_1p5()
                                .flex_wrap()
                                .children(
                                    real.into_iter()
                                        .map(|script| style::script_chip(script, running, cx)),
                                )
                                .children(solos.into_iter().map(|script| {
                                    let slug =
                                        crate::state::terminals::solo_script_slug(script.as_ref())
                                            .unwrap_or_default()
                                            .to_string();
                                    style::solo_script_chip(slug, cx)
                                })),
                        )
                    })
                    .context_menu(move |menu, window, cx| {
                        Self::build_project_menu(
                            menu,
                            window,
                            cx,
                            context_workspace.clone(),
                            context_row.clone(),
                        )
                    }),
            )
            .when(!collapsed && has_in_progress_agents, |column| {
                column.child(agents_section)
            })
    }
}

impl Render for ProjectList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.collect_rows(cx);
        let (project_sections, favorites_collapsed, projects_collapsed) = {
            let workspace = self.workspace.read(cx);
            (
                workspace.project_sections.clone(),
                workspace.favorites_collapsed,
                workspace.projects_collapsed,
            )
        };
        let mut sections = Vec::new();
        let favorite_rows = rows
            .iter()
            .filter(|row| row.is_favorite)
            .cloned()
            .collect::<Vec<_>>();
        if !favorite_rows.is_empty() {
            sections.push(SidebarSection {
                ix: sections.len(),
                label: "Favorites".into(),
                kind: SidebarSectionKind::Favorites,
                collapsed: favorites_collapsed,
                rows: favorite_rows,
            });
        }
        let project_rows = rows
            .iter()
            .filter(|row| !row.is_favorite && row.section_id.is_none())
            .cloned()
            .collect::<Vec<_>>();
        for project_section in project_sections {
            let section_id = project_section.id;
            let section_rows = rows
                .iter()
                .filter(|row| !row.is_favorite && row.section_id == Some(section_id))
                .cloned()
                .collect::<Vec<_>>();
            sections.push(SidebarSection {
                ix: sections.len(),
                label: SharedString::from(project_section.name),
                kind: SidebarSectionKind::Custom(section_id),
                collapsed: project_section.collapsed,
                rows: section_rows,
            });
        }
        if !project_rows.is_empty() {
            sections.push(SidebarSection {
                ix: sections.len(),
                label: "Projects".into(),
                kind: SidebarSectionKind::Projects,
                collapsed: projects_collapsed,
                rows: project_rows,
            });
        }
        let is_empty = sections.is_empty();
        let attention_section = self.render_attention_section(cx);

        v_flex()
            .size_full()
            .px_2()
            .py_2()
            .gap_1p5()
            .child(
                v_flex()
                    .id("project-rows")
                    .flex_1()
                    .gap_0p5()
                    .overflow_y_scroll()
                    .children(attention_section)
                    .children(
                        sections
                            .into_iter()
                            .map(|section| self.render_sidebar_section(section, cx))
                            .collect::<Vec<_>>(),
                    )
                    .when(is_empty, |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_4()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No projects yet.\nAdd a project to get started."),
                        )
                    }),
            )
            .min_w(px(0.))
    }
}
