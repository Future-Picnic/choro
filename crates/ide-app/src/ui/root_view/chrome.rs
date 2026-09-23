use super::*;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_component::scroll::ScrollableElement;
use ide_core::ProjectActivityId;

const FEATURE_REQUESTS_URL: &str = "https://choro.usergist.com/choro/requests";
const SUPPORT_URL: &str = "https://choro.usergist.com/choro/support";
const ROADMAP_URL: &str = "https://choro.usergist.com/choro/roadmap";

enum RailIcon {
    Component(IconName),
    Lucide(lucide_icons::Icon),
    Tumble,
}

#[cfg(target_os = "macos")]
pub(super) fn pocketcomet_is_running() -> bool {
    use objc2_app_kit::NSRunningApplication;
    use objc2_foundation::NSString;

    // Support the current app and builds using its former DailyBob identity.
    ["com.futurepicnic.pocketcomet", "com.futurepicnic.dailybob"]
        .iter()
        .any(|bundle_id| {
            NSRunningApplication::runningApplicationsWithBundleIdentifier(&NSString::from_str(
                bundle_id,
            ))
            .iter()
            .any(|app| !app.isTerminated())
        })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn pocketcomet_is_running() -> bool {
    // Other platforms use the authenticated activity timeout.
    true
}

fn project_activity_label(activity: ProjectActivityId) -> &'static str {
    match activity {
        ProjectActivityId::Agents => "Agents",
        ProjectActivityId::Code => "Code",
        ProjectActivityId::Tasks => "Tasks",
        ProjectActivityId::Docs => "Docs",
        ProjectActivityId::Design => "Design",
        ProjectActivityId::Db => "DB",
        ProjectActivityId::Assets => "Assets",
        ProjectActivityId::Orbit => "Orbit",
        ProjectActivityId::Unknown => "Unknown",
    }
}

impl RootView {
    pub(super) fn update_dock_badge(&self, _cx: &mut Context<Self>) {
        let count = crate::notifications::unread_agent_count();
        let label = (count > 0).then(|| count.to_string());
        crate::notifications::set_dock_badge(label.as_deref());
    }

    pub(super) fn render_title_branch_picker(
        &self,
        project_id: ide_core::ProjectId,
        mut branches: Vec<BranchInfo>,
        left: gpui::Pixels,
        top: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let query = self.title_branch_query.read(cx).value().trim().to_string();
        if !query.is_empty() {
            let needle = query.to_lowercase();
            branches.retain(|branch| branch.name.to_lowercase().contains(&needle));
        }
        branches.sort_by(crate::ui::branch_order::compare_branches);
        branches.truncate(if query.is_empty() {
            TITLE_BRANCH_PICKER_LIMIT
        } else {
            20
        });

        v_flex()
            .absolute()
            .top(top)
            .left(left)
            .w(px(360.))
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .id("title-branch-picker-scroll")
                    .max_h(px(260.))
                    .overflow_y_scroll()
                    .p_1()
                    .gap_0p5()
                    .when(branches.is_empty(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No matching branches"),
                        )
                    })
                    .children(branches.into_iter().enumerate().map(|(ix, branch)| {
                        let name: SharedString = branch.name.clone().into();
                        let checkout_name = branch.name.clone();
                        let git_states = self.git_states.clone();
                        let ahead_behind: Option<SharedString> =
                            if branch.ahead > 0 || branch.behind > 0 {
                                Some(format!("↑{} ↓{}", branch.ahead, branch.behind).into())
                            } else {
                                None
                            };
                        let detail: SharedString = {
                            let mut parts: Vec<String> = Vec::new();
                            if !branch.tip_author.is_empty() {
                                parts.push(branch.tip_author.clone());
                            }
                            let when = branch_relative_time(branch.tip_time);
                            if !when.is_empty() {
                                parts.push(when);
                            }
                            if !branch.tip_summary.is_empty() {
                                parts.push(branch.tip_summary.clone());
                            }
                            parts.join(" · ").into()
                        };

                        h_flex()
                            .id(("title-branch-row", ix))
                            .w_full()
                            .px_2()
                            .py_0p5()
                            .gap_2()
                            .items_center()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .when(branch.is_head, |row| {
                                row.bg(crate::ui::design::surface_2(cx))
                            })
                            .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if !branch.is_head {
                                    if let Some(git) = git_states.read(cx).get(project_id) {
                                        git.update(cx, |git, cx| {
                                            git.checkout(checkout_name.clone(), cx);
                                        });
                                    }
                                }
                                this.title_branch_query
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.title_branch_expanded = false;
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .w(px(18.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(if branch.is_head {
                                        Icon::new(IconName::Check)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::accent(cx))
                                    } else if branch.is_remote {
                                        Icon::new(IconName::Globe)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t3(cx))
                                    } else {
                                        Icon::new(IconName::Replace)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t3(cx))
                                    }),
                            )
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .truncate()
                                            .child(name),
                                    )
                                    .when(!detail.is_empty(), |col| {
                                        col.child(
                                            div()
                                                .text_size(crate::ui::design::text_label())
                                                .text_color(crate::ui::design::t3(cx))
                                                .truncate()
                                                .child(detail),
                                        )
                                    }),
                            )
                            .when_some(ahead_behind, |row, label| {
                                row.child(
                                    div()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(label),
                                )
                            })
                    })),
            )
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.28))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .child(Input::new(&self.title_branch_query)),
                    )
                    .child(
                        Icon::new(IconName::SortDescending)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }

    pub(super) fn render_title_branch_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.title_branch_expanded {
            return None;
        }
        let (project_id, branches) = {
            let project_id = self.workspace.read(cx).active?;
            let git = self.git_states.read(cx).get(project_id)?;
            let git = git.read(cx);
            let branches = git
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.branches.clone())
                .unwrap_or_default();
            (project_id, branches)
        };
        let anchor = self.title_branch_bounds.clone()?;
        Some(
            div()
                .absolute()
                .top(px(0.))
                .left(px(0.))
                .size_full()
                .child(
                    div()
                        .id("title-branch-picker-backdrop")
                        .absolute()
                        .top(px(0.))
                        .left(px(0.))
                        .size_full()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.title_branch_expanded = false;
                            cx.notify();
                        })),
                )
                .child(self.render_title_branch_picker(
                    project_id,
                    branches,
                    anchor.origin.x,
                    anchor.origin.y + anchor.size.height + px(2.),
                    cx,
                ))
                .into_any_element(),
        )
    }

    /// Help and Settings pinned to the right rail footer.
    pub(super) fn rail_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let help = style::rail_footer_button("rail-help", IconName::Info, "Help", cx)
            .tooltip("Help")
            .dropdown_menu_with_anchor(gpui::Corner::TopRight, |menu, _, _| {
                menu.item(
                    PopupMenuItem::new("Send feedback / support")
                        .icon(IconName::Inbox)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url(SUPPORT_URL);
                        }),
                )
                .item(
                    PopupMenuItem::new("Request a feature")
                        .icon(IconName::Star)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url(FEATURE_REQUESTS_URL);
                        }),
                )
                .item(
                    PopupMenuItem::new("Roadmap")
                        .icon(IconName::Map)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url(ROADMAP_URL);
                        }),
                )
                .item(
                    PopupMenuItem::new("Tutorials")
                        .icon(IconName::BookOpen)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url("https://choro.dev");
                        }),
                )
            });

        let settings =
            style::rail_footer_button("rail-settings", IconName::Settings, "Settings", cx)
                .tooltip("Settings (⌘,)")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_settings(window, cx);
                }));

        v_flex()
            .flex_none()
            .w_full()
            .pb_2()
            .child(v_flex().w_full().child(help).child(settings))
    }

    /// Help and Settings controls anchored at the bottom of the right sidebar.
    pub(super) fn settings_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let help = style::sidebar_footer_button("right-sidebar-help", IconName::Info, "Help")
            .dropdown_menu_with_anchor(gpui::Corner::TopRight, |menu, _, _| {
                menu.item(
                    PopupMenuItem::new("Send feedback / support")
                        .icon(IconName::Inbox)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url(SUPPORT_URL);
                        }),
                )
                .item(
                    PopupMenuItem::new("Request a feature")
                        .icon(IconName::Star)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url(FEATURE_REQUESTS_URL);
                        }),
                )
                .item(
                    PopupMenuItem::new("Roadmap")
                        .icon(IconName::Map)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url(ROADMAP_URL);
                        }),
                )
                .item(
                    PopupMenuItem::new("Tutorials")
                        .icon(IconName::BookOpen)
                        .on_click(|_, _, _| {
                            crate::ui::git::git_panel::open_url("https://choro.dev");
                        }),
                )
            });

        h_flex()
            .h(px(44.))
            .w_full()
            .px_3()
            .gap_2()
            .items_center()
            .border_t_1()
            .border_color(style::hairline(cx))
            .child(help)
            .child(
                style::sidebar_footer_button(
                    "right-sidebar-settings",
                    IconName::Settings,
                    "Settings",
                )
                .tooltip("Settings (⌘,)")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.toggle_settings(window, cx);
                })),
            )
    }

    fn activity_customizer(
        &self,
        project_id: ide_core::ProjectId,
        pinned: Vec<ProjectActivityId>,
        current: ProjectActivity,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let workspace = self.workspace.clone();
        let center = self.center.clone();
        let has_override = workspace.read(cx).has_project_activity_override(project_id);
        let defaults = workspace.read(cx).default_project_activities.clone();
        let current_id = current.persisted_id();

        let trigger = style::rail_activity_menu_button(
            "rail-customize-activities",
            lucide_icons::Icon::Grip,
            "Manage",
            cx,
        )
        .tooltip("Choose activities shown in this rail")
        .dropdown_menu_with_anchor(gpui::Corner::TopRight, move |menu, _, _| {
            let menu = ProjectActivityId::ALL.iter().copied().fold(
                menu.min_w(px(220.))
                    .item(PopupMenuItem::label("PROJECT ACTIVITIES")),
                |menu, activity| {
                    let checked = pinned.contains(&activity);
                    let only_pinned = checked && pinned.len() == 1;
                    let fallback = pinned
                        .iter()
                        .copied()
                        .find(|candidate| *candidate != activity)
                        .and_then(ProjectActivity::from_persisted_id);
                    let workspace = workspace.clone();
                    let center = center.clone();
                    menu.item(
                        PopupMenuItem::new(project_activity_label(activity))
                            .checked(checked)
                            .disabled(only_pinned)
                            .on_click(move |_, _, cx| {
                                workspace.update(cx, |workspace, cx| {
                                    workspace.set_project_activity_pinned(
                                        project_id, activity, !checked, cx,
                                    );
                                });
                                if checked && current_id == Some(activity) {
                                    if let Some(fallback) = fallback {
                                        center.update(cx, |center, cx| {
                                            center.show_activity(fallback, cx)
                                        });
                                    }
                                }
                            }),
                    )
                },
            );

            let workspace = workspace.clone();
            let center = center.clone();
            let fallback = defaults
                .first()
                .copied()
                .and_then(ProjectActivity::from_persisted_id);
            let defaults_for_reset = defaults.clone();
            menu.item(PopupMenuItem::separator()).item(
                PopupMenuItem::new("Reset to default")
                    .icon(IconName::Undo2)
                    .disabled(!has_override)
                    .on_click(move |_, _, cx| {
                        workspace.update(cx, |workspace, cx| {
                            workspace.reset_project_activities(project_id, cx)
                        });
                        if current_id
                            .is_some_and(|activity| !defaults_for_reset.contains(&activity))
                        {
                            if let Some(fallback) = fallback {
                                center.update(cx, |center, cx| center.show_activity(fallback, cx));
                            }
                        }
                    }),
            )
        });

        div().w_full().flex_none().child(trigger)
    }

    /// The center's back/forward history controls. Rendered in the header when
    /// the sidebar is collapsed, and in the sidebar's top zone when it's open.
    pub(super) fn nav_history_buttons(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (can_go_back, can_go_forward) = {
            let center = self.center.read(cx);
            (center.can_go_back(), center.can_go_forward())
        };
        let center_for_back = self.center.clone();
        let center_for_forward = self.center.clone();
        h_flex()
            .flex_none()
            .gap_0p5()
            .items_center()
            .child(
                Button::new("title-nav-back")
                    .ghost()
                    .xsmall()
                    .compact()
                    .w(px(24.))
                    .h(crate::ui::design::control_h_xs())
                    .icon(IconName::ChevronLeft)
                    .disabled(!can_go_back)
                    .tooltip("Back")
                    .on_click(move |_, _, cx| {
                        center_for_back.update(cx, |center, cx| center.go_back(cx));
                    }),
            )
            .child(
                Button::new("title-nav-forward")
                    .ghost()
                    .xsmall()
                    .compact()
                    .w(px(24.))
                    .h(crate::ui::design::control_h_xs())
                    .icon(IconName::ChevronRight)
                    .disabled(!can_go_forward)
                    .tooltip("Forward")
                    .on_click(move |_, _, cx| {
                        center_for_forward.update(cx, |center, cx| center.go_forward(cx));
                    }),
            )
    }

    pub(super) fn project_branch_label(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let root_view = cx.entity().clone();
        let (project_visual, repository_context, branch) = {
            let ws = self.workspace.read(cx);
            let active_project = ws.active_project();
            let project_visual = active_project.map(|p| {
                (
                    p.name.clone(),
                    p.icon.clone(),
                    p.icon_color.clone(),
                    p.icon_image_path.clone(),
                )
            });
            let (repository_context, branch) = active_project
                .map(|project| {
                    let git_states = self.git_states.read(cx);
                    let active_repository = git_states.active_repository_path(project.id);
                    let repositories = git_states
                        .repositories(project.id)
                        .into_iter()
                        .filter_map(|git| {
                            let git = git.read(cx);
                            git.is_repo.then(|| {
                                let path = git.repo_path.clone();
                                let label = if path == project.path {
                                    project.name.clone()
                                } else {
                                    path.strip_prefix(&project.path)
                                        .unwrap_or(&path)
                                        .to_string_lossy()
                                        .into_owned()
                                };
                                let selected = active_repository.as_ref() == Some(&path);
                                (path, label, selected)
                            })
                        })
                        .collect::<Vec<_>>();
                    let repository_context =
                        (repositories.len() > 1).then_some((project.id, repositories));
                    let branch = git_states
                        .get(project.id)
                        .and_then(|git| git.read(cx).branch_label());
                    (repository_context, branch)
                })
                .unwrap_or((None, None));
            (project_visual, repository_context, branch)
        };
        h_flex()
            .flex_1()
            .min_w(px(0.))
            .gap_1p5()
            .items_center()
            // Back/forward navigate the center; when the sidebar is open they
            // live in its top zone instead (see `nav_history_buttons`).
            .when(!self.show_left, |bar| {
                bar.child(self.nav_history_buttons(cx))
            })
            .when_some(
                project_visual,
                |bar, (name, icon, icon_color, icon_image_path)| {
                    bar.child(
                        // The project name shows in full (never truncated); the
                        // branch is what ellipsizes when space runs short.
                        h_flex()
                            .flex_none()
                            .gap_1p5()
                            .items_center()
                            .child(
                                div()
                                    .flex_none()
                                    .size(px(22.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(project_icon_element(
                                        &icon,
                                        &icon_color,
                                        icon_image_path.as_deref(),
                                        px(22.),
                                        px(16.),
                                        cx,
                                    )),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(crate::ui::design::text_head())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child(gpui::SharedString::from(name)),
                            ),
                    )
                },
            )
            .when_some(repository_context, |bar, (project_id, options)| {
                let label = options
                    .iter()
                    .find_map(|(_, label, selected)| selected.then_some(label.clone()))
                    .unwrap_or_else(|| "Repository".to_string());
                let git_states = self.git_states.clone();
                bar.child(
                    crate::ui::style::header_dropdown_button("titlebar-repository-selector", cx)
                        .child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::FolderGit2,
                            crate::ui::design::sky(cx).opacity(0.78),
                            crate::ui::design::icon_sm(),
                        ))
                        .child(div().max_w(px(150.)).truncate().child(label))
                        .dropdown_menu(move |menu, _, _| {
                            options.iter().fold(menu, |menu, (path, label, selected)| {
                                let path = path.clone();
                                let git_states = git_states.clone();
                                menu.item(
                                    PopupMenuItem::new(label.clone())
                                        .checked(*selected)
                                        .on_click(move |_, _, cx| {
                                            git_states.update(cx, |states, cx| {
                                                states.set_active_repository(
                                                    project_id,
                                                    path.clone(),
                                                    cx,
                                                )
                                            });
                                        }),
                                )
                            })
                        }),
                )
            })
            .when_some(branch, |bar, branch| {
                bar.child(
                    div()
                        .relative()
                        .child(
                            h_flex()
                                .id("titlebar-branch-selector")
                                .max_w(px(200.))
                                .min_w(px(0.))
                                .px_2()
                                .py_0p5()
                                .gap_1p5()
                                .items_center()
                                .rounded(crate::ui::design::r_sm())
                                .cursor_pointer()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t3(cx))
                                .when(self.title_branch_expanded, |row| {
                                    row.bg(crate::ui::design::surface_2(cx))
                                })
                                .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                .on_hover(cx.listener(|this, hovered, _, cx| {
                                    this.title_branch_hovered = *hovered;
                                    cx.notify();
                                }))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.title_branch_expanded = !this.title_branch_expanded;
                                    if this.title_branch_expanded {
                                        this.title_branch_query.update(cx, |input, cx| {
                                            input.set_value("", window, cx);
                                            input.focus(window, cx);
                                        });
                                    }
                                    cx.notify();
                                }))
                                .child(branch_icon(crate::ui::design::t3(cx)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .child(gpui::SharedString::from(branch)),
                                )
                                .when(
                                    self.title_branch_hovered || self.title_branch_expanded,
                                    |row| {
                                        row.child(
                                            Icon::new(IconName::ChevronDown)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::t3(cx)),
                                        )
                                    },
                                ),
                        )
                        .child(
                            gpui::canvas(
                                move |bounds, _, cx| {
                                    root_view.update(cx, |this, _| {
                                        this.title_branch_bounds = Some(bounds);
                                    });
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full(),
                        ),
                )
            })
    }

    /// The fixed vertical activity rail docked to the right of the project tools.
    pub(super) fn nav_rail(
        &self,
        on_right: bool,
        show_divider: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let activity = self.center.read(cx).activity();
        let quick_ask_history_open = self.center.read(cx).is_quick_ask_history_view();
        let active_project = self.workspace.read(cx).active;
        let onboarding_project = active_project
            .is_some_and(|project_id| crate::ui::onboarding::is_touring_project(project_id, cx));
        let mut visible_activities = active_project
            .map(|project_id| self.workspace.read(cx).project_activities(project_id))
            .unwrap_or_else(ide_core::config::default_pinned_project_activities);
        if onboarding_project {
            for required in ProjectActivityId::ALL {
                if !visible_activities.contains(&required) {
                    visible_activities.push(required);
                }
            }
        }
        // An activity opened from a keyboard shortcut or the command palette
        // remains locatable while active, even if it is normally unpinned.
        if let Some(active_id) = activity.persisted_id() {
            if !visible_activities.contains(&active_id) {
                visible_activities.push(active_id);
            }
        }
        // The design's `.rail` sits on `sink` — the darkest plane, one step below
        // the `nav` sidebar — so the rail reads as its own deepest column.
        let panel_bg = crate::ui::design::sink(cx);
        let item = |id: &'static str,
                    icon: RailIcon,
                    label: &'static str,
                    target: ProjectActivity,
                    cx: &mut Context<Self>| {
            let center = self.center.clone();
            let selected = !quick_ask_history_open && activity == target;
            let is_docs = target == ProjectActivity::Docs;
            let is_tasks = target == ProjectActivity::Tasks;
            // Both rails give the active cell a filled accent chip around its
            // icon so the selection is unmistakable (on the right it lives in the
            // divider column; on the left, inside a rounded pill).
            let fg = if selected {
                crate::ui::design::t1(cx)
            } else {
                crate::ui::design::t3(cx)
            };
            v_flex()
                .id(id)
                .relative()
                .flex_none()
                .h(if on_right {
                    crate::ui::design::rail_footer_cell_h()
                } else {
                    px(56.)
                })
                .gap_1()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .text_color(fg)
                .map(|el| {
                    // The cell never fills — the highlight lives on the icon pill
                    // only (below). The right rail's left-edge divider is one
                    // soft overlay on the rail itself, not per-cell borders.
                    if on_right {
                        el.w_full()
                    } else {
                        el.mx(px(8.))
                    }
                })
                .child(
                    // The design's `.railitem .ic-lg`: the highlight is on the icon
                    // pill only — hover = `surface`, active = `surface-2` — never the
                    // whole cell, and never accent (the rail is the location channel).
                    div()
                        .relative()
                        .size(px(30.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(crate::ui::design::r_sm())
                        .when(selected, |b| b.bg(crate::ui::design::surface_2(cx)))
                        .when(!selected, |b| {
                            b.hover(|s| s.bg(crate::ui::design::surface(cx)))
                        })
                        .child(match icon {
                            RailIcon::Component(icon) => Icon::new(icon)
                                .size(crate::ui::design::icon_lg())
                                .text_color(fg)
                                .into_any_element(),
                            RailIcon::Lucide(icon) => crate::ui::design::indicator::lucide_icon(
                                icon,
                                fg,
                                crate::ui::design::icon_lg(),
                            )
                            .into_any_element(),
                            RailIcon::Tumble => {
                                crate::ui::design::indicator::tumble_rail_icon().into_any_element()
                            }
                        }),
                )
                .child(
                    // `.railitem` label: 11px, `t3` by default, `t1` when active.
                    div()
                        .text_size(crate::ui::design::text_label())
                        .font_weight(if selected {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .child(label),
                )
                .when(is_docs, |item| {
                    item.child(crate::ui::onboarding::target_marker(
                        crate::ui::onboarding::SpotlightTarget::DocsNav,
                        cx,
                    ))
                })
                .when(is_tasks, |item| {
                    item.child(crate::ui::onboarding::target_marker(
                        crate::ui::onboarding::SpotlightTarget::TasksNav,
                        cx,
                    ))
                })
                .on_click(move |_, _, cx| {
                    center.update(cx, |center, cx| center.show_activity(target, cx));
                    if is_docs && active_project.is_some() {
                        crate::ui::onboarding::emit_for_project(
                            active_project.unwrap(),
                            crate::ui::onboarding::OnboardingEvent::DocsOpened,
                            cx,
                        );
                    }
                    if is_tasks && active_project.is_some() {
                        crate::ui::onboarding::emit_for_project(
                            active_project.unwrap(),
                            crate::ui::onboarding::OnboardingEvent::TasksOpened,
                            cx,
                        );
                    }
                })
        };

        let activity_items = [
            (
                "rail-agents",
                RailIcon::Component(IconName::Bot),
                "Agents",
                ProjectActivity::Agents,
                ProjectActivityId::Agents,
            ),
            (
                "rail-code",
                RailIcon::Component(IconName::PanelBottomOpen),
                "Code",
                ProjectActivity::Code,
                ProjectActivityId::Code,
            ),
            (
                "rail-tasks",
                RailIcon::Component(crate::ui::design::tasks_icon()),
                "Tasks",
                ProjectActivity::Tasks,
                ProjectActivityId::Tasks,
            ),
            (
                "rail-docs",
                RailIcon::Component(crate::ui::design::docs_icon()),
                "Docs",
                ProjectActivity::Docs,
                ProjectActivityId::Docs,
            ),
            (
                "rail-design",
                RailIcon::Component(crate::ui::design::design_icon()),
                "Design",
                ProjectActivity::Design,
                ProjectActivityId::Design,
            ),
            (
                "rail-db",
                RailIcon::Component(IconName::Database),
                "DB",
                ProjectActivity::Db,
                ProjectActivityId::Db,
            ),
            (
                "rail-designs",
                RailIcon::Lucide(lucide_icons::Icon::Bookmark),
                "Assets",
                ProjectActivity::Designs,
                ProjectActivityId::Assets,
            ),
            (
                "rail-services",
                RailIcon::Component(IconName::Network),
                "Orbit",
                ProjectActivity::Services,
                ProjectActivityId::Orbit,
            ),
        ]
        .into_iter()
        .filter(|(_, _, _, _, persisted)| visible_activities.contains(persisted))
        .map(|(id, icon, label, target, _)| item(id, icon, label, target, cx))
        .collect::<Vec<_>>();

        let customizer = (on_right && !onboarding_project)
            .then(|| {
                active_project.map(|project_id| {
                    self.activity_customizer(
                        project_id,
                        self.workspace.read(cx).project_activities(project_id),
                        activity,
                        cx,
                    )
                    .into_any_element()
                })
            })
            .flatten();

        v_flex()
            .flex_none()
            .w(px(if on_right { 66. } else { 60. }))
            .h_full()
            // Right-rail cells run flush and carry their own left-edge dividers;
            // the left rail keeps its inset, rounded pills.
            .when(!on_right, |rail| rail.pt_2().gap_1())
            .bg(panel_bg)
            // Left rail: a divider on the sidebar side so the rail reads as its
            // own column, not a bleed of the sidebar (shown whenever the sidebar
            // is present; when it's collapsed the rail is at the window edge).
            .when(!on_right && !show_divider, |rail| {
                rail.border_l_1().border_color(style::hairline(cx))
            })
            .when(show_divider && !on_right, |rail| {
                rail.border_r_1().border_color(style::hairline(cx))
            })
            // Right rail: the left-edge divider is a single soft rule that
            // fades at both ends, overlaid on the rail — "light instead of
            // lines" — rather than border segments chained across every cell.
            .when(on_right, |rail| {
                let separator_style = self.workspace.read(cx).separator_style;
                rail.relative()
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(0.))
                            .bottom(px(0.))
                            .w(px(1.))
                            .child(style::separator_vline(separator_style, cx)),
                    )
                    // Short top spacer so the first cell's chip lines up with
                    // the top of the panel's Board/Git toggle.
                    .child(div().flex_none().h(px(4.)))
            })
            .child(
                v_flex()
                    .id(if on_right {
                        "right-rail-activities"
                    } else {
                        "left-rail-activities"
                    })
                    .w_full()
                    .min_h(px(0.))
                    .when(on_right, |list| list.flex_1())
                    .when(!on_right, |list| list.gap_1())
                    .overflow_y_scrollbar()
                    .children(activity_items),
            )
            // The activity list absorbs spare height and scrolls on short
            // windows, keeping the equally spaced footer actions reachable.
            .when(on_right, |rail| {
                rail.when_some(customizer, |rail, customizer| rail.child(customizer))
                    .when(self.pocketcomet_connected, |rail| {
                        rail.child(item(
                            "rail-pocketcomet",
                            RailIcon::Tumble,
                            "Tumble",
                            ProjectActivity::PocketComet,
                            cx,
                        ))
                    })
                    .child(self.rail_footer(cx))
            })
    }
}
