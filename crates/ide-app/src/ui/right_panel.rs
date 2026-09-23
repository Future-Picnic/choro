use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, StatefulInteractiveElement, Styled, Window,
};
use gpui_component::{
    avatar::Avatar,
    menu::{DropdownMenu as _, PopupMenuItem},
    v_flex, Icon, IconName, Sizable,
};

use crate::ui::agents_panel::AgentsPanel;
use crate::ui::center::{CenterArea, ProjectActivity};
use crate::ui::db::db_panel::DbPanel;
use crate::ui::designs_panel::DesignsPanel;
use crate::ui::docs_panel::DocsPanel;
use crate::ui::files::file_tree::FileTree;
use crate::ui::git::git_panel::{GitPanel, GitTab};
use crate::ui::services_panel::ServicesPanel;
use crate::ui::style;
use crate::ui::tasks_panel::TasksPanel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

fn git_account_selector_visible(selected: RightToolTab) -> bool {
    selected == RightToolTab::Git
}

fn right_tool_tab_after_navigation(
    selected: RightToolTab,
    previous_activity: ProjectActivity,
    activity: ProjectActivity,
    agents_panel_reset_requested: bool,
    git_diff_requested: bool,
) -> RightToolTab {
    if agents_panel_reset_requested || git_diff_requested {
        return RightToolTab::Git;
    }
    if activity == previous_activity {
        return selected;
    }
    match activity {
        ProjectActivity::Code => RightToolTab::Files,
        ProjectActivity::Agents => RightToolTab::Git,
        _ => selected,
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
    selected: RightToolTab,
    last_activity: ProjectActivity,
    last_agents_panel_reset_epoch: u64,
    last_git_diff_open_epoch: u64,
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
        center: Entity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            // The Agents tab badge tracks the panel's attention count.
            cx.observe(&agents_panel, |_, _, cx| cx.notify()).detach();
            cx.observe(&center, |_, _, cx| cx.notify()).detach();
            cx.observe(&git_panel, |_, _, cx| cx.notify()).detach();
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
                // Open on Agents + Git: the center starts on the Agents view, so
                // seed `last_activity` to Agents too. That keeps the first render
                // from auto-switching the panel to the Board tab, leaving Git
                // (the default `selected`) showing on launch.
                selected: RightToolTab::Git,
                last_activity: ProjectActivity::Agents,
                last_agents_panel_reset_epoch: 0,
                last_git_diff_open_epoch: 0,
            }
        })
    }

    pub(crate) fn git_change_count(&self, cx: &App) -> Option<usize> {
        self.git_panel.read(cx).change_count(cx)
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
        let (activity, agents_panel_reset_epoch, git_diff_open_epoch) = {
            let center = self.center.read(cx);
            (
                center.activity(),
                center.agents_panel_reset_epoch(),
                center.git_diff_open_epoch(),
            )
        };
        // Code normally opens with Files. A diff chosen from Git is the one
        // exception: keep Git visible so the user can continue reviewing the
        // change list. Agents keeps Git as its default.
        let agents_panel_reset_requested = activity == ProjectActivity::Agents
            && agents_panel_reset_epoch != self.last_agents_panel_reset_epoch;
        let git_diff_requested = activity == ProjectActivity::Code
            && git_diff_open_epoch != self.last_git_diff_open_epoch;
        self.selected = right_tool_tab_after_navigation(
            self.selected,
            self.last_activity,
            activity,
            agents_panel_reset_requested,
            git_diff_requested,
        );
        self.last_activity = activity;
        self.last_agents_panel_reset_epoch = agents_panel_reset_epoch;
        self.last_git_diff_open_epoch = git_diff_open_epoch;

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
                    .child(self.designs_panel.clone())
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
            ProjectActivity::Code | ProjectActivity::Agents | ProjectActivity::PocketComet => {}
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
            GitTab::Workflows => "Workflows",
        };
        let changes_panel = self.git_panel.clone();
        let commits_panel = self.git_panel.clone();
        let prs_panel = self.git_panel.clone();
        let workflows_panel = self.git_panel.clone();
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
                .item(
                    PopupMenuItem::new("Workflows")
                        .checked(git_tab == GitTab::Workflows)
                        .on_click({
                            let workflows_panel = workflows_panel.clone();
                            move |_, _, cx| {
                                workflows_panel.update(cx, |panel, cx| {
                                    panel.set_active_tab(GitTab::Workflows, cx)
                                });
                            }
                        }),
                )
            });
        // Both read the GitState snapshot's cached values — render must never
        // ask git directly (per-frame subprocesses once tanked scrolling).
        let git_account_remote = self.git_panel.read(cx).git_account_remote(cx);
        let git_accounts = self.git_panel.read(cx).connected_git_accounts();
        let selected_git_account = self.git_panel.read(cx).selected_git_account(cx);
        let git_accounts_loading = self.git_panel.read(cx).git_accounts_loading();
        let git_accounts_error = self.git_panel.read(cx).git_accounts_error();
        let git_repository_error = self.git_panel.read(cx).git_repository_error(cx);
        let git_account_remote =
            git_account_remote.filter(ide_core::git::GitRemote::is_github_https);
        let git_account_selector = git_account_selector_visible(selected).then(|| {
            let account_switching_available = git_account_remote.is_some();
            let account_avatar = selected_git_account
                .clone()
                .map(|account| Avatar::new().name(account).xsmall().into_any_element())
                .unwrap_or_else(|| {
                    Icon::new(IconName::GitHub)
                        .size(crate::ui::design::icon())
                        .text_color(if git_repository_error.is_some() {
                            crate::ui::design::rose(cx)
                        } else {
                            crate::ui::design::t3(cx)
                        })
                        .into_any_element()
                });
            let tooltip = match (
                &selected_git_account,
                &git_accounts_error,
                &git_account_remote,
                &git_repository_error,
            ) {
                (_, _, _, Some(error)) => {
                    format!("GitHub account unavailable · {error}")
                }
                (_, _, None, None) => {
                    "GitHub account switching is unavailable for this repository".to_string()
                }
                // A notice ("Finish connecting in Terminal…") must stay
                // visible even when an account is already selected.
                (Some(account), Some(notice), Some(_), None) => {
                    format!("GitHub account: @{account} · {notice}")
                }
                (Some(account), None, Some(remote), None) => format!(
                    "GitHub account: @{account} · {} · Click to switch",
                    remote.name
                ),
                (None, Some(error), Some(_), None) => {
                    format!("Choose a GitHub account · {error}")
                }
                (None, None, Some(_), None) => {
                    "Choose a GitHub account for this repository".to_string()
                }
            };
            let panel = self.git_panel.clone();
            style::header_svg_button("git-account-selector", account_avatar, cx)
                .tooltip(tooltip)
                .dropdown_menu(move |menu, _, _| {
                    let mut menu = git_accounts.iter().fold(menu, |menu, account| {
                        let login = account.login.clone();
                        let selected = selected_git_account.as_deref() == Some(login.as_str());
                        let panel = panel.clone();
                        menu.item(
                            PopupMenuItem::new(format!("@{login}"))
                                .checked(selected)
                                .disabled(!account_switching_available)
                                .on_click(move |_, _, cx| {
                                    panel.update(cx, |panel, cx| {
                                        panel.select_git_account(Some(login.clone()), cx)
                                    });
                                }),
                        )
                    });
                    if git_accounts.is_empty() {
                        let label = if git_accounts_loading {
                            "Loading connected accounts…"
                        } else {
                            "No connected GitHub accounts"
                        };
                        menu = menu.item(PopupMenuItem::new(label).disabled(true));
                    }
                    let system_panel = panel.clone();
                    let refresh_panel = panel.clone();
                    let connect_panel = panel.clone();
                    menu.separator()
                        .item(
                            PopupMenuItem::new("Use system Git credentials")
                                .checked(selected_git_account.is_none())
                                .disabled(!account_switching_available)
                                .on_click(move |_, _, cx| {
                                    system_panel
                                        .update(cx, |panel, cx| panel.select_git_account(None, cx));
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Refresh accounts").on_click(move |_, _, cx| {
                                refresh_panel
                                    .update(cx, |panel, cx| panel.refresh_git_accounts(cx));
                            }),
                        )
                        .item(
                            PopupMenuItem::new("Connect another GitHub account…").on_click(
                                move |_, _, cx| {
                                    connect_panel
                                        .update(cx, |panel, cx| panel.connect_git_account(cx));
                                },
                            ),
                        )
                })
        });
        v_flex()
            .size_full()
            .child(
                crate::ui::design::header::panel_bar(cx)
                    .child(
                        // Keep the collapse target anchored at the trailing
                        // edge even at the panel's minimum resize width. All
                        // existing controls remain available by scrolling.
                        gpui_component::h_flex()
                            .id("right-panel-header-controls")
                            .flex_1()
                            .min_w(px(0.))
                            .h_full()
                            .items_center()
                            .overflow_x_scroll()
                            .child(
                                gpui_component::h_flex()
                                    .flex_none()
                                    .items_center()
                                    .gap_1p5()
                                    .when_some(git_account_selector, |identity, selector| {
                                        identity.child(selector)
                                    })
                                    .child(crate::ui::design::header::panel_identity(
                                        cur_icon, cur_label, cx,
                                    )),
                            )
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
                    .child(style::right_sidebar_toggle(
                        true,
                        activity == ProjectActivity::Agents,
                        cx,
                    )),
            )
            .child(div().flex_1().min_h(px(0.)).child(match selected {
                RightToolTab::Git => self.git_panel.clone().into_any_element(),
                RightToolTab::Files => self.file_tree.clone().into_any_element(),
                RightToolTab::Agents => self.agents_panel.clone().into_any_element(),
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_account_selector_stays_visible_for_the_git_panel() {
        assert!(git_account_selector_visible(RightToolTab::Git));
        assert!(!git_account_selector_visible(RightToolTab::Files));
        assert!(!git_account_selector_visible(RightToolTab::Agents));
    }

    #[test]
    fn opening_an_agent_from_the_board_keeps_the_board_visible() {
        assert_eq!(
            right_tool_tab_after_navigation(
                RightToolTab::Agents,
                ProjectActivity::Agents,
                ProjectActivity::Agents,
                false,
                false,
            ),
            RightToolTab::Agents
        );
    }

    #[test]
    fn explicit_agents_navigation_restores_git() {
        assert_eq!(
            right_tool_tab_after_navigation(
                RightToolTab::Agents,
                ProjectActivity::Agents,
                ProjectActivity::Agents,
                true,
                false,
            ),
            RightToolTab::Git
        );
    }
}
