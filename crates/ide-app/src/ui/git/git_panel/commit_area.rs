use super::*;

impl GitPanel {
    pub(super) fn render_commit_area(
        &self,
        git: Entity<GitState>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let repository_options = self.repository_options(cx);
        let repository_selector = (repository_options.len() > 1).then(|| {
            let label = repository_options
                .iter()
                .find_map(|(_, label, selected)| selected.then_some(label.clone()))
                .unwrap_or_else(|| "Repository".to_string());
            let options = repository_options.clone();
            let panel = cx.entity().clone();
            crate::ui::style::header_meta_button("git-commit-repository", cx)
                .max_w(px(118.))
                .child(
                    h_flex()
                        .min_w(px(0.))
                        .items_center()
                        .gap_1()
                        .child(crate::ui::design::indicator::lucide_icon(
                            lucide_icons::Icon::FolderGit2,
                            crate::ui::design::sky(cx).opacity(0.78),
                            crate::ui::design::icon_sm(),
                        ))
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t2(cx))
                                .child(label),
                        )
                        .child(
                            gpui_component::Icon::new(IconName::ChevronDown)
                                .size(crate::ui::design::icon_sm())
                                .text_color(crate::ui::design::t4(cx)),
                        ),
                )
                .tooltip("Active repository")
                .dropdown_menu(move |menu, _, _| {
                    options.iter().fold(menu, |menu, (path, label, selected)| {
                        let path = path.clone();
                        let panel = panel.clone();
                        menu.item(
                            PopupMenuItem::new(label.clone())
                                .checked(*selected)
                                .on_click(move |_, _, cx| {
                                    panel.update(cx, |panel, cx| {
                                        panel.select_repository(path.clone(), cx)
                                    });
                                }),
                        )
                    })
                })
        });
        let state = git.read(cx);
        let busy = state.is_busy;
        let has_staged = state
            .snapshot
            .as_ref()
            .map(|s| s.staged().count() > 0)
            .unwrap_or(false);
        let message = state.last_message.clone();
        let error = state.last_error.clone();
        let mut status_notice = self
            .commit_ai_error
            .clone()
            .map(|text| (text, true))
            .or_else(|| {
                self.commit_ai_generating
                    .then(|| ("Generating commit message…".to_string(), false))
            });
        if status_notice.is_none() {
            status_notice = error
                .clone()
                .map(|text| (text, true))
                .or_else(|| message.clone().map(|text| (text, false)));
        }

        // Zed-style remote button: the visible action is the next useful
        // network operation, while the menu keeps the manual operations.
        let head = state
            .snapshot
            .as_ref()
            .and_then(|s| s.branches.iter().find(|b| b.is_head));
        let remote_primary_action = match head {
            Some(b) if b.upstream.is_none() => RemotePrimaryAction::Publish,
            Some(b) if b.ahead > 0 => RemotePrimaryAction::Push,
            Some(b) if b.behind > 0 => RemotePrimaryAction::Pull,
            _ => RemotePrimaryAction::Fetch,
        };
        let remote_primary_label: SharedString = match (remote_primary_action, head) {
            (RemotePrimaryAction::Publish, _) => "Publish".into(),
            (RemotePrimaryAction::Push, Some(b)) if b.ahead > 0 => {
                format!("Push ↑{}", b.ahead).into()
            }
            (RemotePrimaryAction::Push, _) => "Push".into(),
            (RemotePrimaryAction::Pull, Some(b)) if b.behind > 0 => {
                format!("Pull ↓{}", b.behind).into()
            }
            (RemotePrimaryAction::Pull, _) => "Pull".into(),
            (RemotePrimaryAction::Fetch, _) => "Fetch".into(),
        };
        let menu_push_label: SharedString = match head {
            Some(b) if b.upstream.is_none() => "Publish".into(),
            Some(b) if b.ahead > 0 => format!("Push ↑{}", b.ahead).into(),
            _ => "Push".into(),
        };
        let menu_pull_label: SharedString = match head {
            Some(b) if b.behind > 0 => format!("Pull ↓{}", b.behind).into(),
            _ => "Pull".into(),
        };

        let has_tracked_changes = state
            .snapshot
            .as_ref()
            .map(|s| s.unstaged().count() > 0 || s.staged().count() > 0)
            .unwrap_or(false);
        let commit_input_has_text = !self.commit_input.read(cx).value().trim().is_empty();
        let can_generate_commit_message =
            has_tracked_changes && !busy && !self.commit_ai_generating && !commit_input_has_text;
        let has_commits = !state.history.is_empty();

        let branch_name: SharedString = state
            .branch_label()
            .unwrap_or_else(|| "…".to_string())
            .into();
        let head_position: Option<SharedString> = match head {
            Some(b) if b.ahead > 0 || b.behind > 0 => {
                Some(format!("↑{} ↓{}", b.ahead, b.behind).into())
            }
            _ => None,
        };
        let last_commit = state.history.first().cloned();
        let has_last_commit = last_commit.is_some();
        let force_push_detail: SharedString = format!("Current branch: {branch_name}").into();
        let uncommit_detail: SharedString = last_commit
            .as_ref()
            .map(|commit| commit.summary.clone())
            .unwrap_or_else(|| "Last commit".into())
            .into();

        let commit_git = git.clone();
        let commit_all_git = git.clone();
        let commit_input = self.commit_input.clone();
        let commit_all_input = self.commit_input.clone();
        let push_git = git.clone();
        let force_push_git = git.clone();
        let pull_git = git.clone();
        let pull_rebase_git = git.clone();
        let fetch_git = git.clone();
        let primary_fetch_git = git.clone();
        let primary_pull_git = git.clone();
        let primary_push_git = git.clone();
        let uncommit_git = git.clone();
        let expanded = self.branches_expanded;
        let branch_pr = self.branch_pr.clone();
        let branch_hovered = self.branch_row_hovered;
        let pr_hovered = self.pr_chip_hovered;
        let solo_worktree_actions = self
            .scoped_to_lane()
            .then(|| {
                self.scope_agent
                    .map(|agent_id| (agent_id, state.repo_path.clone()))
            })
            .flatten();
        let solo_actions_center = self.center.clone();

        v_flex()
            .w_full()
            .gap_1()
            .pt_2()
            // Branch picker popup opens right above its trigger row.
            .when(expanded, |area| {
                area.child(self.render_branch_list(git.clone(), cx))
            })
            // Row: branch (left) · remote actions dropdown (right).
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .gap_2()
                    .items_center()
                    .when_some(repository_selector, |row, selector| row.child(selector))
                    .child(
                        h_flex()
                            .id("branch-button")
                            .when(self.scoped_to_lane(), |btn| {
                                btn.group("solo-worktree-branch")
                            })
                            // Release the row to the PR chip while it unfurls, so
                            // the branch collapses to just its icon and the PR
                            // gets the full width instead of overflowing.
                            .when(!pr_hovered, |btn| btn.flex_1().min_w(px(0.)))
                            .px_2()
                            .py_0p5()
                            .gap_1p5()
                            .items_center()
                            .rounded(crate::ui::design::r_sm())
                            .cursor_pointer()
                            .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // The Solo lives on its own branch — switching
                                // it out from under the agent isn't a thing.
                                // Say so kindly and point at the way back.
                                if this.scoped_to_lane() {
                                    if let Some(git) = this.active_git(cx) {
                                        git.update(cx, |git, cx| {
                                            git.last_message = Some(
                                                "This is the Solo's own branch — switch to the project branch to change branches."
                                                    .to_string(),
                                            );
                                            git.last_error = None;
                                            cx.notify();
                                        });
                                    }
                                    return;
                                }
                                this.branches_expanded = !this.branches_expanded;
                                cx.notify();
                            }))
                            .on_hover(cx.listener(|this, hovered, _, cx| {
                                this.branch_row_hovered = *hovered;
                                cx.notify();
                            }))
                            // Scoped to a Solo's lane: the fork in sky replaces
                            // the branch glyph so a commit can never be aimed
                            // at the wrong tree by mistake.
                            .child(if self.scoped_to_lane() {
                                crate::ui::design::indicator::solo_icon(
                                    crate::ui::design::sky(cx),
                                    crate::ui::design::icon_sm(),
                                )
                                .into_any_element()
                            } else {
                                branch_icon(crate::ui::design::t3(cx)).into_any_element()
                            })
                            .when(!pr_hovered, |btn| {
                                btn.child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(if self.scoped_to_lane() {
                                            crate::ui::design::sky(cx)
                                        } else {
                                            crate::ui::design::t1(cx).opacity(0.9)
                                        })
                                        .truncate()
                                        .child(branch_name),
                                )
                                .when_some(head_position, |row, label| {
                                    row.child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .child(label),
                                    )
                                })
                            })
                            // The expand chevron is quiet until you hover the
                            // branch (or the picker is already open), so the row
                            // reads as just the branch name at rest. A Solo's
                            // branch never opens the picker, so no chevron.
                            .when((branch_hovered || expanded) && !self.scoped_to_lane(), |btn| {
                                btn.child(
                                    gpui_component::Icon::new(if expanded {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronUp
                                    })
                                    .size(crate::ui::design::icon_sm())
                                    .text_color(crate::ui::design::t3(cx)),
                                )
                            })
                            // A Solo branch cannot be switched, so its hover
                            // affordance exposes worktree actions instead of the
                            // ordinary branch-picker chevron.
                            .when_some(
                                solo_worktree_actions,
                                |btn, (agent_id, lane_path)| {
                                    let delete_center = solo_actions_center.clone();
                                    let open_path = lane_path.to_string_lossy().into_owned();
                                    btn.child(
                                        div()
                                            .flex_none()
                                            // Keep the dropdown's anchor mounted
                                            // after the pointer leaves the branch
                                            // for the popup. Removing the trigger
                                            // here would immediately close it.
                                            .invisible()
                                            .group_hover("solo-worktree-branch", |actions| {
                                                actions.visible()
                                            })
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                |_, _, cx| cx.stop_propagation(),
                                            )
                                            .child(
                                                crate::ui::style::header_icon_button(
                                                    (
                                                        "solo-worktree-actions",
                                                        agent_id.as_u128() as u64,
                                                    ),
                                                    IconName::Ellipsis,
                                                    cx,
                                                )
                                                .tooltip("Solo worktree actions")
                                                .dropdown_menu(move |menu, _, _| {
                                                    let delete_center = delete_center.clone();
                                                    let open_path = open_path.clone();
                                                    menu.item(
                                                        PopupMenuItem::new(
                                                            "Delete Solo Worktree",
                                                        )
                                                        .icon(IconName::Delete)
                                                        .on_click(move |_, window, cx| {
                                                            let _ = delete_center.update(
                                                                cx,
                                                                |center, cx| {
                                                                    center
                                                                        .confirm_delete_solo_worktree(
                                                                            agent_id, window, cx,
                                                                        );
                                                                },
                                                            );
                                                        }),
                                                    )
                                                    .separator()
                                                    .item(
                                                        PopupMenuItem::new("Open in Finder")
                                                            .icon(IconName::FolderOpen)
                                                            .on_click(move |_, _, _| {
                                                                crate::open_with::open_in(
                                                                    None,
                                                                    &open_path,
                                                                );
                                                            }),
                                                    )
                                                }),
                                            ),
                                    )
                                },
                            ),
                    )
                    .when(busy, |row| row.child(Spinner::new().xsmall()))
                    // The PR indicator lives with the branch (a PR is branch-
                    // scoped): a status-tinted glyph + number that stays quiet
                    // until hover, when it unfurls the full title in place —
                    // the branch name yields rather than a tooltip popping up.
                    .when_some(branch_pr, |row, pr| {
                        let (status_label, accent) = pull_request_status_style(&pr, cx);
                        let url = pr.url.clone();
                        let number = pr.number;
                        let can_merge = pr.state.eq_ignore_ascii_case("OPEN") && !pr.is_draft;
                        let merge_pr = pr.clone();
                        let merge_git = git.clone();
                        let panel = cx.entity();
                        let full: SharedString =
                            format!("· {} · {}", status_label, pr.title).into();
                        row.child(
                            crate::ui::style::header_meta_button("branch-pr-chip", cx)
                                .min_w(px(0.))
                                // Fill the freed row on hover so the title has the
                                // whole width to truncate into — never clipping
                                // past the panel edge or shoving Fetch off-screen.
                                .when(pr_hovered, |chip| chip.flex_1())
                                .gap_1()
                                .items_center()
                                .px_1p5()
                                .py_0p5()
                                .rounded(crate::ui::design::r_sm())
                                .cursor_pointer()
                                .on_hover(cx.listener(|this, hovered, _, cx| {
                                    this.pr_chip_hovered = *hovered;
                                    cx.notify();
                                }))
                                .child(pr_icon(accent))
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(format!("#{number}")),
                                )
                                .when(pr_hovered, |chip| {
                                    chip.child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.))
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t3(cx))
                                            .truncate()
                                            .child(full.clone())
                                            // Eased fade-in as it unfurls. gpui has
                                            // no hover transition, so the reveal
                                            // animates but the collapse snaps.
                                            .with_animation(
                                                "branch-pr-unfurl",
                                                Animation::new(Duration::from_millis(180)),
                                                |title, delta| title.opacity(delta),
                                            ),
                                    )
                                    .child(
                                        gpui_component::Icon::new(IconName::ChevronDown)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t4(cx)),
                                    )
                                })
                                .dropdown_menu(move |menu, _, _| {
                                    let open_url_value = url.clone();
                                    let merge_pr = merge_pr.clone();
                                    let merge_git = merge_git.clone();
                                    let panel = panel.clone();
                                    let open_item = PopupMenuItem::new("Open PR")
                                        .icon(IconName::ExternalLink)
                                        .on_click(move |_, _, _| open_url(&open_url_value));
                                    let menu = menu
                                        .item(
                                            PopupMenuItem::new(format!(
                                                "{} → {}",
                                                merge_pr.branch, merge_pr.base_branch
                                            ))
                                            .disabled(true),
                                        )
                                        .separator()
                                        .item(open_item);
                                    if can_merge {
                                        menu.separator().item(
                                            PopupMenuItem::new("Merge PR").on_click(
                                                move |_, window, cx| {
                                                    panel.update(cx, |panel, cx| {
                                                        panel.confirm_merge_pull_request(
                                                            merge_git.clone(),
                                                            merge_pr.clone(),
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                },
                                            ),
                                        )
                                    } else {
                                        menu
                                    }
                                }),
                        )
                    })
                    .child(
                        div().flex_none().child(
                            SplitButton::new(
                                "remote-actions",
                                remote_primary_label,
                                SplitPalette::chatbox(cx),
                                move |mut menu, _, _| {
                                    let fetch = fetch_git.clone();
                                    let pull = pull_git.clone();
                                    let pull_rebase = pull_rebase_git.clone();
                                    let push = push_git.clone();
                                    let force_push = force_push_git.clone();
                                    let force_push_detail = force_push_detail.clone();
                                    menu = menu
                                    .item(PopupMenuItem::new("Fetch").on_click(move |_, _, cx| {
                                        fetch.update(cx, |git, cx| git.fetch(cx));
                                    }))
                                    .item(PopupMenuItem::new(menu_pull_label.clone()).on_click(
                                        move |_, _, cx| {
                                            pull.update(cx, |git, cx| git.pull(cx));
                                        },
                                    ))
                                    .item(PopupMenuItem::new("Pull (Rebase)").on_click(
                                        move |_, _, cx| {
                                            pull_rebase.update(cx, |git, cx| git.pull_rebase(cx));
                                        },
                                    ));
                                menu.separator()
                                    .item(PopupMenuItem::new(menu_push_label.clone()).on_click(
                                        move |_, _, cx| {
                                            push.update(cx, |git, cx| git.push(cx));
                                        },
                                    ))
                                    .item(PopupMenuItem::new("Force Push").on_click(
                                        move |_, window, cx| {
                                            confirm_git_action(
                                                force_push.clone(),
                                                window,
                                                cx,
                                                GitConfirmation {
                                                    title: "Force Push",
                                                    description: "This uses force-with-lease and can still rewrite remote history if your branch diverged.",
                                                    detail: force_push_detail.clone(),
                                                    confirm_id: "confirm-force-push",
                                                    confirm_label: "Force Push",
                                                    action: GitState::push_force,
                                                },
                                            );
                                        },
                                    ))
                                },
                            )
                            .disabled(busy)
                            .on_primary(move |_, cx| match remote_primary_action {
                                RemotePrimaryAction::Fetch => {
                                    primary_fetch_git.update(cx, |git, cx| git.fetch(cx));
                                }
                                RemotePrimaryAction::Pull => {
                                    primary_pull_git.update(cx, |git, cx| git.pull(cx));
                                }
                                RemotePrimaryAction::Push | RemotePrimaryAction::Publish => {
                                    primary_push_git.update(cx, |git, cx| git.push(cx));
                                }
                            }),
                        ),
                    ),
            )
            .when_some(error.clone(), |area, error| {
                area.child(
                    h_flex()
                        .mx_2()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx).opacity(0.45))
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_2()
                        .py_1p5()
                        .child(
                            gpui_component::Icon::new(IconName::TriangleAlert)
                                .size(crate::ui::design::icon_md())
                                .text_color(crate::ui::design::rose(cx)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.))
                                .text_size(crate::ui::design::text_ui())
                                .line_height(gpui::relative(1.35))
                                .text_color(crate::ui::design::rose(cx))
                                .child(SharedString::from(error)),
                        ),
                )
            })
            // The commit editor uses the same stable flow as the chat composer:
            // a fixed editor viewport followed by a fixed action row. Nothing
            // overlays the input, so its caret and scroll bounds cannot jump
            // behind the controls while typing.
            .child(
                div()
                    .mx_2()
                    .h(crate::ui::design::composer_frame_h())
                    .flex_none()
                    .rounded(crate::ui::design::r_md())
                    .border_1()
                    .border_color(crate::ui::design::line(cx))
                    .overflow_hidden()
                    .bg(crate::ui::design::focus(cx))
                    .child(
                        v_flex()
                            .size_full()
                            .px_2()
                            .pt_2()
                            .pb_1()
                            .gap_1()
                            .child(
                                div()
                                    .w_full()
                                    .flex_1()
                                    .min_h(px(0.))
                                    .overflow_hidden()
                                    .child(
                                        Input::new(&self.commit_input)
                                            .appearance(false)
                                            .bordered(false)
                                            .focus_bordered(false)
                                            .w_full()
                                            .min_w(px(0.))
                                            .h_full(),
                                    ),
                            )
                            .child(
                            h_flex()
                                .w_full()
                                .items_center()
                                .justify_between()
                                .child(
                                    h_flex()
                                        .gap_0p5()
                                        .items_center()
                                        .child(
                                            Button::new("generate-commit-message")
                                                .ghost()
                                                .xsmall()
                                                .icon(IconName::Bot)
                                                .label("Generate")
                                                .text_color(crate::ui::design::accent(cx))
                                                .tooltip(if commit_input_has_text {
                                                    "Clear the message before generating"
                                                } else {
                                                    "Generate commit message with your selected AI provider"
                                                })
                                                .disabled(!can_generate_commit_message)
                                                .on_click({
                                                    let git = git.clone();
                                                    cx.listener(move |panel, _, window, cx| {
                                                        panel.generate_commit_message(
                                                            git.clone(),
                                                            has_staged,
                                                            window,
                                                            cx,
                                                        );
                                                    })
                                                }),
                                        )
                                        .when(self.commit_ai_generating, |row| {
                                            row.child(Spinner::new().xsmall())
                                        }),
                                )
                                .child({
                                    // Split button: main action adapts (Commit when staged,
                                    // Commit Tracked otherwise); the menu offers both.
                                    let main_label: SharedString = if has_staged {
                                        "Commit".into()
                                    } else {
                                        "Commit Tracked".into()
                                    };
                                    let menu_commit_git = commit_git.clone();
                                    let menu_commit_input = commit_input.clone();
                                    let menu_all_git = commit_all_git.clone();
                                    let menu_all_input = commit_all_input.clone();
                                    SplitButton::new(
                                        "commit-split",
                                        main_label,
                                        SplitPalette::editor(cx),
                                        move |mut menu, _, _| {
                                let staged_git = menu_commit_git.clone();
                                let staged_input = menu_commit_input.clone();
                                let all_git = menu_all_git.clone();
                                let all_input = menu_all_input.clone();
                                menu = menu.item(
                                    PopupMenuItem::new("Commit Staged")
                                        .disabled(!has_staged)
                                        .on_click(move |_, window, cx| {
                                            let text =
                                                staged_input.read(cx).value().trim().to_string();
                                            if text.is_empty() {
                                                return;
                                            }
                                            staged_git.update(cx, |git, cx| git.commit(text, cx));
                                            staged_input.update(cx, |input, cx| {
                                                input.set_value("", window, cx);
                                            });
                                        }),
                                );
                                menu.item(
                                    PopupMenuItem::new("Commit All Tracked")
                                        .disabled(!has_tracked_changes)
                                        .on_click(move |_, window, cx| {
                                            let text =
                                                all_input.read(cx).value().trim().to_string();
                                            if text.is_empty() {
                                                return;
                                            }
                                            all_git.update(cx, |git, cx| git.commit_all(text, cx));
                                            all_input.update(cx, |input, cx| {
                                                input.set_value("", window, cx);
                                            });
                                        }),
                                )
                                        },
                                    )
                                    .disabled(busy || (!has_staged && !has_tracked_changes))
                                    .on_primary(move |window, cx| {
                            let text = commit_input.read(cx).value().trim().to_string();
                            if text.is_empty() {
                                return;
                            }
                            if has_staged {
                                commit_git.update(cx, |git, cx| git.commit(text, cx));
                            } else {
                                commit_all_git.update(cx, |git, cx| git.commit_all(text, cx));
                            }
                            commit_input.update(cx, |input, cx| {
                                input.set_value("", window, cx);
                            });
                                    })
                                }),
                            ),
                    ),
            )
            // Feedback footer: always reserves header height (blank until a
            // commit exists) so the commit box above keeps the same baseline as
            // the chat composer, and the last-commit line sits level with the
            // Terminal/Files/Notes bar in the sibling panel. No top divider.
            .child(
                    h_flex()
                        .w_full()
                        .h(crate::ui::design::header_h())
                        .flex_none()
                        .px(crate::ui::design::git_meta_row_pad_x())
                        .gap_1()
                        .items_center()
                        .when_some(last_commit, |row, commit| {
                            row.child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .truncate()
                                    .child(SharedString::from(commit.summary.clone())),
                            )
                            .child(
                                Button::new("uncommit-last")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Undo2)
                                    .tooltip("Uncommit — soft reset, keep changes staged")
                                    .disabled(busy || !has_commits)
                                    .on_click(move |_, window, cx| {
                                        confirm_git_action(
                                            uncommit_git.clone(),
                                            window,
                                            cx,
                                            GitConfirmation {
                                                title: "Uncommit",
                                                description: "This runs a soft reset of HEAD~1 and keeps the commit changes staged.",
                                                detail: uncommit_detail.clone(),
                                                confirm_id: "confirm-uncommit",
                                                confirm_label: "Uncommit",
                                                action: GitState::uncommit,
                                            },
                                        );
                                    }),
                            )
                        })
                        .when(!has_last_commit, |row| row.child(div().flex_1()))
                        .when_some(status_notice, |row, (status, is_error)| {
                            let full_status = status.clone();
                            row.child(
                                Button::new("git-status-notice")
                                    .ghost()
                                    .xsmall()
                                    .icon(if is_error {
                                        IconName::TriangleAlert
                                    } else {
                                        IconName::CircleCheck
                                    })
                                    .label(if is_error { "Error" } else { "Done" })
                                    .text_color(if is_error {
                                        crate::ui::design::rose(cx)
                                    } else {
                                        crate::ui::design::t3(cx)
                                    })
                                    .tooltip(full_status.clone())
                                    .dropdown_menu(move |menu, _, _| {
                                        menu.item(PopupMenuItem::new(full_status.clone()))
                                    }),
                            )
                        }),
                )
    }
}
