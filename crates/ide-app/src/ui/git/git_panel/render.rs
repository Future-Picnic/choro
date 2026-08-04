use super::*;

impl Render for GitPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.maybe_refresh_solo_ahead(cx);
        self.sync_lane_scope(cx);
        let active_project = self.workspace.read(cx).active;
        let active_repository = self
            .active_git(cx)
            .map(|git| git.read(cx).repo_path.clone());
        if self.last_active_project != active_project
            || self.last_active_repository != active_repository
        {
            self.last_active_project = active_project;
            self.last_active_repository = active_repository;
            self.commit_ai_error = None;
            self.commit_ai_generating = false;
            self.last_push_notice_message = None;
            self.branch_pr_key = None;
            self.branch_pr_last_message = None;
            self.branch_pr_checked_at = None;
            self.branch_pr_fetching = false;
            self.branch_pr = None;
            self.repo_pr_key = None;
            self.repo_prs_last_message = None;
            self.repo_prs.clear();
            self.repo_prs_fetching = false;
            self.repo_prs_checked_at = None;
            self.repo_prs_error = None;
            self.collapsed_status_folders.clear();
            self.hovered_status_file = None;
            self.branch_row_hovered = false;
            self.pr_chip_hovered = false;
            self.commit_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
            });
        }

        let Some(git) = self.active_git(cx) else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_color(crate::ui::design::t3(cx))
                .child("No project selected")
                .into_any_element();
        };

        if !git.read(cx).is_repo {
            let (busy, error) = {
                let state = git.read(cx);
                (state.is_busy, state.last_error.clone())
            };
            let initialize_git = git.clone();
            let publish_git = git.clone();
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .px_5()
                .child(
                    v_flex()
                        .w_full()
                        .max_w(px(320.))
                        .gap_4()
                        .child(
                            v_flex()
                                .gap_1p5()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child("Start using source control"),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("This project doesn't have a Git repository. Initialize one to track changes, create commits, and use branches."),
                                ),
                        )
                        .child(
                            crate::ui::style::primary_button_compact(
                                "initialize-git-repository",
                                if busy {
                                    "Working…"
                                } else {
                                    "Initialize Repository"
                                },
                                cx,
                            )
                            .w_full()
                            .disabled(busy)
                            .on_click(move |_, _, cx| {
                                initialize_git.update(cx, |git, cx| {
                                    git.initialize_repository(cx);
                                });
                            }),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Or publish this folder directly to a new GitHub repository. Choro will initialize Git, create the first commit, add origin, and push it."),
                                )
                                .child(
                                    crate::ui::style::ship_button(
                                        "publish-project-to-github",
                                        "Publish to GitHub",
                                        cx,
                                    )
                                    .w_full()
                                    .disabled(busy)
                                    .on_click(move |_, window, cx| {
                                        open_publish_repository_dialog(
                                            publish_git.clone(),
                                            window,
                                            cx,
                                        );
                                    }),
                                ),
                        )
                        .when_some(error, |content, error| {
                            content.child(
                                div()
                                    .w_full()
                                    .rounded(crate::ui::design::r_sm())
                                    .border_1()
                                    .border_color(crate::ui::design::rose(cx).opacity(0.45))
                                    .bg(crate::ui::design::rose(cx).opacity(0.08))
                                    .px_3()
                                    .py_2()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::rose(cx))
                                    .child(SharedString::from(error)),
                            )
                        }),
                )
                .into_any_element();
        }

        self.sync_push_notice(git.clone(), window, cx);
        self.sync_branch_pull_request(git.clone(), cx);
        self.sync_repo_pull_requests(git.clone(), cx);

        let entries = git
            .read(cx)
            .snapshot
            .as_ref()
            .map(|s| s.entries.clone())
            .unwrap_or_default();
        let (staged, unstaged, untracked) = split_entries(&entries);
        let clean = staged.is_empty() && unstaged.is_empty() && untracked.is_empty();
        let (status_view, status_group) = {
            let workspace = self.workspace.read(cx);
            (workspace.git_status_view, workspace.git_status_group)
        };
        let hovered_status_file = self.hovered_status_file.clone();
        let tab = self.tab;
        let project = self.workspace.read(cx).active;
        let center = self.center.clone();
        let view_diff_center = center.clone();
        let stage_all_git = git.clone();
        let (line_add, line_remove) = git
            .read(cx)
            .snapshot
            .as_ref()
            .map(|s| (s.insertions, s.deletions))
            .unwrap_or((0, 0));
        let has_stageable = !unstaged.is_empty() || !untracked.is_empty();
        let discard_all_count = unstaged.len() + untracked.len();
        let has_discardable = discard_all_count > 0;
        let has_unstageable = !staged.is_empty();
        let has_any_changes = !entries.is_empty();
        let stash_count = git.read(cx).stash_count;
        let pop_stash_detail: SharedString = format!(
            "{stash_count} stash{} available",
            if stash_count == 1 { "" } else { "es" }
        )
        .into();
        let top_stage_all_git = git.clone();
        let top_unstage_all_git = git.clone();
        let top_discard_all_git = git.clone();
        let top_stash_git = git.clone();
        let top_pop_stash_git = git.clone();
        let top_apply_stash_git = git.clone();
        let top_open_diff_center = center.clone();
        let discard_icon_color = crate::ui::design::rose(cx);
        let view_settings_button = {
            let list_workspace = self.workspace.clone();
            let tree_workspace = self.workspace.clone();
            let no_group_workspace = self.workspace.clone();
            let status_group_workspace = self.workspace.clone();
            Button::new("git-status-view-settings")
                .ghost()
                .xsmall()
                .compact()
                .h(crate::ui::design::control_h_xs())
                .icon(IconName::Settings2)
                .tooltip("Change file list view")
                .dropdown_menu(move |menu, _, _| {
                    let list_workspace = list_workspace.clone();
                    let tree_workspace = tree_workspace.clone();
                    let no_group_workspace = no_group_workspace.clone();
                    let status_group_workspace = status_group_workspace.clone();
                    menu.item(PopupMenuItem::label("View"))
                        .item(
                            PopupMenuItem::new("List")
                                .checked(status_view == GitStatusViewMode::List)
                                .on_click(move |_, _, cx| {
                                    list_workspace.update(cx, |workspace, cx| {
                                        workspace.set_git_status_view(GitStatusViewMode::List, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Tree")
                                .checked(status_view == GitStatusViewMode::Tree)
                                .on_click(move |_, _, cx| {
                                    tree_workspace.update(cx, |workspace, cx| {
                                        workspace.set_git_status_view(GitStatusViewMode::Tree, cx);
                                    });
                                }),
                        )
                        .separator()
                        .item(PopupMenuItem::label("Group By"))
                        .item(
                            PopupMenuItem::new("None")
                                .checked(status_group == GitStatusGroupMode::None)
                                .on_click(move |_, _, cx| {
                                    no_group_workspace.update(cx, |workspace, cx| {
                                        workspace
                                            .set_git_status_group(GitStatusGroupMode::None, cx);
                                    });
                                }),
                        )
                        .item(
                            PopupMenuItem::new("Status")
                                .checked(status_group == GitStatusGroupMode::Status)
                                .on_click(move |_, _, cx| {
                                    status_group_workspace.update(cx, |workspace, cx| {
                                        workspace
                                            .set_git_status_group(GitStatusGroupMode::Status, cx);
                                    });
                                }),
                        )
                })
        };

        // The stage control is a stateful split button: the main action stages
        // — or unstages, once everything is already staged — all changes, and the
        // ▾ holds every git action (replaces the old Stage-all + ⋯).
        let show_unstage = has_unstageable && !has_stageable;
        // Neutral (like Commit Tracked / Fetch) — staging is a secondary action;
        // accent is reserved for the one primary action (Commit).
        // Our design-system SplitButton with the same `chatbox` palette as Fetch,
        // so Stage-all is the *same component* — not gpui's DropdownButton widget.
        let stage_split_button = SplitButton::new(
            "stage-split",
            if show_unstage { "Unstage All" } else { "Stage All" },
            SplitPalette::chatbox(cx),
            move |mut menu, _, _| {
                let stage_all = top_stage_all_git.clone();
                let unstage_all = top_unstage_all_git.clone();
                let discard_all = top_discard_all_git.clone();
                let stash = top_stash_git.clone();
                let pop = top_pop_stash_git.clone();
                let apply = top_apply_stash_git.clone();
                let pop_stash_detail = pop_stash_detail.clone();
                let open_diff_center = top_open_diff_center.clone();
                menu = menu
                    .item(
                        PopupMenuItem::new("Stage all")
                            .icon(IconName::Check)
                            .disabled(!has_stageable)
                            .on_click(move |_, _, cx| {
                                stage_all.update(cx, |git, cx| git.stage_all(cx));
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Unstage all")
                            .icon(IconName::Minus)
                            .disabled(!has_unstageable)
                            .on_click(move |_, _, cx| {
                                unstage_all.update(cx, |git, cx| git.unstage_all(cx));
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Discard all")
                            .icon(
                                gpui_component::Icon::new(IconName::Delete)
                                    .text_color(discard_icon_color),
                            )
                            .disabled(!has_discardable)
                            .on_click(move |_, window, cx| {
                                confirm_git_action(
                                    discard_all.clone(),
                                    window,
                                    cx,
                                    GitConfirmation {
                                        title: "Discard All",
                                        description: "This restores tracked files from Git and deletes untracked files. Staged changes are left alone.",
                                        detail: format!(
                                            "{discard_all_count} file{}",
                                            if discard_all_count == 1 { "" } else { "s" }
                                        )
                                        .into(),
                                        confirm_id: "confirm-discard-all",
                                        confirm_label: "Discard All",
                                        action: GitState::discard_all,
                                    },
                                );
                            }),
                    );
                menu = menu
                    .separator()
                    .item(
                        PopupMenuItem::new("Stash all changes")
                            .icon(IconName::Inbox)
                            .disabled(!has_any_changes)
                            .on_click(move |_, _, cx| {
                                stash.update(cx, |git, cx| git.stash_all(cx));
                            }),
                    )
                    .item(
                        PopupMenuItem::new(format!("Pop stash ({stash_count})"))
                            .icon(IconName::ArrowUp)
                            .disabled(stash_count == 0)
                            .on_click(move |_, window, cx| {
                                confirm_git_action(
                                    pop.clone(),
                                    window,
                                    cx,
                                    GitConfirmation {
                                        title: "Pop Stash",
                                        description: "This applies the latest stash and removes it from the stash stack. Conflicts may need manual cleanup.",
                                        detail: pop_stash_detail.clone(),
                                        confirm_id: "confirm-pop-stash",
                                        confirm_label: "Pop Stash",
                                        action: GitState::stash_pop,
                                    },
                                );
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Apply stash")
                            .icon(IconName::Copy)
                            .disabled(stash_count == 0)
                            .on_click(move |_, _, cx| {
                                apply.update(cx, |git, cx| git.stash_apply(cx));
                            }),
                    );
                menu.separator().item(
                    PopupMenuItem::new("Open diff")
                        .icon(IconName::ExternalLink)
                        .disabled(clean)
                        .on_click(move |_, _, cx| {
                            let Some(project) = project else { return };
                            open_diff_center
                                .update(cx, |center, cx| {
                                    center.open_diff_from_git(
                                        project,
                                        crate::ui::git::diff_pane::DiffKind::Project,
                                        "All Changes".into(),
                                        cx,
                                    );
                                })
                                .ok();
                        }),
                )
            },
        )
        .disabled(clean)
        .on_primary(move |_, cx| {
            stage_all_git.update(cx, |git, cx| {
                if show_unstage {
                    git.unstage_all(cx);
                } else {
                    git.stage_all(cx);
                }
            });
        });

        let staged_rows = status_list_entries(staged, true);
        let unstaged_rows = status_list_entries(unstaged, false);
        let untracked_rows = status_list_entries(untracked, false);
        let collapsed_folders = self.collapsed_status_folders.clone();
        let status_content: gpui::AnyElement = if clean {
            v_flex()
                .w_full()
                .items_center()
                .py_8()
                .gap_2()
                .text_color(crate::ui::design::t3(cx))
                .child(
                    gpui_component::Icon::new(IconName::CircleCheck)
                        .size(crate::ui::design::icon_xl())
                        .text_color(crate::ui::design::sage(cx)),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .child("Working tree clean"),
                )
                .into_any_element()
        } else if status_group == GitStatusGroupMode::Status {
            v_flex()
                .w_full()
                .when(!staged_rows.is_empty(), |panel| {
                    panel.child(render_section(
                        Some("STAGED"),
                        staged_rows,
                        true,
                        status_view,
                        &collapsed_folders,
                        hovered_status_file.as_ref(),
                        git.clone(),
                        project,
                        center.clone(),
                        cx,
                    ))
                })
                .when(!unstaged_rows.is_empty(), |panel| {
                    panel.child(render_section(
                        Some("CHANGES"),
                        unstaged_rows,
                        false,
                        status_view,
                        &collapsed_folders,
                        hovered_status_file.as_ref(),
                        git.clone(),
                        project,
                        center.clone(),
                        cx,
                    ))
                })
                .when(!untracked_rows.is_empty(), |panel| {
                    panel.child(render_section(
                        Some("UNTRACKED"),
                        untracked_rows,
                        false,
                        status_view,
                        &collapsed_folders,
                        hovered_status_file.as_ref(),
                        git.clone(),
                        project,
                        center.clone(),
                        cx,
                    ))
                })
                .into_any_element()
        } else {
            let mut all_rows = staged_rows;
            all_rows.extend(unstaged_rows);
            all_rows.extend(untracked_rows);
            render_section(
                None,
                all_rows,
                false,
                status_view,
                &collapsed_folders,
                hovered_status_file.as_ref(),
                git.clone(),
                project,
                center.clone(),
                cx,
            )
        };

        let body: gpui::AnyElement = match tab {
            GitTab::Commits => self.render_history(git.clone(), cx).into_any_element(),
            GitTab::PullRequests => self.render_pull_requests(cx).into_any_element(),
            GitTab::Changes => v_flex()
                .flex_1()
                .min_h(px(0.))
                .child(
                    // Clickable diff summary (opens the project diff) + Stage all
                    // + the Changes-scoped git-actions menu.
                    h_flex()
                        .id("view-diff-row")
                        .w_full()
                        .px_3()
                        .py_1p5()
                        .gap_1p5()
                        .items_center()
                        .child(
                            h_flex()
                                .id("view-diff-action")
                                .gap_1()
                                .items_center()
                                .px_2()
                                .py_1()
                                .rounded(crate::ui::design::r_sm())
                                .cursor_pointer()
                                .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                .on_click(move |_, _, cx| {
                                    let Some(project) = project else { return };
                                    view_diff_center
                                        .update(cx, |center, cx| {
                                            center.open_diff_from_git(
                                                project,
                                                crate::ui::git::diff_pane::DiffKind::Project,
                                                "All Changes".into(),
                                                cx,
                                            );
                                        })
                                        .ok();
                                })
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_1()
                                        .child(
                                            svg()
                                                .path("icons/file-diff.svg")
                                                .size(crate::ui::design::icon_md())
                                                .flex_none()
                                                .text_color(crate::ui::design::t3(cx)),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .text_color(if clean {
                                                    crate::ui::design::t3(cx)
                                                } else {
                                                    crate::ui::design::t1(cx)
                                                })
                                                .child("View"),
                                        ),
                                )
                                .when(line_add > 0, |row| {
                                    row.child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::sage(cx))
                                            .child(format!("+{line_add}")),
                                    )
                                })
                                .when(line_remove > 0, |row| {
                                    row.child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::rose(cx))
                                            .child(format!("−{line_remove}")),
                                    )
                                }),
                        )
                        .child(div().flex_1())
                        .child(view_settings_button)
                        .child(stage_split_button),
                )
                .child(
                    v_flex()
                        .id("git-status-scroll")
                        .flex_1()
                        .min_h(px(0.))
                        .overflow_y_scroll()
                        .child(status_content),
                )
                .into_any_element(),
        };

        // The Changes/Commits/PRs tabs now live in the right-panel header row
        // (shared with the "Git" label + view flip), so the panel body starts here.
        div()
            .relative()
            .size_full()
            .child(
                v_flex()
                    .size_full()
                    // Solo scope flip — exists only while a Solo is the open
                    // agent; defaults to the Solo, flips back to Main.
                    .when_some(self.render_scope_flip(cx), |panel, flip| panel.child(flip))
                    .child(body)
                    .when(tab == GitTab::Changes, |panel| {
                        panel.child(self.render_commit_area(git.clone(), cx))
                    }),
            )
            .into_any_element()
    }
}
