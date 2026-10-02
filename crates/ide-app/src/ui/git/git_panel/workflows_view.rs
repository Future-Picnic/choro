use super::*;

fn workflow_policy_label(policy: GitWorkflowCompletionPolicy) -> &'static str {
    match policy {
        GitWorkflowCompletionPolicy::ConfirmBeforeMerge => "Confirm before merge",
        GitWorkflowCompletionPolicy::AutoMergeWhenReady => "Auto-merge when ready",
    }
}

pub(super) fn workflow_state_style(state: GitWorkflowRunState, cx: &App) -> (&'static str, Hsla) {
    match state {
        GitWorkflowRunState::CreatingPullRequest => ("Creating PR…", crate::ui::design::amber(cx)),
        GitWorkflowRunState::WaitingForRequirements => {
            ("Waiting for requirements", crate::ui::design::amber(cx))
        }
        GitWorkflowRunState::AwaitingConfirmation => {
            ("Ready to merge", crate::ui::design::sage(cx))
        }
        GitWorkflowRunState::AutoMergeEnabled => ("Waiting for GitHub", crate::ui::design::sky(cx)),
        GitWorkflowRunState::Merged => ("Merged", crate::ui::design::accent(cx)),
        GitWorkflowRunState::Blocked => ("Blocked", crate::ui::design::rose(cx)),
        GitWorkflowRunState::Closed => ("Closed", crate::ui::design::t3(cx)),
        GitWorkflowRunState::NeedsAttention => ("Needs attention", crate::ui::design::rose(cx)),
        GitWorkflowRunState::Failed => ("Failed", crate::ui::design::rose(cx)),
    }
}

impl GitPanel {
    pub(super) fn workflow_data_for_selected_repo(
        &self,
        cx: &App,
    ) -> Option<(ProjectId, PathBuf, Vec<GitWorkflow>, Vec<GitWorkflowRun>)> {
        let (project_id, _, repository_path) = self.selected_workflow_context(cx)?;
        let workspace = self.workspace.read(cx);
        let project = workspace
            .projects
            .iter()
            .find(|project| project.id == project_id)?;
        let workflows = project
            .git_workflows
            .iter()
            .filter(|workflow| workflow.repository_path == repository_path)
            .cloned()
            .collect();
        let runs = project
            .git_workflow_runs
            .iter()
            .filter(|run| run.repository_path == repository_path)
            .cloned()
            .collect();
        Some((project_id, repository_path, workflows, runs))
    }

    fn move_workflow(&mut self, workflow_id: uuid::Uuid, offset: isize, cx: &mut Context<Self>) {
        let Some((project_id, repository_path, workflows, _)) =
            self.workflow_data_for_selected_repo(cx)
        else {
            return;
        };
        let mut ordered: Vec<uuid::Uuid> = workflows.iter().map(|workflow| workflow.id).collect();
        let Some(index) = ordered.iter().position(|id| *id == workflow_id) else {
            return;
        };
        let target = index as isize + offset;
        if target < 0 || target >= ordered.len() as isize {
            return;
        }
        ordered.swap(index, target as usize);
        self.workspace.update(cx, |workspace, cx| {
            if let Err(error) =
                workspace.reorder_git_workflows(project_id, &repository_path, &ordered, cx)
            {
                eprintln!("failed to reorder Git workflows: {error:#}");
            }
        });
    }

    fn confirm_delete_workflow(&self, workflow: GitWorkflow, window: &mut Window, cx: &mut App) {
        let Some(project_id) = self.workspace.read(cx).active else {
            return;
        };
        let workspace = self.workspace.clone();
        let workflow_id = workflow.id;
        ConfirmDialog::new(
            "Delete workflow?",
            "This removes only the saved Choro definition. Remote branches, pull requests, and historical runs stay untouched.",
        )
        .tone(crate::ui::confirm::ConfirmTone::Danger)
        .icon(IconName::Delete)
        .detail(workflow.name)
        .confirm_label("Delete workflow")
        .confirm_id("confirm-delete-git-workflow")
        .on_confirm(move |_, cx| {
            workspace.update(cx, |workspace, cx| {
                workspace.delete_git_workflow(project_id, workflow_id, cx);
            });
        })
        .open(window, cx);
    }

    fn confirm_merge_workflow_run(&self, run: GitWorkflowRun, window: &mut Window, cx: &mut App) {
        let Some((project_id, repo_path, _)) = self.selected_workflow_context(cx) else {
            return;
        };
        let Some(number) = run.pull_request_number else {
            return;
        };
        let git = self.active_git(cx);
        let workspace = self.workspace.clone();
        ConfirmDialog::new(
            format!("Merge pull request #{number}?"),
            "Choro will re-read the exact pull request and source commit, then use the repository's preferred allowed merge method. GitHub protections still apply.",
        )
        .tone(crate::ui::confirm::ConfirmTone::Primary)
        .icon(IconName::GitHub)
        .branch_route(run.source_branch.clone(), run.destination_branch.clone())
        .checkbox(
            "Archive branch after merge",
            "Hide it from Choro's branch picker. Search for it to restore it; files and GitHub branches are kept.",
        )
        .confirm_label("Merge PR")
        .confirm_id("confirm-merge-git-workflow")
        .on_confirm_with_checkbox(move |archive, window, cx| {
            let repo_path = repo_path.clone();
            let mut pending_run = run.clone();
            let pull_request_selector = number.to_string();
            let destination = run.destination_branch.clone();
            let expected_head = run.expected_head_sha.clone();
            let git = git.clone();
            let workspace = workspace.clone();
            let window_handle = window.window_handle();
            cx.spawn(async move |cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        merge_pull_request_with_archive(
                            &repo_path,
                            &pull_request_selector,
                            Some(&destination),
                            expected_head.as_deref(),
                            archive,
                        )
                    })
                    .await;
                pending_run.updated_at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or(pending_run.updated_at);
                match &result {
                    Ok(_) => {
                        pending_run.state = GitWorkflowRunState::Merged;
                        pending_run.error = None;
                    }
                    Err(error) => {
                        pending_run.state = GitWorkflowRunState::NeedsAttention;
                        pending_run.error = Some(format!("{error:#}"));
                    }
                }
                workspace
                    .update(cx, |workspace, cx| {
                        workspace
                            .upsert_git_workflow_run(project_id, pending_run, cx)
                            .ok();
                    })
                    .ok();
                if let Some(git) = git {
                    git.update(cx, |git, cx| git.refresh(cx)).ok();
                }
                let notification = match result {
                    Ok(outcome) => outcome.notification(),
                    Err(error) => Notification::error(format!("{error:#}")),
                };
                window_handle
                    .update(cx, |_, window, cx| window.push_notification(notification, cx))
                    .ok();
            })
            .detach();
        })
        .open(window, cx);
    }

    fn render_workflow_repository_selector(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let repository_options = self.repository_options(cx);
        (repository_options.len() > 1).then(|| {
            let label = repository_options
                .iter()
                .find_map(|(_, label, selected)| selected.then_some(label.clone()))
                .unwrap_or_else(|| "Repository".to_string());
            let options = repository_options.clone();
            let panel = cx.entity();
            crate::ui::style::header_meta_button("git-workflow-repository", cx)
                .max_w(px(160.))
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
        })
    }

    fn render_saved_workflow_card(
        &self,
        workflow: GitWorkflow,
        latest_run: Option<GitWorkflowRun>,
        index: usize,
        total: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (status, status_color) = latest_run
            .as_ref()
            .map(|run| workflow_state_style(run.state, cx))
            .unwrap_or(("Ready to review", crate::ui::design::t3(cx)));
        let route = format!(
            "{} → {}",
            workflow.source_branch, workflow.destination_branch
        );
        let action_panel = cx.entity();
        let menu_panel = action_panel.clone();
        let run_workflow = workflow.clone();
        let edit_workflow = workflow.clone();
        let duplicate_id = workflow.id;
        let delete_workflow = workflow.clone();
        let move_up_id = workflow.id;
        let move_down_id = workflow.id;
        let project_id = self.workspace.read(cx).active;
        let workspace = self.workspace.clone();
        let busy = latest_run
            .as_ref()
            .is_some_and(|run| run.state == GitWorkflowRunState::CreatingPullRequest);
        let action_label = match latest_run.as_ref().map(|run| run.state) {
            Some(GitWorkflowRunState::CreatingPullRequest) => "Creating PR…",
            Some(GitWorkflowRunState::WaitingForRequirements) => "View status",
            Some(GitWorkflowRunState::AwaitingConfirmation) => "Review merge",
            Some(GitWorkflowRunState::AutoMergeEnabled) => "Waiting for GitHub",
            Some(GitWorkflowRunState::Blocked) => "Edit workflow",
            Some(GitWorkflowRunState::NeedsAttention)
                if latest_run
                    .as_ref()
                    .and_then(|run| run.pull_request_number)
                    .is_some() =>
            {
                "Review merge"
            }
            Some(GitWorkflowRunState::NeedsAttention) => "Edit workflow",
            Some(
                GitWorkflowRunState::Merged
                | GitWorkflowRunState::Closed
                | GitWorkflowRunState::Failed,
            ) => "Run again",
            None => "Review and run",
        };
        let action_run = latest_run.clone();
        v_flex()
            .id(("git-workflow-card", index))
            .w_full()
            .gap_2()
            .rounded(crate::ui::design::r_md())
            .bg(crate::ui::design::surface(cx))
            .px_3()
            .py_2p5()
            .child(
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .id(("git-workflow-name", index))
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .truncate()
                                    .tooltip({
                                        let name = workflow.name.clone();
                                        move |window, cx| {
                                            Tooltip::new(name.clone()).build(window, cx)
                                        }
                                    })
                                    .child(workflow.name.clone()),
                            )
                            .child(
                                h_flex()
                                    .min_w(px(0.))
                                    .gap_1p5()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(div().min_w(px(0.)).truncate().child(route.clone()))
                                    .child("·")
                                    .child(workflow_policy_label(workflow.completion_policy)),
                            ),
                    )
                    .child(
                        crate::ui::style::header_icon_button(
                            ("git-workflow-actions", index),
                            IconName::Ellipsis,
                            cx,
                        )
                        .tooltip(format!("Actions for {}", workflow.name))
                        .dropdown_menu(move |menu, _, _| {
                            let panel_edit = menu_panel.clone();
                            let panel_delete = menu_panel.clone();
                            let panel_up = menu_panel.clone();
                            let panel_down = menu_panel.clone();
                            let workspace = workspace.clone();
                            let edit_workflow = edit_workflow.clone();
                            let delete_workflow = delete_workflow.clone();
                            menu.item(PopupMenuItem::new("Edit").on_click(move |_, window, cx| {
                                let workflow = edit_workflow.clone();
                                panel_edit.update(cx, |panel, cx| {
                                    panel.open_edit_workflow(workflow, window, cx)
                                });
                            }))
                            .item(
                                PopupMenuItem::new("Duplicate")
                                    .icon(IconName::Copy)
                                    .on_click(move |_, _, cx| {
                                        if let Some(project_id) = project_id {
                                            workspace.update(cx, |workspace, cx| {
                                                workspace
                                                    .duplicate_git_workflow(
                                                        project_id,
                                                        duplicate_id,
                                                        cx,
                                                    )
                                                    .ok();
                                            });
                                        }
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("Move up")
                                    .icon(IconName::ArrowUp)
                                    .disabled(index == 0)
                                    .on_click(move |_, _, cx| {
                                        panel_up.update(cx, |panel, cx| {
                                            panel.move_workflow(move_up_id, -1, cx)
                                        });
                                    }),
                            )
                            .item(
                                PopupMenuItem::new("Move down")
                                    .icon(IconName::ArrowDown)
                                    .disabled(index + 1 >= total)
                                    .on_click(move |_, _, cx| {
                                        panel_down.update(cx, |panel, cx| {
                                            panel.move_workflow(move_down_id, 1, cx)
                                        });
                                    }),
                            )
                            .separator()
                            .item(
                                PopupMenuItem::new("Delete")
                                    .icon(IconName::Delete)
                                    .on_click(move |_, window, cx| {
                                        let workflow = delete_workflow.clone();
                                        panel_delete.update(cx, |panel, cx| {
                                            panel.confirm_delete_workflow(workflow, window, cx)
                                        });
                                    }),
                            )
                        }),
                    ),
            )
            .when_some(
                latest_run.as_ref().and_then(|run| run.error.clone()),
                |card, error| {
                    card.child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::rose(cx))
                            .child(error),
                    )
                },
            )
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(status_color)
                            .child(
                                gpui_component::Icon::new(
                                    match latest_run.as_ref().map(|run| run.state) {
                                        Some(GitWorkflowRunState::Merged) => IconName::CircleCheck,
                                        Some(
                                            GitWorkflowRunState::Blocked
                                            | GitWorkflowRunState::NeedsAttention
                                            | GitWorkflowRunState::Failed,
                                        ) => IconName::TriangleAlert,
                                        Some(GitWorkflowRunState::CreatingPullRequest) => {
                                            IconName::Loader
                                        }
                                        _ => IconName::Network,
                                    },
                                )
                                .size(crate::ui::design::icon_sm()),
                            )
                            .child(status),
                    )
                    .child(if busy {
                        crate::ui::style::busy_button_compact(
                            ("git-workflow-primary", index),
                            action_label,
                            cx,
                        )
                    } else {
                        crate::ui::style::primary_button_compact(
                            ("git-workflow-primary", index),
                            action_label,
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            match action_run.as_ref().map(|run| run.state) {
                                Some(
                                    GitWorkflowRunState::WaitingForRequirements
                                    | GitWorkflowRunState::AutoMergeEnabled,
                                ) => {
                                    action_panel.update(cx, |panel, cx| {
                                        panel.set_active_tab(GitTab::PullRequests, cx)
                                    });
                                }
                                Some(GitWorkflowRunState::AwaitingConfirmation) => {
                                    if let Some(run) = action_run.clone() {
                                        action_panel.update(cx, |panel, cx| {
                                            panel.confirm_merge_workflow_run(run, window, cx)
                                        });
                                    }
                                }
                                Some(GitWorkflowRunState::NeedsAttention)
                                    if action_run
                                        .as_ref()
                                        .and_then(|run| run.pull_request_number)
                                        .is_some() =>
                                {
                                    if let Some(run) = action_run.clone() {
                                        action_panel.update(cx, |panel, cx| {
                                            panel.confirm_merge_workflow_run(run, window, cx)
                                        });
                                    }
                                }
                                Some(
                                    GitWorkflowRunState::Blocked
                                    | GitWorkflowRunState::NeedsAttention,
                                ) => {
                                    let workflow = run_workflow.clone();
                                    action_panel.update(cx, |panel, cx| {
                                        panel.open_edit_workflow(workflow, window, cx)
                                    });
                                }
                                _ => {
                                    let workflow = run_workflow.clone();
                                    action_panel.update(cx, |panel, cx| {
                                        panel.open_saved_workflow_review(workflow, window, cx)
                                    });
                                }
                            }
                        })
                    }),
            )
    }

    fn render_recent_run(
        &self,
        run: GitWorkflowRun,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (status, color) = workflow_state_style(run.state, cx);
        let panel = cx.entity();
        let action_run = run.clone();
        h_flex()
            .id(("git-workflow-run", index))
            .w_full()
            .min_w(px(0.))
            .items_center()
            .gap_2()
            .py_1p5()
            .child(
                gpui_component::Icon::new(if run.state == GitWorkflowRunState::Merged {
                    IconName::CircleCheck
                } else if matches!(
                    run.state,
                    GitWorkflowRunState::Blocked
                        | GitWorkflowRunState::NeedsAttention
                        | GitWorkflowRunState::Failed
                ) {
                    IconName::TriangleAlert
                } else {
                    IconName::Network
                })
                .size(crate::ui::design::icon_md())
                .text_color(color),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_0p5()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(format!(
                                "{} → {}{}",
                                run.source_branch,
                                run.destination_branch,
                                run.pull_request_number
                                    .map(|number| format!(" · #{number}"))
                                    .unwrap_or_default()
                            )),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(if run.error.is_some() {
                                crate::ui::design::rose(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .truncate()
                            .child(run.error.clone().unwrap_or_else(|| status.to_string())),
                    ),
            )
            .when(
                matches!(
                    run.state,
                    GitWorkflowRunState::AwaitingConfirmation | GitWorkflowRunState::NeedsAttention
                ) && run.pull_request_number.is_some(),
                |row| {
                    row.child(
                        crate::ui::style::secondary_button_compact(
                            ("git-workflow-run-action", index),
                            "Review merge",
                        )
                        .on_click(move |_, window, cx| {
                            let run = action_run.clone();
                            panel.update(cx, |panel, cx| {
                                panel.confirm_merge_workflow_run(run, window, cx)
                            })
                        }),
                    )
                },
            )
    }

    pub(super) fn render_workflows(
        &self,
        git: Entity<GitState>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some((_project_id, _repository_path, workflows, mut runs)) =
            self.workflow_data_for_selected_repo(cx)
        else {
            return div().into_any_element();
        };
        let github_remote = git
            .read(cx)
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.primary_remote.clone())
            .is_some_and(|remote| remote.is_github_https());
        let repository_selector = self.render_workflow_repository_selector(cx);
        let panel_new = cx.entity();
        let panel_run = cx.entity();
        if !github_remote {
            return v_flex()
                .size_full()
                .child(
                    h_flex()
                        .w_full()
                        .min_h(crate::ui::design::subhead_h())
                        .px_3()
                        .items_center()
                        .when_some(repository_selector, |row, selector| row.child(selector)),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .px_5()
                        .gap_2()
                        .child(
                            gpui_component::Icon::new(IconName::GitHub)
                                .size(crate::ui::design::icon_xl())
                                .text_color(crate::ui::design::t3(cx)),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Git Workflows require GitHub"),
                        )
                        .child(
                            div()
                                .max_w(px(320.))
                                .text_center()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child("Choose a repository with a secure GitHub origin and connect its GitHub CLI account."),
                        ),
                )
                .into_any_element();
        }

        let retry_available = self.workflow_runs_error.is_some()
            || runs.iter().any(|run| {
                run.error
                    .as_deref()
                    .is_some_and(|error| error.starts_with("Last refresh failed;"))
            });
        runs.sort_by_key(|run| std::cmp::Reverse(run.updated_at));
        let recent_runs: Vec<GitWorkflowRun> = runs.iter().take(12).cloned().collect();
        let latest_by_workflow: HashMap<uuid::Uuid, GitWorkflowRun> = runs
            .iter()
            .filter_map(|run| run.workflow_id.map(|id| (id, run.clone())))
            .fold(HashMap::new(), |mut map, (id, run)| {
                map.entry(id).or_insert(run);
                map
            });

        let content = if workflows.is_empty() && recent_runs.is_empty() {
            let panel_new = panel_new.clone();
            let panel_run = panel_run.clone();
            v_flex()
                .id("git-workflows-scroll")
                .flex_1()
                .items_center()
                .justify_center()
                .px_5()
                .gap_3()
                .child(
                    gpui_component::Icon::new(IconName::Network)
                        .size(crate::ui::design::icon_xl())
                        .text_color(crate::ui::design::accent(cx)),
                )
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t1(cx))
                        .child("Promote committed work safely"),
                )
                .child(
                    div()
                        .max_w(px(340.))
                        .text_center()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .child("Git Workflows move work between remote branches through pull requests. Your local branch, files, index, and refs stay untouched."),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            crate::ui::style::dialog_neutral_button(
                                "empty-run-once-workflow",
                                "Run once",
                                cx,
                            )
                            .on_click(move |_, window, cx| {
                                panel_run.update(cx, |panel, cx| {
                                    panel.open_run_once(window, cx)
                                })
                            }),
                        )
                        .child(
                            crate::ui::style::primary_button_compact(
                                "empty-new-workflow",
                                "New workflow",
                                cx,
                            )
                            .icon(IconName::Plus)
                            .on_click(move |_, window, cx| {
                                panel_new.update(cx, |panel, cx| {
                                    panel.open_new_workflow(window, cx)
                                })
                            }),
                        ),
                )
                .into_any_element()
        } else {
            v_flex()
                .id("git-workflows-list-scroll")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px_2()
                .pb_3()
                .when(!workflows.is_empty(), |content| {
                    content
                        .child(
                            div()
                                .px_1()
                                .pt_3()
                                .pb_1p5()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t2(cx))
                                .child("Saved workflows"),
                        )
                        .child(
                            v_flex()
                                .gap_2()
                                .children(workflows.iter().cloned().enumerate().map(
                                    |(index, workflow)| {
                                        self.render_saved_workflow_card(
                                            workflow.clone(),
                                            latest_by_workflow.get(&workflow.id).cloned(),
                                            index,
                                            workflows.len(),
                                            cx,
                                        )
                                    },
                                )),
                        )
                })
                .when(!recent_runs.is_empty(), |content| {
                    content
                        .child(
                            div()
                                .px_1()
                                .pt_4()
                                .pb_1()
                                .text_size(crate::ui::design::text_ui())
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(crate::ui::design::t2(cx))
                                .child("Current and recent runs"),
                        )
                        .child(
                            v_flex().px_1().children(
                                recent_runs
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, run)| self.render_recent_run(run, index, cx)),
                            ),
                        )
                })
                .into_any_element()
        };

        let panel_run = cx.entity();
        let panel_new = cx.entity();
        let panel_retry = cx.entity();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .min_h(crate::ui::design::subhead_h())
                    .px_2()
                    .gap_2()
                    .items_center()
                    .when_some(repository_selector, |row, selector| row.child(selector))
                    .child(div().flex_1())
                    .when(self.workflow_runs_refreshing, |row| {
                        row.child(Spinner::new().xsmall())
                    })
                    .when(retry_available, |row| {
                        row.child(
                            crate::ui::style::refresh_icon_button("retry-git-workflow-status", cx)
                                .tooltip("Retry GitHub workflow status")
                                .disabled(self.workflow_runs_refreshing)
                                .on_click(move |_, _, cx| {
                                    panel_retry.update(cx, |panel, cx| {
                                        panel.workflow_runs_checked_at = None;
                                        if let Some(git) = panel.active_git(cx) {
                                            panel.sync_workflow_runs(git, cx);
                                        }
                                    });
                                }),
                        )
                    })
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "run-once-git-workflow",
                            "Run once",
                            cx,
                        )
                        .on_click(move |_, window, cx| {
                            panel_run.update(cx, |panel, cx| panel.open_run_once(window, cx))
                        }),
                    )
                    .child(
                        crate::ui::style::primary_button_compact(
                            "new-git-workflow",
                            "New workflow",
                            cx,
                        )
                        .icon(IconName::Plus)
                        .on_click(move |_, window, cx| {
                            panel_new.update(cx, |panel, cx| panel.open_new_workflow(window, cx))
                        }),
                    ),
            )
            .when_some(self.workflow_runs_error.clone(), |content, error| {
                content.child(
                    div()
                        .mx_3()
                        .mb_1()
                        .rounded(crate::ui::design::r_sm())
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_2()
                        .py_1()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(content)
            .into_any_element()
    }
}
