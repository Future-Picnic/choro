use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use gpui::{
    div, ease_in_out, ease_out_quint, prelude::FluentBuilder, px, uniform_list, Animation,
    AnimationExt, App, AppContext, Context, Entity, FontWeight, InteractiveElement, IntoElement,
    ParentElement, PathPromptOptions, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{ContextMenuExt, DropdownMenu as _, PopupMenu, PopupMenuItem},
    tooltip::Tooltip,
    v_flex, Icon, IconName, Sizable, WindowExt,
};
use ide_core::{
    agents, AgentRecord, AgentRuntimeKind, AgentStatus, ProjectId, ProjectSectionId,
    CUSTOM_PROJECT_SVG_ICON,
};
use uuid::Uuid;

use crate::state::agent_chat::AgentChatStatus;
use crate::state::agents::AgentRecordsEvent;
use crate::state::delegation::display::DelegationActivity;
use crate::state::{
    AgentActivityCache, AgentChatState, AgentRecords, GitStates, TerminalManager, Workspace,
};
use crate::ui::center::CenterArea;
use crate::ui::logo_spinner::{delegation_spinner, logo_spinner};
use crate::ui::project_visuals::{
    import_project_svg, project_color_options, project_icon_color, project_icon_element,
    project_icon_glyph, project_icon_options, project_icon_visual_glyph, POPULAR_PROJECT_ICONS,
};
use crate::ui::style;

mod agent_hover_card;
mod delegation_rows;

/// Everything a sidebar row displays for one project.
#[derive(Clone)]
struct RowInfo {
    ix: usize,
    id: ProjectId,
    name: SharedString,
    path: SharedString,
    icon: String,
    icon_color: String,
    icon_image_path: Option<PathBuf>,
    section_id: Option<ProjectSectionId>,
    is_favorite: bool,
    is_active: bool,
    /// Files with uncommitted changes.
    changes: usize,
    /// Names of script presets currently running.
    scripts: Vec<SharedString>,
    /// Any live agent terminal actively writing for this project.
    agents_working: bool,
    /// Work in progress only because Experts are busy on a lead's behalf.
    agents_delegating: bool,
    /// Agents waiting for user attention.
    agents_waiting: usize,
    /// Agents included by the selected view (the In progress lane in All agents).
    in_progress_agents: Vec<AgentRecord>,
}

/// Budget for the list's 16px padding and card's 24px padding. Script chips
/// start at the project icon, size to their labels up to a readable cap, and
/// reserve the overflow indicator before choosing how many fit on one line.
fn visible_sidebar_script_count(scripts: &[(SharedString, bool)], sidebar_width: f32) -> usize {
    const INSET: f32 = 40.;
    const GAP: f32 = 4.;
    let available = (sidebar_width - INSET).max(0.);
    let all_width = scripts
        .iter()
        .map(|(name, _)| style::sidebar_script_chip_width(name.as_ref()))
        .sum::<f32>()
        + GAP * scripts.len().saturating_sub(1) as f32;
    if all_width <= available {
        return scripts.len();
    }

    // The total is a conservative digit-width bound for the eventual hidden
    // count, so the +N chip cannot get clipped when crossing 9 or 99 scripts.
    let chip_budget =
        (available - style::sidebar_script_overflow_width(scripts.len()) - GAP).max(0.);
    let mut used = 0.;
    scripts
        .iter()
        .take_while(|(name, _)| {
            let width = style::sidebar_script_chip_width(name.as_ref());
            let next = if used == 0. {
                width
            } else {
                used + GAP + width
            };
            if next > chip_budget {
                return false;
            }
            used = next;
            true
        })
        .count()
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

/// Live activity rolled up per project for the collapsed row indicator.
#[derive(Clone, Copy, Default)]
struct ProjectActivity {
    working: bool,
    delegating: bool,
    waiting: usize,
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

/// Active work has its own single expanded project; normal project expansion
/// remains in Workspace and is restored when returning to All agents.
struct AgentListView {
    active_work: bool,
    expanded_project: Option<ProjectId>,
}

impl Default for AgentListView {
    fn default() -> Self {
        Self {
            active_work: true,
            expanded_project: None,
        }
    }
}

impl AgentListView {
    fn toggle_project(&mut self, project: ProjectId) {
        self.expanded_project = (self.expanded_project != Some(project)).then_some(project);
    }

    fn includes(
        &self,
        project: ProjectId,
        status: AgentStatus,
        runtime: ProjectAgentRuntime,
    ) -> bool {
        if !self.active_work {
            return status == AgentStatus::InProgress;
        }
        !status.is_finished()
            && (matches!(
                runtime,
                ProjectAgentRuntime::Working | ProjectAgentRuntime::Waiting
            ) || (self.expanded_project == Some(project) && status == AgentStatus::InProgress))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoveredAgentRow {
    Project(Uuid),
    Pinned(Uuid),
    Attention(Uuid),
}

struct ProjectIconDialog {
    workspace: Entity<Workspace>,
    project: ProjectId,
    project_name: SharedString,
    selected_icon: String,
    selected_color: String,
    selected_svg_path: Option<PathBuf>,
    selected_svg_name: Option<SharedString>,
    import_error: Option<SharedString>,
    search_input: Entity<InputState>,
}

impl ProjectIconDialog {
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let icon = self.selected_icon.clone();
        let color = self.selected_color.clone();
        let svg_path = self.selected_svg_path.clone();
        self.workspace.update(cx, |workspace, cx| {
            workspace.set_project_visual(self.project, icon, color, svg_path, cx);
        });
        window.close_dialog(cx);
    }

    fn import_svg(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Import SVG".into()),
        });
        let project = self.project;
        cx.spawn(async move |this, cx| {
            let source = match receiver.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                _ => None,
            };
            let Some(source) = source else {
                return;
            };
            let source_name = source
                .file_name()
                .map(|name| SharedString::from(name.to_string_lossy().into_owned()))
                .unwrap_or_else(|| SharedString::from("Custom SVG"));
            let result = import_project_svg(project, &source);
            this.update(cx, |this, cx| {
                match result {
                    Ok(path) => {
                        this.selected_icon = CUSTOM_PROJECT_SVG_ICON.to_string();
                        this.selected_svg_path = Some(path);
                        this.selected_svg_name = Some(source_name);
                        this.import_error = None;
                    }
                    Err(error) => {
                        this.import_error = Some(SharedString::from(error.to_string()));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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
                                self.selected_svg_path.as_deref(),
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
                                                                            this.selected_svg_path =
                                                                            None;
                                                                            this.selected_svg_name =
                                                                            None;
                                                                            this.import_error =
                                                                                None;
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
                h_flex()
                    .w_full()
                    .gap_3()
                    .items_center()
                    .rounded(crate::ui::design::r_sm())
                    .border_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.4))
                    .px_3()
                    .py_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(
                                        self.selected_svg_name
                                            .clone()
                                            .unwrap_or_else(|| "Custom SVG".into()),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_label())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child("Simple monochrome SVG · recolored with the palette"),
                            ),
                    )
                    .child(
                        style::dialog_neutral_button(
                            "import-project-svg",
                            if self.selected_svg_path.is_some() {
                                "Replace SVG"
                            } else {
                                "Import SVG"
                            },
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.import_svg(cx))),
                    ),
            )
            .when_some(self.import_error.clone(), |content, error| {
                content.child(
                    div()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
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
    /// Leads whose delegated Expert rows are disclosed. Never set by activity:
    /// only the chevron on the lead's own row toggles it.
    expanded_delegations: HashSet<Uuid>,
    agent_list_view: AgentListView,
    /// Agent opened from the attention section; kept visible there until another
    /// agent is opened, so the row doesn't vanish under the click.
    attention_pinned: Option<Uuid>,
    hovered_agent: Option<HoveredAgentRow>,
    agent_hover: agent_hover_card::SidebarAgentHover,
    /// Waiting agent ids seen on the last refresh; `None` until the first scan.
    /// A newly waiting agent auto-expands a collapsed attention section, but the
    /// baseline scan at startup respects the persisted collapse.
    known_waiting: Option<HashSet<Uuid>>,
    /// Snapshot used to distinguish real renames from the many status changes
    /// that also emit AgentRecordsEvent::Changed.
    known_agent_titles: HashMap<Uuid, String>,
    /// Bumped per rename so GPUI mounts one fresh, one-shot title animation.
    agent_title_animation_epochs: HashMap<Uuid, u64>,
}

impl ProjectList {
    fn refresh_agent_title_animations(&mut self, records: Vec<AgentRecord>) {
        let next_titles = records
            .into_iter()
            .map(|agent| (agent.id, agent.title))
            .collect::<HashMap<_, _>>();
        for (agent_id, title) in &next_titles {
            if self
                .known_agent_titles
                .get(agent_id)
                .is_some_and(|known| known != title)
            {
                let epoch = self
                    .agent_title_animation_epochs
                    .entry(*agent_id)
                    .or_insert(0);
                *epoch = epoch.wrapping_add(1);
            }
        }
        self.agent_title_animation_epochs
            .retain(|agent_id, _| next_titles.contains_key(agent_id));
        self.known_agent_titles = next_titles;
    }

    fn render_agent_title(
        &self,
        agent: &AgentRecord,
        hovered: bool,
        weight: FontWeight,
        color: gpui::Hsla,
        cx: &App,
    ) -> gpui::AnyElement {
        // Only mount a one-shot animation for the single hovered title, and
        // only when its character count is likely to overflow the compact row.
        // This keeps idle sidebar rows completely animation-free.
        let title_chars = agent.title.chars().count();
        if hovered && title_chars > 28 {
            let travel = (((title_chars - 28) as f32 * 6.4) + 12.0).min(420.0);
            let duration = Duration::from_millis((1_500.0 + travel * 8.0) as u64);
            let seed = (agent.id.as_u128() as u64).rotate_left(11);
            return div()
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .text_size(crate::ui::design::text_head())
                .font_weight(weight)
                .text_color(color)
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .child(SharedString::from(agent.title.clone()))
                        .with_animation(
                            ("agent-title-hover-reveal", seed),
                            Animation::new(duration).with_easing(ease_in_out),
                            move |title, delta| title.relative().left(px(-travel * delta)),
                        ),
                )
                .into_any_element();
        }

        let title = div()
            .flex_1()
            .min_w(px(0.))
            .text_size(crate::ui::design::text_head())
            .font_weight(weight)
            .text_color(color)
            .truncate()
            .child(SharedString::from(agent.title.clone()));

        let Some(epoch) = self.agent_title_animation_epochs.get(&agent.id).copied() else {
            return title.into_any_element();
        };
        let seed = (agent.id.as_u128() as u64)
            .rotate_left(17)
            .wrapping_add(epoch);
        let accent = crate::ui::design::accent(cx);
        title
            .rounded(crate::ui::design::r_sm())
            .with_animation(
                ("agent-title-rename", seed),
                Animation::new(Duration::from_millis(280)).with_easing(ease_out_quint()),
                move |title, delta| {
                    // Primary: the resolved name settles upward. Secondary:
                    // opacity catches up. Ambient: a quiet accent wash recedes.
                    title
                        .relative()
                        .top(px((1.0 - delta) * 3.0))
                        .opacity(0.38 + delta * 0.62)
                        .bg(accent.opacity((1.0 - delta) * 0.13))
                },
            )
            .into_any_element()
    }

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
            let known_agent_titles = agents
                .read(cx)
                .all_records()
                .into_iter()
                .map(|agent| (agent.id, agent.title))
                .collect();
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
                    if matches!(event, AgentRecordsEvent::Changed) {
                        this.refresh_agent_title_animations(agents.read(cx).all_records());
                    }
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
            // Delegation is application-owned: a lead's in-progress state must
            // update here even while a different chat is selected.
            if let Some(handle) = cx
                .try_global::<crate::state::delegation::DelegationHandle>()
                .cloned()
            {
                cx.observe(&handle.0, |this: &mut Self, _, cx| {
                    this.refresh_attention_autoexpand(cx);
                    cx.notify();
                })
                .detach();
            }
            let sidebar_active_work = workspace.read(cx).sidebar_active_work;
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
                expanded_delegations: HashSet::new(),
                agent_list_view: AgentListView {
                    active_work: sidebar_active_work,
                    expanded_project: None,
                },
                attention_pinned: None,
                hovered_agent: None,
                agent_hover: Default::default(),
                known_waiting: None,
                known_agent_titles,
                agent_title_animation_epochs: HashMap::new(),
            }
        })
    }

    fn agent_activity(
        &self,
        project: ProjectId,
        records: &[AgentRecord],
        cx: &App,
    ) -> ProjectActivity {
        let mut activity = ProjectActivity::default();
        for agent in records
            .iter()
            .filter(|agent| agent.project_id == project && !agent.status.is_finished())
        {
            match self.runtime_for_agent(project, agent, cx) {
                ProjectAgentRuntime::Working => {
                    activity.working = true;
                    if self.delegation_activity_for(agent.id, cx) == DelegationActivity::Working {
                        activity.delegating = true;
                    }
                }
                ProjectAgentRuntime::Waiting => activity.waiting += 1,
                _ => {}
            }
        }
        activity
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
                let activity = self.agent_activity(p.id, &agent_records, cx);
                let in_progress_agents = agent_records
                    .iter()
                    .filter(|agent| {
                        agent.project_id == p.id
                            && self.agent_list_view.includes(
                                p.id,
                                agent.status,
                                if self.agent_list_view.active_work {
                                    self.runtime_for_agent(p.id, agent, cx)
                                } else {
                                    ProjectAgentRuntime::Idle
                                },
                            )
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
                    icon_image_path: p.icon_image_path.clone(),
                    section_id: p.section_id,
                    is_favorite: p.is_favorite,
                    is_active: state.active == Some(p.id),
                    changes,
                    scripts: terminals.running_scripts(p.id),
                    agents_working: activity.working,
                    agents_delegating: activity.delegating,
                    agents_waiting: activity.waiting,
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
        icon_image_path: Option<PathBuf>,
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
                selected_svg_name: icon_image_path
                    .as_ref()
                    .map(|_| SharedString::from("Custom SVG")),
                selected_svg_path: icon_image_path,
                import_error: None,
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
                        style::dialog_neutral_button("cancel-project-icon", "Cancel", cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                        style::primary_button_compact("save-project-icon", "Done", cx).on_click(
                            move |_, window, cx| {
                                save_dialog.update(cx, |dialog, cx| dialog.save(window, cx));
                            },
                        ),
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
            if row.agents_delegating {
                return delegation_spinner(
                    16.,
                    "project-row-delegation",
                    row.ix,
                    crate::ui::design::amber(cx),
                );
            }
            return logo_spinner(16., "project-row-logo", row.ix, crate::ui::design::t3(cx));
        }

        div().into_any_element()
    }

    /// The provider's own runtime, overlaid with application-owned delegation
    /// state: a lead whose Experts are busy is in progress even while its own
    /// provider sits idle, and an Expert waiting on a person is real attention.
    /// Provider status itself is never rewritten.
    fn runtime_for_agent(
        &self,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &App,
    ) -> ProjectAgentRuntime {
        let provider = self.provider_runtime_for_agent(project, agent, cx);
        if agent.status.is_finished() || provider == ProjectAgentRuntime::Waiting {
            return provider;
        }
        match self.delegation_activity_for(agent.id, cx) {
            DelegationActivity::Attention => ProjectAgentRuntime::Waiting,
            DelegationActivity::Working => ProjectAgentRuntime::Working,
            DelegationActivity::Paused | DelegationActivity::Idle => provider,
        }
    }

    fn provider_runtime_for_agent(
        &self,
        project: ProjectId,
        agent: &AgentRecord,
        cx: &App,
    ) -> ProjectAgentRuntime {
        if agent.status.is_finished() {
            return ProjectAgentRuntime::Idle;
        }
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

    /// Selecting a project restores its previous activity through CenterArea's
    /// workspace observer. Creating an agent remains an explicit row action.
    /// Clicking the project that is already active only folds its agent list.
    fn select_or_toggle_project(
        &mut self,
        project: ProjectId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_list_view.active_work {
            self.agent_list_view.toggle_project(project);
            if self.workspace.read(cx).active != Some(project) {
                self.workspace
                    .update(cx, |workspace, cx| workspace.set_active(project, cx));
            }
            cx.notify();
            return;
        }
        if self.workspace.read(cx).active == Some(project) {
            self.workspace.update(cx, |workspace, cx| {
                workspace.toggle_project_expanded(project, cx)
            });
            return;
        }
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
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
        let hover_key = HoveredAgentRow::Project(agent_id);
        let hovered = self.hovered_agent == Some(hover_key);
        let pinned = self.workspace.read(cx).is_agent_pinned(agent_id);
        let delegation = self.delegation_state_for(agent_id, cx);
        let has_delegations = !delegation.tasks.is_empty();
        let delegations_expanded = has_delegations && self.expanded_delegations.contains(&agent_id);

        let row = h_flex()
            .id(("project-agent-row", agent_id.as_u128() as u64))
            .child(self.agent_hover_anchor(hover_key, cx))
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
            .on_hover(cx.listener(move |this, is_hovered, window, cx| {
                this.set_agent_card_hover(hover_key, agent_id, *is_hovered, window, cx);
                this.hovered_agent = if *is_hovered {
                    Some(hover_key)
                } else if this.hovered_agent == Some(hover_key) {
                    None
                } else {
                    this.hovered_agent
                };
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_agent(project, agent_id, window, cx);
            }))
            .when(
                agent
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.is_pocketcomet()),
                |row| {
                    row.child(crate::ui::design::indicator::pocketcomet_icon(
                        crate::ui::design::accent(cx),
                        crate::ui::design::icon_sm(),
                    ))
                },
            )
            // A Solo wears its fork before the name, sky-marked like everywhere.
            .when(agent.is_active_solo(), |row| {
                row.child(crate::ui::design::indicator::solo_icon(
                    crate::ui::design::sky(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            // Sidebar agent row (`.agent`): 13px, muted `t2` by default so the
            // list reads calm; only the selected/waiting row lifts to `t1`.
            .child(self.render_agent_title(
                agent,
                hovered,
                if waiting {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                },
                if waiting {
                    warning
                } else if selected {
                    crate::ui::design::t1(cx)
                } else {
                    crate::ui::design::t2(cx)
                },
                cx,
            ))
            .when(hovered, |row| {
                row.child(self.render_agent_actions(agent_id, pinned, cx))
            })
            .when(has_delegations, |row| {
                row.child(self.render_delegation_toggle(
                    agent_id,
                    &delegation,
                    runtime == ProjectAgentRuntime::Working,
                    hovered,
                    cx,
                ))
            })
            .when(
                !has_delegations && !hovered && runtime == ProjectAgentRuntime::Working,
                |row| {
                    row.child(div().flex_none().w(px(34.)).flex().justify_end().child(
                        logo_spinner(
                            16.,
                            "project-agent-logo",
                            row_ix * 1000 + agent_ix,
                            crate::ui::design::t3(cx),
                        ),
                    ))
                },
            )
            .when(!has_delegations && !hovered && waiting, |row| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(34.))
                        .flex()
                        .justify_end()
                        .items_center()
                        .child(div().size(px(6.)).rounded_full().bg(warning)),
                )
            });
        if !delegations_expanded {
            return row.into_any_element();
        }
        v_flex()
            .w_full()
            .gap_0p5()
            .child(row)
            .child(self.render_delegated_task_rows(project, agent_id, px(34.), &delegation, cx))
            .into_any_element()
    }

    fn render_agent_actions(
        &self,
        agent_id: Uuid,
        pinned: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        h_flex()
            .flex_none()
            .w(px(42.))
            .gap_0p5()
            .items_center()
            .justify_end()
            .child(
                style::sidebar_agent_action_button(
                    ("sidebar-agent-pin", agent_id.as_u128() as u64),
                    lucide_icons::Icon::Pin,
                    if pinned {
                        crate::ui::design::accent(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    },
                    cx,
                )
                .tooltip(if pinned { "Unpin agent" } else { "Pin agent" })
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.set_agent_pinned(agent_id, !pinned, cx);
                    });
                })),
            )
            .child(
                style::sidebar_agent_action_button(
                    ("sidebar-agent-complete", agent_id.as_u128() as u64),
                    lucide_icons::Icon::Check,
                    crate::ui::design::sage(cx),
                    cx,
                )
                .tooltip("Mark complete")
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.workspace.update(cx, |workspace, cx| {
                        workspace.set_agent_pinned(agent_id, false, cx);
                    });
                    // This is deliberately the same status transition used by
                    // the agent's Done controls. CenterArea observes it and
                    // requests the terminal brain summary for chat agents.
                    this.agents.update(cx, |agents, cx| {
                        agents.update_status(agent_id, AgentStatus::Done, cx);
                    });
                })),
            )
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
                if agent.status.is_finished() {
                    continue;
                }
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

    fn collect_pinned_agents(
        &self,
        cx: &App,
    ) -> Vec<(ProjectId, SharedString, String, AgentRecord)> {
        let state = self.workspace.read(cx);
        let mut pinned = Vec::new();
        for project in &state.projects {
            let project_name = SharedString::from(project.name.clone());
            for agent in self.agents.read(cx).records_for_project(project.id) {
                if agent.status.is_finished()
                    || !state.is_agent_pinned(agent.id)
                    || self.runtime_for_agent(project.id, &agent, cx)
                        == ProjectAgentRuntime::Waiting
                {
                    continue;
                }
                pinned.push((
                    project.id,
                    project_name.clone(),
                    project.icon.clone(),
                    agent,
                ));
            }
        }
        pinned.sort_by_key(|(_, _, _, agent)| std::cmp::Reverse(agent.updated_at));
        pinned
    }

    fn render_pinned_agent(
        &self,
        ix: usize,
        project: ProjectId,
        project_name: SharedString,
        project_icon_id: &str,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let runtime = self.runtime_for_agent(project, agent, cx);
        let active_project = self.workspace.read(cx).active;
        let selected = active_project == Some(project)
            && self.agents.read(cx).explicitly_selected_agent_id(project) == Some(agent_id);
        let hover_key = HoveredAgentRow::Pinned(agent_id);
        let hovered = self.hovered_agent == Some(hover_key);
        let custom_svg_path = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|candidate| candidate.id == project)
            .and_then(|candidate| candidate.icon_image_path.clone());

        let delegation = self.delegation_state_for(agent_id, cx);
        let has_delegations = !delegation.tasks.is_empty();
        let expanded = self.expanded_delegations.contains(&agent_id);
        let row = h_flex()
            .id(("pinned-agent-row", agent_id.as_u128() as u64))
            .child(self.agent_hover_anchor(hover_key, cx))
            .w_full()
            .min_h(px(30.))
            .pl_3()
            .pr_2()
            .py_1()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .when(selected, |row| row.bg(crate::ui::design::surface_2(cx)))
            .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.5)))
            .on_hover(cx.listener(move |this, is_hovered, window, cx| {
                this.set_agent_card_hover(hover_key, agent_id, *is_hovered, window, cx);
                this.hovered_agent = if *is_hovered {
                    Some(hover_key)
                } else if this.hovered_agent == Some(hover_key) {
                    None
                } else {
                    this.hovered_agent
                };
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.open_agent(project, agent_id, window, cx);
            }))
            .when(agent.is_active_solo(), |row| {
                row.child(crate::ui::design::indicator::solo_icon(
                    crate::ui::design::sky(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .child(self.render_agent_title(
                agent,
                hovered,
                FontWeight::NORMAL,
                if selected {
                    crate::ui::design::t1(cx)
                } else {
                    crate::ui::design::t2(cx)
                },
                cx,
            ))
            .when(hovered, |row| {
                row.child(self.render_agent_actions(agent_id, true, cx))
            })
            .when(has_delegations, |row| {
                row.child(self.render_delegation_toggle(
                    agent_id,
                    &delegation,
                    runtime == ProjectAgentRuntime::Working,
                    hovered,
                    cx,
                ))
            })
            .when(
                !has_delegations && !hovered && runtime == ProjectAgentRuntime::Working,
                |row| {
                    row.child(div().flex_none().w(px(34.)).flex().justify_end().child(
                        logo_spinner(16., "pinned-agent-logo", ix, crate::ui::design::t3(cx)),
                    ))
                },
            )
            .when(!hovered, |row| {
                row.child(
                    div()
                        .id(("pinned-agent-project", agent_id.as_u128() as u64))
                        .flex_none()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(move |window, cx| {
                            Tooltip::new(project_name.clone()).build(window, cx)
                        })
                        .child(project_icon_visual_glyph(
                            project_icon_id,
                            custom_svg_path.as_deref(),
                            crate::ui::design::t3(cx),
                            px(13.),
                        )),
                )
            })
            .into_any_element();
        v_flex()
            .w_full()
            .child(row)
            .when(expanded && !delegation.tasks.is_empty(), |col| {
                col.child(self.render_delegated_task_rows(
                    project,
                    agent_id,
                    px(12.),
                    &delegation,
                    cx,
                ))
            })
            .into_any_element()
    }

    fn render_pinned_section(&self, cx: &mut Context<Self>) -> Option<gpui::AnyElement> {
        let pinned = self.collect_pinned_agents(cx);
        if pinned.is_empty() {
            return None;
        }
        let collapsed = self.workspace.read(cx).pinned_agents_collapsed;

        Some(
            v_flex()
                .w_full()
                .gap_0p5()
                .mb_3()
                .child(
                    h_flex()
                        .id("pinned-agents-section-header")
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
                                workspace.toggle_pinned_agents_collapsed(cx);
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
                                .text_size(crate::ui::design::text_label())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t4(cx))
                                .child("PINNED"),
                        ),
                )
                .when(!collapsed, |section| {
                    section.children(
                        pinned
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (project, project_name, project_icon_id, agent))| {
                                self.render_pinned_agent(
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
        _ix: usize,
        project: ProjectId,
        project_name: SharedString,
        project_icon_id: &str,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let warning = crate::ui::design::amber(cx);
        let selected = self.attention_pinned == Some(agent_id);
        let hover_key = HoveredAgentRow::Attention(agent_id);
        let hovered = self.hovered_agent == Some(hover_key);
        let pinned = self.workspace.read(cx).is_agent_pinned(agent_id);
        let custom_svg_path = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|candidate| candidate.id == project)
            .and_then(|candidate| candidate.icon_image_path.clone());

        h_flex()
            .id(("attention-agent-row", agent_id.as_u128() as u64))
            .child(self.agent_hover_anchor(hover_key, cx))
            .w_full()
            .min_h(px(30.))
            .pl_3()
            .pr_2()
            .py_1()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .when(selected, |row| row.bg(crate::ui::design::surface_2(cx)))
            .hover(|row| row.bg(crate::ui::design::surface_2(cx).opacity(0.5)))
            .on_hover(cx.listener(move |this, hovered, window, cx| {
                this.set_agent_card_hover(hover_key, agent_id, *hovered, window, cx);
                this.hovered_agent = if *hovered {
                    Some(hover_key)
                } else if this.hovered_agent == Some(hover_key) {
                    None
                } else {
                    this.hovered_agent
                };
                cx.notify();
            }))
            .on_click(cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                this.attention_pinned = Some(agent_id);
                this.open_agent(project, agent_id, window, cx);
            }))
            .when(agent.is_active_solo(), |row| {
                row.child(crate::ui::design::indicator::solo_icon(
                    crate::ui::design::sky(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .child(self.render_agent_title(agent, hovered, FontWeight::MEDIUM, warning, cx))
            .when(hovered, |row| {
                row.child(self.render_agent_actions(agent_id, pinned, cx))
            })
            .when(!hovered, |row| {
                row.child(
                    div()
                        .id(("attention-agent-project", agent_id.as_u128() as u64))
                        .flex_none()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .tooltip(move |window, cx| {
                            Tooltip::new(project_name.clone()).build(window, cx)
                        })
                        .child(project_icon_visual_glyph(
                            project_icon_id,
                            custom_svg_path.as_deref(),
                            warning,
                            px(13.),
                        )),
                )
            })
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
        let current_icon_image_path = row.icon_image_path.clone();
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
                                current_icon_image_path.clone(),
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
        after_rows: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let collapsed = section.collapsed;
        v_flex()
            .id(("project-sidebar-section", section.ix))
            .w_full()
            .gap_0p5()
            .when(section.ix > 0 && after_rows, |section| section.mt_4())
            .when(section.ix > 0 && !after_rows, |section| section.mt_1())
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

    fn render_script_indicators(
        &self,
        row_ix: usize,
        scripts: Vec<SharedString>,
        cx: &App,
    ) -> impl IntoElement {
        // Preserve real scripts first, followed by sky-marked Solo runs.
        let (solos, real): (Vec<_>, Vec<_>) = scripts.into_iter().partition(|script| {
            crate::state::terminals::solo_script_slug(script.as_ref()).is_some()
        });
        let scripts: Vec<(SharedString, bool)> = real
            .into_iter()
            .map(|name| (name, false))
            .chain(solos.into_iter().map(|script| {
                let slug =
                    crate::state::terminals::solo_script_slug(script.as_ref()).unwrap_or_default();
                (SharedString::from(slug.to_string()), true)
            }))
            .collect();
        let visible = visible_sidebar_script_count(&scripts, self.workspace.read(cx).panels.left);
        let hidden = scripts.len() - visible;

        h_flex()
            .w_full()
            .min_w(px(0.))
            .h(px(20.))
            .gap(px(4.))
            .overflow_hidden()
            .children(
                scripts
                    .iter()
                    .take(visible)
                    .enumerate()
                    .map(|(ix, (name, solo))| {
                        let full_name = if *solo {
                            SharedString::from(format!("Solo: {name}"))
                        } else {
                            name.clone()
                        };
                        style::sidebar_script_chip(name.clone(), *solo, cx)
                            .id(("project-script", ix))
                            .tooltip(move |window, cx| {
                                Tooltip::new(full_name.clone()).build(window, cx)
                            })
                    }),
            )
            .when(hidden > 0, |row| {
                row.child(
                    style::sidebar_script_overflow(hidden, cx)
                        .id(("project-script-overflow", row_ix))
                        .tooltip(move |window, cx| {
                            let scripts = scripts.clone();
                            Tooltip::element(move |_, cx| {
                                v_flex()
                                    .max_w(px(320.))
                                    .py_1()
                                    .gap_1()
                                    .text_size(crate::ui::design::text_label())
                                    .child(
                                        div()
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(format!("{} running scripts", scripts.len())),
                                    )
                                    .children(scripts.iter().map(|(name, solo)| {
                                        let color = if *solo {
                                            crate::ui::design::sky(cx)
                                        } else {
                                            crate::ui::design::sage(cx)
                                        };
                                        h_flex()
                                            .gap_1p5()
                                            .items_center()
                                            .child(
                                                div()
                                                    .size(px(5.))
                                                    .flex_none()
                                                    .rounded_full()
                                                    .bg(color),
                                            )
                                            .child(div().min_w(px(0.)).whitespace_normal().child(
                                                if *solo {
                                                    SharedString::from(format!("Solo: {name}"))
                                                } else {
                                                    name.clone()
                                                },
                                            ))
                                    }))
                            })
                            .build(window, cx)
                        }),
                )
            })
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
        let collapsed = if self.agent_list_view.active_work {
            self.agent_list_view.expanded_project != Some(id)
        } else {
            !self.workspace.read(cx).expanded_projects.contains(&id)
        };
        let hovered = self.hovered_project == Some(id);
        let show_actions = hovered || row.is_active || self.menu_project == Some(id);
        let has_in_progress_agents = !row.in_progress_agents.is_empty();
        let agents_section =
            self.render_project_agents_section(row.ix, id, &row.in_progress_agents, cx);

        v_flex()
            .id(("project-row", row.ix))
            .w_full()
            .gap_0p5()
            .child(
                v_flex()
                    .id(("project-row-card", row.ix))
                    .w_full()
                    .px_3()
                    // Every project is one 22px line, open or closed: the card
                    // stays snug around it and the agent list below brings its
                    // own indent and rhythm.
                    .py(px(4.))
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
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.select_or_toggle_project(id, window, cx);
                                    }))
                                    .child(project_icon_element(
                                        &row.icon,
                                        &row.icon_color,
                                        row.icon_image_path.as_deref(),
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
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.select_or_toggle_project(id, window, cx);
                                    }))
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            // Sidebar project row (`.proj`): 13px, medium.
                                            .text_size(crate::ui::design::text_head())
                                            .font_weight(FontWeight::MEDIUM)
                                            // Only the active project holds full
                                            // strength; the rest rest one step
                                            // down, still above their agents.
                                            .text_color(if row.is_active {
                                                crate::ui::design::t1(cx)
                                            } else {
                                                crate::ui::design::t1_soft(cx)
                                            })
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
                                            .tooltip(if self.agent_list_view.active_work {
                                                if collapsed {
                                                    "Show all agents in project"
                                                } else {
                                                    "Show active work only"
                                                }
                                            } else if collapsed {
                                                "Expand project"
                                            } else {
                                                "Collapse project"
                                            })
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    cx.stop_propagation();
                                                    if this.agent_list_view.active_work {
                                                        this.agent_list_view.toggle_project(id);
                                                        cx.notify();
                                                    } else {
                                                        this.workspace.update(
                                                            cx,
                                                            |workspace, cx| {
                                                                workspace.toggle_project_expanded(
                                                                    id, cx,
                                                                );
                                                            },
                                                        );
                                                    }
                                                }),
                                            ),
                                        )
                                    }),
                            )
                            .when(
                                !self.agent_list_view.active_work
                                    && collapsed
                                    && (row.agents_working || row.agents_waiting > 0),
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
                        card.child(self.render_script_indicators(row.ix, scripts, cx))
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
            .when(
                (!collapsed || self.agent_list_view.active_work) && has_in_progress_agents,
                |column| column.child(agents_section),
            )
    }
}

impl Render for ProjectList {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.agent_hover.begin_render();
        // The Active/All choice lives in Workspace (persisted, switched from the
        // sidebar footer). Mirror it before collecting rows so the filter agrees.
        self.agent_list_view.active_work = self.workspace.read(cx).sidebar_active_work;
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
        let pinned_section = self.render_pinned_section(cx);
        // The 16px section break exists to close a list of projects. A header
        // that follows a closed (or empty) section has no list to close, so it
        // stacks at row rhythm instead of floating in its own band.
        let mut previous_section_had_rows = false;
        let section_elements = sections
            .into_iter()
            .map(|section| {
                let had_rows = !section.collapsed && !section.rows.is_empty();
                let element = self.render_sidebar_section(section, previous_section_had_rows, cx);
                previous_section_had_rows = had_rows;
                element
            })
            .collect::<Vec<_>>();

        self.agent_hover.finish_render();

        v_flex()
            .child(self.agent_sidebar_bounds(cx))
            .size_full()
            .px_2()
            .py_2()
            .gap_1p5()
            .child(
                v_flex()
                    .id("project-rows")
                    .flex_1()
                    .min_h(px(0.))
                    .gap_0p5()
                    .overflow_y_scroll()
                    .children(attention_section)
                    .children(pinned_section)
                    .children(section_elements)
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

#[cfg(test)]
mod sidebar_view_tests {
    use super::*;

    fn script_names(names: &[&str]) -> Vec<(SharedString, bool)> {
        names
            .iter()
            .map(|name| (SharedString::from((*name).to_string()), false))
            .collect()
    }

    #[test]
    fn sidebar_scripts_collapse_only_the_items_that_do_not_fit() {
        let scripts = script_names(&["API Server", "Web Dev", "Web Dev", "Web Actions"]);
        assert_eq!(visible_sidebar_script_count(&scripts, 320.), 3);
        assert_eq!(visible_sidebar_script_count(&scripts, 500.), 4);
    }

    #[test]
    fn sidebar_script_width_caps_long_names() {
        assert_eq!(style::sidebar_script_chip_width(&"x".repeat(100)), 96.);
    }

    #[test]
    fn sidebar_active_work_keeps_work_visible_when_expanding_another_project() {
        let a = ProjectId::new();
        let b = ProjectId::new();
        let mut view = AgentListView {
            active_work: true,
            ..Default::default()
        };
        view.toggle_project(a);
        assert!(view.includes(a, AgentStatus::InProgress, ProjectAgentRuntime::Idle));
        assert!(!view.includes(b, AgentStatus::InProgress, ProjectAgentRuntime::Idle));
        view.toggle_project(b);
        assert!(!view.includes(a, AgentStatus::InProgress, ProjectAgentRuntime::Idle));
        assert!(view.includes(a, AgentStatus::InProgress, ProjectAgentRuntime::Working));
        assert!(view.includes(a, AgentStatus::InProgress, ProjectAgentRuntime::Waiting));
        assert!(view.includes(b, AgentStatus::InProgress, ProjectAgentRuntime::Idle));
        view.toggle_project(b);
        assert!(!view.includes(b, AgentStatus::InProgress, ProjectAgentRuntime::Idle));
        assert!(view.includes(b, AgentStatus::InProgress, ProjectAgentRuntime::Working));
    }

    #[test]
    fn sidebar_active_work_filters_runtime_without_changing_the_all_agents_lane() {
        let project = ProjectId::new();
        let mut view = AgentListView {
            active_work: true,
            ..Default::default()
        };
        for runtime in [
            ProjectAgentRuntime::NotStarted,
            ProjectAgentRuntime::Open,
            ProjectAgentRuntime::Idle,
            ProjectAgentRuntime::Ended,
        ] {
            assert!(!view.includes(project, AgentStatus::InProgress, runtime));
        }
        // Runtime activity still counts if the manual board lane is different.
        assert!(view.includes(project, AgentStatus::Todo, ProjectAgentRuntime::Working));
        assert!(view.includes(project, AgentStatus::Todo, ProjectAgentRuntime::Waiting));
        view.toggle_project(project);
        assert!(!view.includes(project, AgentStatus::Done, ProjectAgentRuntime::Working));
        assert!(!view.includes(project, AgentStatus::Rejected, ProjectAgentRuntime::Waiting));
        view.active_work = false;
        assert!(view.includes(project, AgentStatus::InProgress, ProjectAgentRuntime::Idle));
        assert!(!view.includes(project, AgentStatus::Todo, ProjectAgentRuntime::Working));
    }
}
