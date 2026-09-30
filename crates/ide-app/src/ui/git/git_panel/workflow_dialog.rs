use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkflowDialogMode {
    EditDefinition,
    RunOnce,
    RunSaved,
}

pub(super) struct WorkflowDialog {
    workspace: Entity<Workspace>,
    project_id: ProjectId,
    repo_path: PathBuf,
    repository_path: PathBuf,
    mode: WorkflowDialogMode,
    workflow_id: Option<uuid::Uuid>,
    name_input: Entity<InputState>,
    title_input: Entity<InputState>,
    body_input: Entity<InputState>,
    source_branch: String,
    destination_branch: String,
    completion_policy: GitWorkflowCompletionPolicy,
    branches: Vec<workflow_support::RemoteWorkflowBranch>,
    branches_loading: bool,
    review_loading: bool,
    starting: bool,
    review: Option<workflow_support::WorkflowReview>,
    suggested_name: Option<String>,
    error: Option<String>,
}

impl WorkflowDialog {
    #[allow(clippy::too_many_arguments)]
    fn open(
        workspace: Entity<Workspace>,
        project_id: ProjectId,
        repo_path: PathBuf,
        repository_path: PathBuf,
        mode: WorkflowDialogMode,
        workflow: Option<GitWorkflow>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let name_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. Staging to production"));
        let title_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Pull request title"));
        let body_input = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .placeholder("Pull request description")
        });
        let (workflow_id, name, source_branch, destination_branch, completion_policy) = workflow
            .map(|workflow| {
                (
                    Some(workflow.id),
                    workflow.name,
                    workflow.source_branch,
                    workflow.destination_branch,
                    workflow.completion_policy,
                )
            })
            .unwrap_or((
                None,
                String::new(),
                String::new(),
                String::new(),
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
            ));
        name_input.update(cx, |input, cx| input.set_value(name, window, cx));

        let dialog = cx.new(|_| Self {
            workspace,
            project_id,
            repo_path,
            repository_path,
            mode,
            workflow_id,
            name_input,
            title_input,
            body_input,
            source_branch,
            destination_branch,
            completion_policy,
            branches: Vec::new(),
            branches_loading: true,
            review_loading: false,
            starting: false,
            review: None,
            suggested_name: None,
            error: None,
        });
        let title = match mode {
            WorkflowDialogMode::EditDefinition if workflow_id.is_some() => "Edit workflow",
            WorkflowDialogMode::EditDefinition => "New workflow",
            WorkflowDialogMode::RunOnce => "Run once",
            WorkflowDialogMode::RunSaved => "Review workflow",
        };
        window.open_dialog(cx, {
            let dialog = dialog.clone();
            move |dialog_view, _, _| {
                dialog_view
                    .title(title)
                    .w(px(660.))
                    .overlay_closable(false)
                    .child(dialog.clone())
            }
        });
        dialog.update(cx, |dialog, cx| {
            dialog.load_branches(window, cx);
        });
    }

    fn load_branches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let repo_path = self.repo_path.clone();
        let name_input = self.name_input.clone();
        let mode = self.mode;
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { workflow_support::list_remote_workflow_branches(&repo_path) })
                .await;
            let should_prepare_saved_review = this.update(cx, |dialog, cx| {
                dialog.branches_loading = false;
                match result {
                    Ok(branches) => {
                        dialog.branches = branches;
                        if dialog.destination_branch.is_empty() {
                            dialog.destination_branch = dialog
                                .branches
                                .iter()
                                .find(|branch| matches!(branch.name.as_str(), "main" | "master"))
                                .or_else(|| dialog.branches.first())
                                .map(|branch| branch.name.clone())
                                .unwrap_or_default();
                        }
                        if dialog.source_branch.is_empty() {
                            dialog.source_branch = dialog
                                .branches
                                .iter()
                                .find(|branch| branch.name != dialog.destination_branch)
                                .map(|branch| branch.name.clone())
                                .unwrap_or_default();
                        }
                        if mode == WorkflowDialogMode::EditDefinition
                            && name_input.read(cx).value().trim().is_empty()
                            && !dialog.source_branch.is_empty()
                            && !dialog.destination_branch.is_empty()
                        {
                            let suggestion = format!(
                                "{} to {}",
                                dialog.source_branch, dialog.destination_branch
                            );
                            dialog.suggested_name = Some(suggestion.clone());
                            window_handle
                                .update(cx, |_, window, cx| {
                                    name_input.update(cx, |input, cx| {
                                        input.set_value(suggestion.clone(), window, cx)
                                    });
                                })
                                .ok();
                        }
                        dialog.error = workflow_support::missing_workflow_branch(
                            &dialog.branches,
                            &dialog.source_branch,
                            &dialog.destination_branch,
                        );
                    }
                    Err(error) => dialog.error = Some(format!("{error:#}")),
                }
                cx.notify();
                mode == WorkflowDialogMode::RunSaved && dialog.error.is_none()
            });
            if matches!(should_prepare_saved_review, Ok(true)) {
                window_handle
                    .update(cx, |_, window, cx| {
                        this.update(cx, |dialog, cx| dialog.prepare_review(window, cx))
                            .ok();
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn update_suggested_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.mode != WorkflowDialogMode::EditDefinition
            || self.source_branch.is_empty()
            || self.destination_branch.is_empty()
        {
            return;
        }
        let current = self.name_input.read(cx).value().to_string();
        if current.trim().is_empty()
            || self
                .suggested_name
                .as_deref()
                .is_some_and(|suggestion| current == suggestion)
        {
            let suggestion = format!("{} to {}", self.source_branch, self.destination_branch);
            self.name_input.update(cx, |input, cx| {
                input.set_value(suggestion.clone(), window, cx)
            });
            self.suggested_name = Some(suggestion);
        }
    }

    fn save_definition(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(error) = workflow_support::missing_workflow_branch(
            &self.branches,
            &self.source_branch,
            &self.destination_branch,
        ) {
            self.error = Some(error);
            cx.notify();
            return;
        }
        let name = self.name_input.read(cx).value().to_string();
        let mut workflow = match GitWorkflow::new(
            self.repository_path.clone(),
            name,
            self.source_branch.clone(),
            self.destination_branch.clone(),
            self.completion_policy,
        ) {
            Ok(workflow) => workflow,
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                cx.notify();
                return;
            }
        };
        if let Some(id) = self.workflow_id {
            workflow.id = id;
        }
        let result = self.workspace.update(cx, |workspace, cx| {
            if self.workflow_id.is_some() {
                workspace.update_git_workflow(self.project_id, workflow, cx)
            } else {
                workspace
                    .add_git_workflow(self.project_id, workflow, cx)
                    .map(|_| ())
            }
        });
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                cx.notify();
            }
        }
    }

    fn prepare_review(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.review_loading || self.starting {
            return;
        }
        if self.source_branch.is_empty()
            || self.destination_branch.is_empty()
            || self.source_branch == self.destination_branch
        {
            self.error = Some("Choose two different remote branches".into());
            cx.notify();
            return;
        }
        if let Some(error) = workflow_support::missing_workflow_branch(
            &self.branches,
            &self.source_branch,
            &self.destination_branch,
        ) {
            self.error = Some(error);
            cx.notify();
            return;
        }
        self.review_loading = true;
        self.error = None;
        let repo_path = self.repo_path.clone();
        let source = self.source_branch.clone();
        let destination = self.destination_branch.clone();
        let requested_source = source.clone();
        let requested_destination = destination.clone();
        let workflow_name = self.name_input.read(cx).value().to_string();
        let title_input = self.title_input.clone();
        let body_input = self.body_input.clone();
        let window_handle = window.window_handle();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    workflow_support::review_remote_workflow(&repo_path, &source, &destination)
                })
                .await;
            this.update(cx, |dialog, cx| {
                dialog.review_loading = false;
                if dialog.source_branch != requested_source
                    || dialog.destination_branch != requested_destination
                {
                    dialog.review = None;
                    cx.notify();
                    return;
                }
                match result {
                    Ok(review) => {
                        if review.existing_pull_request.is_none() {
                            let title = if workflow_name.trim().is_empty() {
                                format!(
                                    "Merge {} into {}",
                                    dialog.source_branch, dialog.destination_branch
                                )
                            } else {
                                workflow_name.trim().to_string()
                            };
                            let text = workflow_support::deterministic_pull_request_text(
                                title,
                                &dialog.source_branch,
                                &dialog.destination_branch,
                                &review.comparison,
                            );
                            window_handle
                                .update(cx, |_, window, cx| {
                                    title_input.update(cx, |input, cx| {
                                        input.set_value(text.title.clone(), window, cx)
                                    });
                                    body_input.update(cx, |input, cx| {
                                        input.set_value(text.body.clone(), window, cx)
                                    });
                                })
                                .ok();
                        }
                        dialog.review = Some(review);
                        dialog.error = None;
                    }
                    Err(error) => dialog.error = Some(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn start_workflow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.starting {
            return;
        }
        let Some(review) = self.review.clone() else {
            return;
        };
        if !review.comparison.has_commits_to_merge() {
            self.error = Some(format!(
                "{} already contains {}",
                self.destination_branch, self.source_branch
            ));
            cx.notify();
            return;
        }
        let pull_request = if review.existing_pull_request.is_none() {
            let title = self.title_input.read(cx).value().to_string();
            let body = self.body_input.read(cx).value().to_string();
            if title.trim().is_empty() || body.trim().is_empty() {
                self.error = Some("Enter a pull request title and description".into());
                cx.notify();
                return;
            }
            GeneratedPullRequest { title, body }
        } else {
            GeneratedPullRequest {
                title: String::new(),
                body: String::new(),
            }
        };
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let mut run = GitWorkflowRun {
            id: uuid::Uuid::new_v4(),
            workflow_id: self.workflow_id,
            repository_path: self.repository_path.clone(),
            source_branch: self.source_branch.clone(),
            destination_branch: self.destination_branch.clone(),
            pull_request_number: None,
            expected_head_sha: Some(review.comparison.source_sha.clone()),
            state: GitWorkflowRunState::CreatingPullRequest,
            error: None,
            started_at: now,
            updated_at: now,
        };
        if let Err(error) = self.workspace.update(cx, |workspace, cx| {
            workspace.upsert_git_workflow_run(self.project_id, run.clone(), cx)
        }) {
            self.error = Some(format!("{error:#}"));
            cx.notify();
            return;
        }
        self.starting = true;
        self.error = None;
        cx.notify();
        let repo_path = self.repo_path.clone();
        let policy = self.completion_policy;
        let project_id = self.project_id;
        let source = self.source_branch.clone();
        let destination = self.destination_branch.clone();
        let workspace = self.workspace.clone();
        let window_handle = window.window_handle();
        let mut failed_run = run.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let pr = workflow_support::create_or_reuse_workflow_pull_request(
                        &repo_path,
                        &source,
                        &destination,
                        &pull_request,
                    )?;
                    workflow_support::validate_pull_request_matches_review(
                        &pr,
                        run.expected_head_sha.as_deref(),
                    )?;
                    run.pull_request_number = Some(pr.number);
                    if policy == GitWorkflowCompletionPolicy::AutoMergeWhenReady {
                        match workflow_support::enable_workflow_auto_merge(
                            &repo_path,
                            pr.number,
                            &source,
                            &destination,
                            run.expected_head_sha.as_deref().unwrap_or_default(),
                        ) {
                            Ok(updated_pr) => {
                                let reconciliation =
                                    workflow_support::reconcile_workflow_pull_request(
                                        Some(&updated_pr),
                                        run.expected_head_sha.as_deref(),
                                        policy,
                                    );
                                run.state = if reconciliation.state == GitWorkflowRunState::Merged {
                                    GitWorkflowRunState::Merged
                                } else {
                                    GitWorkflowRunState::AutoMergeEnabled
                                };
                                run.error = reconciliation.error;
                            }
                            Err(error) => {
                                let reconciliation =
                                    workflow_support::auto_merge_unavailable(&error);
                                run.state = reconciliation.state;
                                run.error = reconciliation.error;
                            }
                        }
                    } else {
                        let reconciliation = workflow_support::reconcile_workflow_pull_request(
                            Some(&pr),
                            run.expected_head_sha.as_deref(),
                            policy,
                        );
                        run.state = reconciliation.state;
                        run.error = reconciliation.error;
                    }
                    run.updated_at = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or(run.updated_at);
                    Ok::<_, anyhow::Error>(run)
                })
                .await;
            this.update(cx, |dialog, cx| {
                dialog.starting = false;
                match result {
                    Ok(run) => {
                        let state = run.state;
                        let number = run.pull_request_number;
                        let save = workspace.update(cx, |workspace, cx| {
                            workspace.upsert_git_workflow_run(project_id, run, cx)
                        });
                        if let Err(error) = save {
                            dialog.error = Some(format!("{error:#}"));
                            cx.notify();
                            return;
                        }
                        window_handle
                            .update(cx, |_, window, cx| {
                                window.close_dialog(cx);
                                let message = match state {
                                    GitWorkflowRunState::AutoMergeEnabled => {
                                        format!(
                                            "Auto-merge enabled for PR #{}",
                                            number.unwrap_or(0)
                                        )
                                    }
                                    GitWorkflowRunState::AwaitingConfirmation => format!(
                                        "PR #{} is ready for merge review",
                                        number.unwrap_or(0)
                                    ),
                                    _ => {
                                        format!("Workflow started with PR #{}", number.unwrap_or(0))
                                    }
                                };
                                window.push_notification(Notification::success(message), cx);
                            })
                            .ok();
                    }
                    Err(error) => {
                        failed_run.state = GitWorkflowRunState::Failed;
                        failed_run.error = Some(format!("{error:#}"));
                        failed_run.updated_at = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map(|duration| duration.as_secs())
                            .unwrap_or(failed_run.updated_at);
                        workspace.update(cx, |workspace, cx| {
                            workspace
                                .upsert_git_workflow_run(project_id, failed_run.clone(), cx)
                                .ok();
                        });
                        dialog.review = None;
                        dialog.error = Some(format!("{error:#}"));
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn branch_selector(
        &self,
        id: &'static str,
        label: &'static str,
        source: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let value = if source {
            self.source_branch.clone()
        } else {
            self.destination_branch.clone()
        };
        let branches = self.branches.clone();
        let dialog = cx.entity();
        v_flex()
            .flex_1()
            .min_w(px(0.))
            .gap_1()
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t2(cx))
                    .child(label),
            )
            .child(
                crate::ui::style::dialog_neutral_button(
                    id,
                    if value.is_empty() {
                        "Choose branch".to_string()
                    } else {
                        value.clone()
                    },
                    cx,
                )
                .w_full()
                .disabled(self.branches_loading || self.review_loading || self.starting)
                .dropdown_menu(move |menu, _, _| {
                    branches.iter().fold(menu, |menu, branch| {
                        let name = branch.name.clone();
                        let menu_label = if branch.protected {
                            format!("{} · protected", branch.name)
                        } else {
                            branch.name.clone()
                        };
                        let selected = name == value;
                        let dialog = dialog.clone();
                        menu.item(PopupMenuItem::new(menu_label).checked(selected).on_click(
                            move |_, window, cx| {
                                dialog.update(cx, |dialog, cx| {
                                    if source {
                                        dialog.source_branch = name.clone();
                                    } else {
                                        dialog.destination_branch = name.clone();
                                    }
                                    dialog.review = None;
                                    dialog.error = workflow_support::missing_workflow_branch(
                                        &dialog.branches,
                                        &dialog.source_branch,
                                        &dialog.destination_branch,
                                    );
                                    dialog.update_suggested_name(window, cx);
                                    cx.notify();
                                });
                            },
                        ))
                    })
                }),
            )
    }

    fn policy_choice(
        &self,
        policy: GitWorkflowCompletionPolicy,
        title: &'static str,
        description: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected = self.completion_policy == policy;
        crate::ui::style::dialog_choice_card_button(
            match policy {
                GitWorkflowCompletionPolicy::ConfirmBeforeMerge => "workflow-policy-confirm",
                GitWorkflowCompletionPolicy::AutoMergeWhenReady => "workflow-policy-auto",
            },
            selected,
            cx,
        )
        .disabled(self.starting)
        .on_click(cx.listener(move |dialog, _, _, cx| {
            dialog.completion_policy = policy;
            dialog.error = None;
            cx.notify();
        }))
        .child(
            h_flex()
                .w_full()
                .h_full()
                .items_start()
                .gap_2()
                .px_3()
                .py_2p5()
                .child(
                    gpui_component::Icon::new(if selected {
                        IconName::CircleCheck
                    } else {
                        IconName::CircleUser
                    })
                    .size(crate::ui::design::icon_md())
                    .text_color(if selected {
                        crate::ui::design::accent(cx)
                    } else {
                        crate::ui::design::t3(cx)
                    }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(description),
                        ),
                ),
        )
    }

    fn render_review(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(review) = self.review.as_ref() else {
            return div().into_any_element();
        };
        let comparison = &review.comparison;
        let existing = review.existing_pull_request.as_ref();
        let no_commits_to_merge = !comparison.has_commits_to_merge();
        let (status_label, status_color) = if no_commits_to_merge {
            ("No changes".into(), crate::ui::design::sage(cx))
        } else {
            existing
                .map(|pr| pull_request_status_style(pr, cx))
                .unwrap_or_else(|| ("New pull request".into(), crate::ui::design::sky(cx)))
        };
        v_flex()
            .w_full()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .rounded(crate::ui::design::r_md())
                    .bg(crate::ui::design::surface(cx))
                    .px_3()
                    .py_2p5()
                    .child(
                        v_flex()
                            .min_w(px(0.))
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(format!(
                                        "{} → {}",
                                        self.source_branch, self.destination_branch
                                    )),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(format!(
                                        "Source {}",
                                        comparison.source_sha.chars().take(10).collect::<String>()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(status_color)
                            .child(status_label),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .gap_4()
                    .children([
                        ("Commits", comparison.total_commits.to_string()),
                        ("Changed files", comparison.changed_files.to_string()),
                        ("Behind", comparison.behind_by.to_string()),
                    ].into_iter().map(|(label, value)| {
                        v_flex()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(value),
                            )
                    })),
            )
            .when(no_commits_to_merge, |content| {
                content.child(
                    h_flex()
                        .w_full()
                        .items_start()
                        .gap_2()
                        .rounded(crate::ui::design::r_md())
                        .bg(crate::ui::design::sage(cx).opacity(0.08))
                        .px_3()
                        .py_2p5()
                        .child(
                            gpui_component::Icon::new(IconName::CircleCheck)
                                .size(crate::ui::design::icon_md())
                                .text_color(crate::ui::design::sage(cx)),
                        )
                        .child(
                            v_flex()
                                .min_w(px(0.))
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child(format!(
                                            "{} already contains {}",
                                            self.destination_branch, self.source_branch
                                        )),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t2(cx))
                                        .child("There are no remote commits to merge, so Choro won't open a pull request. Go back to choose another route."),
                                ),
                        ),
                )
            })
            .when(!no_commits_to_merge, |content| {
                content
                    .when_some(existing, |content, pr| {
                        let checks = match pr.check_state {
                            PullRequestCheckState::Passing => "Passing",
                            PullRequestCheckState::Pending => "Pending",
                            PullRequestCheckState::Failing => "Failing",
                            PullRequestCheckState::Unknown => "Not reported",
                        };
                        let mergeability = pr
                            .merge_state_status
                            .as_deref()
                            .unwrap_or("Not reported")
                            .replace('_', " ")
                            .to_ascii_lowercase();
                        let reviews = pr
                            .review_decision
                            .as_deref()
                            .unwrap_or("Not reported")
                            .replace('_', " ")
                            .to_ascii_lowercase();
                        content.child(
                            v_flex()
                                .gap_2()
                                .child(
                                    h_flex()
                                        .w_full()
                                        .gap_4()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child(format!("Mergeability: {mergeability}"))
                                        .child(format!("Checks: {checks}"))
                                        .child(format!("Reviews: {reviews}"))
                                        .child(if pr.is_draft {
                                            "Draft"
                                        } else {
                                            "Ready for review"
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(format!("Continue pull request #{}", pr.number)),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_body())
                                        .text_color(crate::ui::design::t2(cx))
                                        .child(pr.title.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Choro will reuse this exact route without replacing its title or description."),
                                ),
                        )
                    })
                    .when(existing.is_none(), |content| {
                        content
                    .child(
                        h_flex()
                            .w_full()
                            .gap_4()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child("Mergeability: evaluated when PR opens")
                            .child("Checks: not started")
                            .child("Reviews: not started")
                            .child("Draft: no"),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Pull request title"),
                            )
                            .child(Input::new(&self.title_input)),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Pull request description"),
                            )
                            .child(Input::new(&self.body_input).w_full().h(px(150.))),
                    )
                    })
            })
            .into_any_element()
    }
}

impl Render for WorkflowDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let reviewing = self.review.is_some();
        let no_commits_to_merge = self
            .review
            .as_ref()
            .is_some_and(|review| !review.comparison.has_commits_to_merge());
        let missing_branch = !self.branches_loading
            && workflow_support::missing_workflow_branch(
                &self.branches,
                &self.source_branch,
                &self.destination_branch,
            )
            .is_some();
        let can_continue = !self.branches_loading
            && !self.review_loading
            && !self.starting
            && !self.source_branch.is_empty()
            && !self.destination_branch.is_empty()
            && self.source_branch != self.destination_branch
            && !missing_branch;
        v_flex()
            .w_full()
            .gap_4()
            .when(!reviewing, |content| {
                content
                    .when(self.mode == WorkflowDialogMode::EditDefinition, |content| {
                        content.child(
                            v_flex()
                                .w_full()
                                .gap_1()
                                .child(
                                    div()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Workflow name"),
                                )
                                .child(Input::new(&self.name_input)),
                        )
                    })
                    .child(
                        h_flex()
                            .w_full()
                            .items_end()
                            .gap_3()
                            .child(self.branch_selector("workflow-source", "Source", true, cx))
                            .child(
                                gpui_component::Icon::new(IconName::ArrowRight)
                                    .size(crate::ui::design::icon_md())
                                    .text_color(crate::ui::design::t3(cx)),
                            )
                            .child(self.branch_selector(
                                "workflow-destination",
                                "Destination",
                                false,
                                cx,
                            )),
                    )
                    .when(self.branches_loading, |content| {
                        content.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(Spinner::new().xsmall())
                                .child("Loading remote branches from GitHub…"),
                        )
                    })
                    .child(
                        v_flex()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_size(crate::ui::design::text_ui())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("When requirements are satisfied"),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .child(self.policy_choice(
                                        GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
                                        "Confirm before merge",
                                        "Choro waits for your final review and explicit merge confirmation.",
                                        cx,
                                    ))
                                    .child(self.policy_choice(
                                        GitWorkflowCompletionPolicy::AutoMergeWhenReady,
                                        "Auto-merge when ready",
                                        "GitHub merges after its checks, reviews, queue, and rules allow it.",
                                        cx,
                                    )),
                            ),
                    )
            })
            .when(reviewing, |content| content.child(self.render_review(cx)))
            .when(self.review_loading, |content| {
                content.child(
                    h_flex()
                        .w_full()
                        .py_6()
                        .justify_center()
                        .items_center()
                        .gap_2()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child(Spinner::new().small())
                        .child("Comparing remote branches and checking for an exact PR…"),
                )
            })
            .when_some(self.error.clone(), |content, error| {
                content.child(
                    v_flex()
                        .w_full()
                        .gap_1()
                        .rounded(crate::ui::design::r_md())
                        .bg(crate::ui::design::rose(cx).opacity(0.08))
                        .px_3()
                        .py_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child("This workflow needs attention")
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        crate::ui::style::dialog_neutral_button(
                            "cancel-git-workflow",
                            if no_commits_to_merge {
                                "Close"
                            } else {
                                "Cancel"
                            },
                            cx,
                        )
                        .disabled(self.starting)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .when(reviewing, |actions| {
                        actions.child(
                            crate::ui::style::dialog_neutral_button(
                                "back-git-workflow",
                                "Back",
                                cx,
                            )
                            .disabled(self.starting)
                            .on_click(cx.listener(|dialog, _, _, cx| {
                                dialog.review = None;
                                dialog.error = None;
                                cx.notify();
                            })),
                        )
                    })
                    .when(!no_commits_to_merge, |actions| {
                        actions.child(match self.mode {
                            WorkflowDialogMode::RunSaved if missing_branch => {
                                crate::ui::style::primary_button_compact(
                                    "edit-missing-git-workflow",
                                    "Edit workflow",
                                    cx,
                                )
                                .on_click(cx.listener(|dialog, _, _, cx| {
                                    dialog.mode = WorkflowDialogMode::EditDefinition;
                                    dialog.review = None;
                                    cx.notify();
                                }))
                            }
                            WorkflowDialogMode::EditDefinition => {
                                crate::ui::style::primary_button_compact(
                                    "save-git-workflow",
                                    "Save workflow",
                                    cx,
                                )
                                .disabled(!can_continue)
                                .on_click(cx.listener(|dialog, _, window, cx| {
                                    dialog.save_definition(window, cx)
                                }))
                            }
                            _ if reviewing => crate::ui::style::primary_button_compact(
                                "start-git-workflow",
                                if self.starting {
                                    "Starting…"
                                } else if self
                                    .review
                                    .as_ref()
                                    .is_some_and(|review| review.existing_pull_request.is_some())
                                {
                                    "Continue workflow"
                                } else {
                                    "Start workflow"
                                },
                                cx,
                            )
                            .disabled(self.starting)
                            .on_click(cx.listener(|dialog, _, window, cx| {
                                dialog.start_workflow(window, cx)
                            })),
                            _ => crate::ui::style::primary_button_compact(
                                "review-git-workflow",
                                "Review remote changes",
                                cx,
                            )
                            .disabled(!can_continue)
                            .on_click(cx.listener(|dialog, _, window, cx| {
                                dialog.prepare_review(window, cx)
                            })),
                        })
                    }),
            )
    }
}

impl GitPanel {
    pub(super) fn open_new_workflow(&self, window: &mut Window, cx: &mut App) {
        let Some((project_id, repo_path, repository_path)) = self.selected_workflow_context(cx)
        else {
            return;
        };
        WorkflowDialog::open(
            self.workspace.clone(),
            project_id,
            repo_path,
            repository_path,
            WorkflowDialogMode::EditDefinition,
            None,
            window,
            cx,
        );
    }

    pub(super) fn open_edit_workflow(
        &self,
        workflow: GitWorkflow,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((project_id, repo_path, repository_path)) = self.selected_workflow_context(cx)
        else {
            return;
        };
        WorkflowDialog::open(
            self.workspace.clone(),
            project_id,
            repo_path,
            repository_path,
            WorkflowDialogMode::EditDefinition,
            Some(workflow),
            window,
            cx,
        );
    }

    pub(super) fn open_run_once(&self, window: &mut Window, cx: &mut App) {
        let Some((project_id, repo_path, repository_path)) = self.selected_workflow_context(cx)
        else {
            return;
        };
        WorkflowDialog::open(
            self.workspace.clone(),
            project_id,
            repo_path,
            repository_path,
            WorkflowDialogMode::RunOnce,
            None,
            window,
            cx,
        );
    }

    pub(super) fn open_saved_workflow_review(
        &self,
        workflow: GitWorkflow,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some((project_id, repo_path, repository_path)) = self.selected_workflow_context(cx)
        else {
            return;
        };
        WorkflowDialog::open(
            self.workspace.clone(),
            project_id,
            repo_path,
            repository_path,
            WorkflowDialogMode::RunSaved,
            Some(workflow),
            window,
            cx,
        );
    }
}
