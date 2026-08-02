use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, IntoElement, ParentElement,
    Render, Styled, Window,
};
use gpui_component::{
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex, IconName,
};

use crate::ui::agents_panel::AgentsPanel;
use crate::ui::center::{CenterArea, ProjectActivity};
use crate::ui::db::db_panel::DbPanel;
use crate::ui::designs_panel::DesignsPanel;
use crate::ui::docs_panel::DocsPanel;
use crate::ui::files::file_tree::FileTree;
use crate::ui::git::git_panel::{GitPanel, GitTab};
use crate::ui::penpot_panel::PenpotPanel;
use crate::ui::services_panel::ServicesPanel;
use crate::ui::style;
use crate::ui::tasks_panel::TasksPanel;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RightToolTab {
    Git,
    Files,
    Agents,
}

impl RightToolTab {
    #[allow(
        dead_code,
        reason = "stable tab identifiers are retained for future keyed rendering"
    )]
    fn element_id(self) -> usize {
        match self {
            RightToolTab::Git => 0,
            RightToolTab::Files => 1,
            RightToolTab::Agents => 2,
        }
    }
}

/// Right panel: contextual tools for the selected project activity.
pub struct RightPanel {
    center: Entity<CenterArea>,
    git_panel: Entity<GitPanel>,
    file_tree: Entity<FileTree>,
    agents_panel: Entity<AgentsPanel>,
    db_panel: Entity<DbPanel>,
    docs_panel: Entity<DocsPanel>,
    designs_panel: Entity<DesignsPanel>,
    tasks_panel: Entity<TasksPanel>,
    services_panel: Entity<ServicesPanel>,
    penpot_panel: Entity<PenpotPanel>,
    selected: RightToolTab,
    last_activity: ProjectActivity,
    last_agent_open_epoch: u64,
}

impl RightPanel {
    pub fn view(
        git_panel: Entity<GitPanel>,
        file_tree: Entity<FileTree>,
        agents_panel: Entity<AgentsPanel>,
        db_panel: Entity<DbPanel>,
        docs_panel: Entity<DocsPanel>,
        designs_panel: Entity<DesignsPanel>,
        tasks_panel: Entity<TasksPanel>,
        services_panel: Entity<ServicesPanel>,
        penpot_panel: Entity<PenpotPanel>,
        center: Entity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            // The Agents tab badge tracks the panel's attention count.
            cx.observe(&agents_panel, |_, _, cx| cx.notify()).detach();
            cx.observe(&center, |_, _, cx| cx.notify()).detach();
            Self {
                center,
                git_panel,
                file_tree,
                agents_panel,
                db_panel,
                docs_panel,
                designs_panel,
                tasks_panel,
                services_panel,
                penpot_panel,
                // Open on Agents + Git: the center starts on the Agents view, so
                // seed `last_activity` to Agents too. That keeps the first render
                // from auto-switching the panel to the Board tab, leaving Git
                // (the default `selected`) showing on launch.
                selected: RightToolTab::Git,
                last_activity: ProjectActivity::Agents,
                last_agent_open_epoch: 0,
            }
        })
    }

    pub fn open_new_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = RightToolTab::Git;
        self.center.update(cx, |center, cx| center.show_agents(cx));
        self.agents_panel
            .update(cx, |panel, cx| panel.open_new_agent_dialog(window, cx));
        cx.notify();
    }

    pub fn show_git(&mut self, cx: &mut Context<Self>) {
        self.selected = RightToolTab::Git;
        cx.notify();
    }
}

impl Render for RightPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (activity, agent_open_epoch) = {
            let center = self.center.read(cx);
            (center.activity(), center.agent_open_epoch())
        };
        // Code always opens with Files so the primary way into the editor is
        // immediately available. Agents keeps Git as its default because that
        // view is where agent-authored changes are reviewed.
        let agents_requested =
            activity == ProjectActivity::Agents && agent_open_epoch != self.last_agent_open_epoch;
        if agents_requested {
            self.selected = RightToolTab::Git;
        } else if activity != self.last_activity {
            match activity {
                ProjectActivity::Code => self.selected = RightToolTab::Files,
                ProjectActivity::Agents => self.selected = RightToolTab::Git,
                _ => {}
            }
            self.last_activity = activity;
        }
        self.last_agent_open_epoch = agent_open_epoch;

        // Docs and Designs are now first-class activities with their own panel —
        // no in-panel toggle.
        match activity {
            ProjectActivity::Docs => {
                return div()
                    .size_full()
                    .child(self.docs_panel.clone())
                    .into_any_element();
            }
            ProjectActivity::Designs => {
                return div()
                    .size_full()
                    .child(self.designs_panel.clone())
                    .into_any_element();
            }
            ProjectActivity::Design => {
                return div()
                    .size_full()
                    .child(self.penpot_panel.clone())
                    .into_any_element();
            }
            ProjectActivity::Db => {
                return div()
                    .size_full()
                    .child(self.db_panel.clone())
                    .into_any_element();
            }
            ProjectActivity::Tasks => {
                return div()
                    .size_full()
                    .child(self.tasks_panel.clone())
                    .into_any_element();
            }
            ProjectActivity::Services => {
                return div()
                    .size_full()
                    .child(self.services_panel.clone())
                    .into_any_element();
            }
            ProjectActivity::Code | ProjectActivity::Agents => {}
        }

        let selected = match (activity, self.selected) {
            (ProjectActivity::Code, RightToolTab::Agents) => RightToolTab::Files,
            (ProjectActivity::Agents, RightToolTab::Files) => RightToolTab::Git,
            (_, tab) => tab,
        };

        // The panel names its current view and offers ONE flip to the other view
        // (Agents↔Board · Code↔Files) — never a two-way toggle. (Design law: "you
        // never show a toggle; you have a button that flips it.")
        let (cur_label, cur_icon) = match selected {
            RightToolTab::Files => ("Files", Some(IconName::FolderOpen)),
            RightToolTab::Agents => ("Board", None),
            RightToolTab::Git => ("Git", None),
        };
        let (flip_to, flip_label, flip_icon) = match (activity, selected) {
            (ProjectActivity::Agents, RightToolTab::Git) => {
                (RightToolTab::Agents, "Board", IconName::Bot)
            }
            (ProjectActivity::Code, RightToolTab::Git) => {
                (RightToolTab::Files, "Files", IconName::FolderOpen)
            }
            _ => (RightToolTab::Git, "Git", IconName::GitHub),
        };

        // Changes owns almost all Git-panel usage, so one compact view selector
        // replaces three permanently visible tabs. The active view stays clear
        // while Commits and PRs remain one click away; Board remains pinned right.
        let git_tab = self.git_panel.read(cx).active_tab();
        let git_tab_label = match git_tab {
            GitTab::Changes => "Changes",
            GitTab::Commits => "Commits",
            GitTab::PullRequests => "PRs",
        };
        let changes_panel = self.git_panel.clone();
        let commits_panel = self.git_panel.clone();
        let prs_panel = self.git_panel.clone();
        let git_view_selector = style::header_dropdown_button("git-view-selector", cx)
            .label(git_tab_label)
            .text_size(crate::ui::design::text_ui())
            .dropdown_menu(move |menu, _, _| {
                let changes_panel = changes_panel.clone();
                let commits_panel = commits_panel.clone();
                let prs_panel = prs_panel.clone();
                menu.item(
                    PopupMenuItem::new("Changes")
                        .checked(git_tab == GitTab::Changes)
                        .on_click(move |_, _, cx| {
                            changes_panel
                                .update(cx, |panel, cx| panel.set_active_tab(GitTab::Changes, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new("Commits")
                        .checked(git_tab == GitTab::Commits)
                        .on_click(move |_, _, cx| {
                            commits_panel
                                .update(cx, |panel, cx| panel.set_active_tab(GitTab::Commits, cx));
                        }),
                )
                .item(
                    PopupMenuItem::new("Pull requests")
                        .checked(git_tab == GitTab::PullRequests)
                        .on_click(move |_, _, cx| {
                            prs_panel.update(cx, |panel, cx| {
                                panel.set_active_tab(GitTab::PullRequests, cx)
                            });
                        }),
                )
            });
        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(crate::ui::design::header::panel_identity(
                        cur_icon, cur_label, cx,
                    ))
                    .when(selected == RightToolTab::Git, |row| {
                        row.child(
                            div()
                                .ml(crate::ui::design::panel_identity_tabs_gap())
                                .child(git_view_selector),
                        )
                    })
                    .child(div().flex_1().min_w(crate::ui::design::panel_action_gap()))
                    .child(
                        style::ghost_button_compact(("right-panel-flip", 0u64), flip_label)
                            .flex_none()
                            .icon(flip_icon)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.selected = flip_to;
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex_1().min_h(px(0.)).child(match selected {
                RightToolTab::Git => self.git_panel.clone().into_any_element(),
                RightToolTab::Files => self.file_tree.clone().into_any_element(),
                RightToolTab::Agents => self.agents_panel.clone().into_any_element(),
            }))
            .into_any_element()
    }
}
