use super::*;

struct MultiRepoPreparationRequest {
    index: usize,
    repo: PathBuf,
    current_branch: Option<String>,
    branch_name: String,
    files: Vec<PathBuf>,
    commit_message: String,
    pr_base_branch: String,
    pr_title: String,
    pr_description: String,
}

struct MultiRepoRunRequest {
    index: usize,
    label: String,
    repo: PathBuf,
    current_branch: Option<String>,
    needs_upstream: bool,
    branch_name: String,
    files: Vec<PathBuf>,
    commit_message: String,
    pr_base_branch: String,
    pr_title: String,
    pr_description: String,
    pending_commit: Option<AgentShipPendingCommit>,
}

fn actionable_repository_indices(
    repositories: impl IntoIterator<Item = (bool, usize)>,
) -> Vec<usize> {
    repositories
        .into_iter()
        .enumerate()
        .filter_map(|(index, (completed, included_file_count))| {
            (!completed && included_file_count > 0).then_some(index)
        })
        .collect()
}

impl MultiRepoShipRepository {
    fn scope_files(&self, scope: AgentShipScope) -> &[PathBuf] {
        match scope {
            AgentShipScope::Conversation => &self.conversation_files,
            AgentShipScope::All => &self.all_files,
        }
    }

    fn included_files(&self, scope: AgentShipScope) -> Vec<PathBuf> {
        self.scope_files(scope)
            .iter()
            .filter(|path| !self.deselected.contains(*path))
            .cloned()
            .collect()
    }

    fn staged_outside_scope(&self, scope: AgentShipScope) -> Vec<PathBuf> {
        let included = self
            .included_files(scope)
            .into_iter()
            .collect::<HashSet<_>>();
        self.staged_files
            .iter()
            .filter(|path| !included.contains(*path))
            .cloned()
            .collect()
    }
}

impl MultiRepoShipDialog {
    fn start_summary_maintenance(&mut self, cx: &mut App) {
        if self.summary_maintenance_started {
            return;
        }
        self.summary_maintenance_started =
            start_ship_summary_maintenance(&self.center, self.agent_id, cx);
    }

    fn current_action(&self) -> AgentShipAction {
        if self.open_pr {
            AgentShipAction::CommitPushPr
        } else if self.push {
            AgentShipAction::CommitPush
        } else {
            AgentShipAction::Commit
        }
    }

    fn actionable_indices(&self) -> Vec<usize> {
        actionable_repository_indices(self.repositories.iter().map(|repository| {
            (
                repository.completed,
                repository.included_files(self.scope).len(),
            )
        }))
    }

    fn included_file_count(&self) -> usize {
        self.repositories
            .iter()
            .filter(|repository| !repository.completed)
            .map(|repository| repository.included_files(self.scope).len())
            .sum()
    }

    fn validation_error(&self) -> Option<String> {
        let indices = self.actionable_indices();
        if indices.is_empty() {
            return Some("No files selected for this scope.".into());
        }
        for index in indices {
            let repository = &self.repositories[index];
            if repository.branch.is_none() && !self.create_branch {
                return Some(format!(
                    "{} is on a detached HEAD. Choose New branch to ship it.",
                    repository.label
                ));
            }
            let staged_outside = repository.staged_outside_scope(self.scope);
            if !staged_outside.is_empty() {
                return Some(format!(
                    "{} has staged files outside this scope. Unstage them or choose All changes.",
                    repository.label
                ));
            }
            if self.open_pr && repository.pr_base_branch.trim().is_empty() {
                return Some(format!("Choose a base branch for {}.", repository.label));
            }
            if self.open_pr
                && !self.create_branch
                && repository.branch.as_deref().is_some_and(|branch| {
                    validate_agent_ship_pr_branches(branch, &repository.pr_base_branch).is_err()
                })
            {
                return Some(format!(
                    "{} is already on the selected base branch. Choose New branch to open a pull request.",
                    repository.label
                ));
            }
        }
        None
    }

    fn reset_prepared(&mut self) {
        if self
            .repositories
            .iter()
            .any(|repository| repository.completed)
        {
            return;
        }
        self.prepared = false;
        self.error = None;
    }

    fn preparation_requests(&self, cx: &App) -> Vec<MultiRepoPreparationRequest> {
        self.actionable_indices()
            .into_iter()
            .map(|index| {
                let repository = &self.repositories[index];
                MultiRepoPreparationRequest {
                    index,
                    repo: repository.repo_path.clone(),
                    current_branch: repository.branch.clone(),
                    branch_name: repository.branch_name.read(cx).value().trim().to_string(),
                    files: repository.included_files(self.scope),
                    commit_message: repository
                        .commit_message
                        .read(cx)
                        .value()
                        .trim()
                        .to_string(),
                    pr_base_branch: repository.pr_base_branch.trim().to_string(),
                    pr_title: repository.pr_title.read(cx).value().trim().to_string(),
                    pr_description: repository
                        .pr_description
                        .read(cx)
                        .value()
                        .trim()
                        .to_string(),
                }
            })
            .collect()
    }

    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if let Some(error) = self.validation_error() {
            self.error = Some(error);
            cx.notify();
            return;
        }
        let requests = self.preparation_requests(cx);
        let generation_agent = self.generation_agent.clone();
        let agent_title = self.agent_title.clone();
        let create_branch = self.create_branch;
        let action = self.current_action();
        let auto_ship = self.auto_ship;
        let window_handle = window.window_handle();
        self.busy = true;
        self.prepared = false;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut prepared = Vec::with_capacity(requests.len());
                    for request in requests {
                        let preparation = prepare_agent_ship_content(
                            &generation_agent,
                            &request.repo,
                            request.current_branch.as_deref(),
                            create_branch,
                            &request.branch_name,
                            &agent_title,
                            &request.files,
                            &request.commit_message,
                            &request.pr_base_branch,
                            &request.pr_title,
                            &request.pr_description,
                            action,
                        )?;
                        prepared.push((request.index, preparation));
                    }
                    anyhow::Ok(prepared)
                })
                .await;

            let should_run = this
                .update(cx, |dialog, cx| {
                    dialog.busy = false;
                    match result {
                        Ok(prepared) => {
                            window_handle
                                .update(cx, |_, window, cx| {
                                    for (index, preparation) in prepared {
                                        let repository = &dialog.repositories[index];
                                        if let Some(branch_name) = preparation.branch_name {
                                            repository.branch_name.update(cx, |input, cx| {
                                                input.set_value(branch_name, window, cx)
                                            });
                                        }
                                        repository.commit_message.update(cx, |input, cx| {
                                            input.set_value(
                                                preparation.commit_message.clone(),
                                                window,
                                                cx,
                                            )
                                        });
                                        if let Some(pr) = preparation.pr {
                                            repository.pr_title.update(cx, |input, cx| {
                                                input.set_value(pr.title, window, cx)
                                            });
                                            repository.pr_description.update(cx, |input, cx| {
                                                input.set_value(pr.body, window, cx)
                                            });
                                        }
                                    }
                                })
                                .ok();
                            dialog.prepared = true;
                            dialog.error = None;
                            crate::notifications::play_generated_sound();
                            crate::ui::onboarding::emit_for_project(
                                dialog.project_id,
                                crate::ui::onboarding::OnboardingEvent::ShipPrepared,
                                cx,
                            );
                        }
                        Err(error) => {
                            dialog.error = Some(format!(
                                "Nothing was generated. Fix this and try all repositories again: {error:#}"
                            ));
                        }
                    }
                    cx.notify();
                    auto_ship && dialog.prepared
                })
                .unwrap_or(false);
            if should_run {
                window_handle
                    .update(cx, |_, window, cx| {
                        this.update(cx, |dialog, cx| {
                            let action = dialog.current_action();
                            dialog.run(action, window, cx);
                        })
                        .ok();
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn run_requests(&self, cx: &App) -> Vec<MultiRepoRunRequest> {
        self.actionable_indices()
            .into_iter()
            .map(|index| {
                let repository = &self.repositories[index];
                MultiRepoRunRequest {
                    index,
                    label: repository.label.clone(),
                    repo: repository.repo_path.clone(),
                    current_branch: repository.branch.clone(),
                    needs_upstream: repository.needs_upstream,
                    branch_name: repository.branch_name.read(cx).value().trim().to_string(),
                    files: repository.included_files(self.scope),
                    commit_message: repository
                        .commit_message
                        .read(cx)
                        .value()
                        .trim()
                        .to_string(),
                    pr_base_branch: repository.pr_base_branch.trim().to_string(),
                    pr_title: repository.pr_title.read(cx).value().trim().to_string(),
                    pr_description: repository
                        .pr_description
                        .read(cx)
                        .value()
                        .trim()
                        .to_string(),
                    pending_commit: repository.pending_commit.clone(),
                }
            })
            .collect()
    }

    fn run(&mut self, action: AgentShipAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !self.prepared {
            self.error = Some("Generate content for all repositories first.".into());
            cx.notify();
            return;
        }
        if let Some(error) = self.validation_error() {
            self.error = Some(error);
            cx.notify();
            return;
        }
        self.start_summary_maintenance(cx);
        let requests = self.run_requests(cx);
        let total = requests.len();
        let agent_id = self.agent_id;
        let project_id = self.project_id;
        let create_branch = self.create_branch;
        let center = self.center.clone();
        let window_handle = window.window_handle();
        self.busy = true;
        self.error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::with_capacity(requests.len());
                    for request in requests {
                        let result = run_agent_ship_operation(
                            &request.repo,
                            agent_id,
                            project_id,
                            request.current_branch.as_deref(),
                            create_branch,
                            &request.branch_name,
                            request.needs_upstream,
                            &request.files,
                            &request.commit_message,
                            &request.pr_base_branch,
                            &request.pr_title,
                            &request.pr_description,
                            request.pending_commit,
                            action,
                        );
                        results.push((request.index, request.label, result));
                    }
                    results
                })
                .await;

            let mut pr_urls = Vec::new();
            let all_complete = this
                .update(cx, |dialog, cx| {
                    dialog.busy = false;
                    let mut failures = Vec::new();
                    let mut completed_now = 0usize;
                    let mut committed_now = 0usize;
                    let mut pushed_now = 0usize;
                    for (index, label, result) in results {
                        let repository = &mut dialog.repositories[index];
                        match result {
                            Ok(outcome) => {
                                completed_now += 1;
                                committed_now += 1;
                                if !matches!(action, AgentShipAction::Commit) {
                                    pushed_now += 1;
                                }
                                repository.pending_commit = None;
                                repository.completed = true;
                                if let Some(url) = outcome.pr_url.clone() {
                                    pr_urls.push(url);
                                }
                                if let Some(center) = center.upgrade() {
                                    let tracked_repo = repository.tracked_repo_path.clone();
                                    let repository_label = repository.label.clone();
                                    center.update(cx, |center, cx| {
                                        center.attach_ship_commit_to_changed_files(
                                            agent_id,
                                            outcome.snapshot_id,
                                            outcome.commit_sha.clone(),
                                            cx,
                                        );
                                        center.append_agent_ship_result(
                                            agent_id,
                                            Some(repository_label),
                                            &outcome,
                                            cx,
                                        );
                                        if let Some(branch) = outcome.tracked_pr_branch.clone() {
                                            center.track_agent_ship_pr_branch(
                                                agent_id,
                                                tracked_repo,
                                                branch,
                                                cx,
                                            );
                                        }
                                    });
                                }
                                repository.git.update(cx, |git, cx| {
                                    git.last_message = Some(outcome.message);
                                    git.last_error = None;
                                    git.refresh(cx);
                                });
                            }
                            Err(error) => {
                                if let Some(pending) = error.pending_commit.as_ref() {
                                    committed_now += 1;
                                    if pending.pushed {
                                        pushed_now += 1;
                                    }
                                }
                                repository.pending_commit = error.pending_commit.clone();
                                let error = error.to_string();
                                failures.push(format!("{label}: {error}"));
                                repository.git.update(cx, |git, cx| {
                                    git.last_error = Some(error.clone());
                                    git.last_error_from_refresh = false;
                                    git.refresh(cx);
                                });
                            }
                        }
                    }
                    let all_complete = dialog
                        .repositories
                        .iter()
                        .all(|repository| {
                            repository.completed
                                || repository.included_files(dialog.scope).is_empty()
                        });
                    if all_complete {
                        dialog.error = None;
                        crate::ui::onboarding::emit_for_project(
                            project_id,
                            crate::ui::onboarding::OnboardingEvent::ShipCompleted,
                            cx,
                        );
                    } else if !failures.is_empty() {
                        let progress = match action {
                            AgentShipAction::Commit => {
                                format!("Committed {completed_now} of {total} repositories.")
                            }
                            AgentShipAction::CommitPush => format!(
                                "Committed {committed_now} and pushed {pushed_now} of {total} repositories."
                            ),
                            AgentShipAction::CommitPushPr => format!(
                                "Committed {committed_now}, pushed {pushed_now}, and opened {completed_now} of {total} pull requests."
                            ),
                        };
                        dialog.error = Some(format!(
                            "{progress} Fix and retry the remaining repositories: {}",
                            failures.join(" · ")
                        ));
                    }
                    cx.notify();
                    all_complete
                })
                .unwrap_or(false);
            if all_complete {
                window_handle
                    .update(cx, |_, window, cx| {
                        window.close_dialog(cx);
                        for url in pr_urls {
                            crate::ui::git::git_panel::open_url(&url);
                        }
                    })
                    .ok();
            }
        })
        .detach();
    }

    fn label(text: &'static str, cx: &App) -> impl IntoElement {
        div()
            .text_size(crate::ui::design::text_ui())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(crate::ui::design::t1(cx))
            .child(text)
    }

    fn segment(
        id: &'static str,
        label: &'static str,
        selected: bool,
        disabled: bool,
        handler: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let foreground = crate::ui::design::t1(cx);
        let muted = crate::ui::design::t3(cx);
        let mut segment = h_flex()
            .id(id)
            .flex_1()
            .min_w(px(0.))
            .h(crate::ui::design::control_h())
            .items_center()
            .justify_center()
            .rounded(crate::ui::design::r_sm())
            .px_2()
            .border_1()
            .border_color(if selected {
                crate::ui::design::line(cx).opacity(0.26)
            } else {
                gpui::transparent_black()
            })
            .text_size(crate::ui::design::text_ui())
            .font_weight(if selected {
                gpui::FontWeight::SEMIBOLD
            } else {
                gpui::FontWeight::MEDIUM
            })
            .text_color(if selected { foreground } else { muted })
            .when(selected, |segment| {
                segment.bg(crate::ui::design::surface_2(cx))
            })
            .child(div().truncate().child(label));
        if !disabled {
            segment = segment
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    handler(this, cx);
                    cx.notify();
                }));
        }
        segment.into_any_element()
    }

    fn check(
        id: &'static str,
        label: &'static str,
        checked: bool,
        disabled: bool,
        handler: impl Fn(&mut Self, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut row = h_flex()
            .id(id)
            .items_center()
            .gap_2()
            .text_size(crate::ui::design::text_ui())
            .text_color(if disabled {
                crate::ui::design::t3(cx)
            } else {
                crate::ui::design::t2(cx)
            })
            .child(crate::ui::style::checkbox((id, 0usize), checked, cx))
            .child(label);
        if !disabled {
            row = row
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    handler(this, cx);
                    cx.notify();
                }));
        }
        row.into_any_element()
    }

    fn render_file(
        &self,
        repository_index: usize,
        file_index: usize,
        path: &PathBuf,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let repository = &self.repositories[repository_index];
        let checked = !repository.deselected.contains(path);
        let kind = repository.file_kinds.get(path).copied();
        let color = kind
            .map(|kind| AgentShipDialog::ship_kind_color(kind, cx))
            .unwrap_or_else(|| crate::ui::design::t3(cx));
        let letter = kind.map(|kind| kind.letter()).unwrap_or("•");
        let path_to_toggle = path.clone();
        h_flex()
            .w_full()
            .items_center()
            .gap_2p5()
            .px_1()
            .py_0p5()
            .child(
                crate::ui::style::checkbox(
                    ("multi-ship-file", repository_index * 10_000 + file_index),
                    checked,
                    cx,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    let repository = &mut this.repositories[repository_index];
                    if !repository.deselected.remove(&path_to_toggle) {
                        repository.deselected.insert(path_to_toggle.clone());
                    }
                    this.reset_prepared();
                    cx.notify();
                })),
            )
            .child(crate::ui::style::status_letter(letter, color, cx))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_body())
                    .font_family(crate::ui::design::FONT_MONO)
                    .text_color(crate::ui::design::t1(cx))
                    .child(SharedString::from(path.display().to_string())),
            )
            .into_any_element()
    }
}

impl Render for MultiRepoShipDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.selected_repository >= self.repositories.len() {
            self.selected_repository = 0;
        }
        let selected_index = self.selected_repository;
        let repository = &self.repositories[selected_index];
        let repo_label = repository.label.clone();
        let files = repository.scope_files(self.scope).to_vec();
        let included_here = repository.included_files(self.scope).len();
        let branch = repository
            .branch
            .clone()
            .unwrap_or_else(|| "Detached HEAD".into());
        let branch_name = repository.branch_name.clone();
        let commit_message = repository.commit_message.clone();
        let pr_title = repository.pr_title.clone();
        let pr_description = repository.pr_description.clone();
        let pr_base_branch = repository.pr_base_branch.clone();
        let mut pr_base_options = repository.pr_base_branch_options.clone();
        pr_base_options.sort_by_key(|name| crate::ui::branch_order::branch_priority(name, false));
        let dialog_entity = cx.entity().clone();
        let repository_labels = self
            .repositories
            .iter()
            .map(|repository| repository.label.clone())
            .collect::<Vec<_>>();
        let pending_repositories = self.actionable_indices().len();
        let included_files = self.included_file_count();
        let settings_locked = self.busy
            || self
                .repositories
                .iter()
                .any(|repository| repository.completed);
        let can_continue = !self.busy && self.validation_error().is_none();
        let action = self.current_action();
        let repo_word = if pending_repositories == 1 {
            "repo"
        } else {
            "repos"
        };
        let primary_label = if self.busy {
            if !self.prepared && !self.auto_ship {
                "Generating content…".to_string()
            } else {
                let operation = if !self.prepared {
                    match action {
                        AgentShipAction::Commit => "Generating + committing",
                        AgentShipAction::CommitPush => "Generating + pushing",
                        AgentShipAction::CommitPushPr => "Generating + opening PRs",
                    }
                } else {
                    match action {
                        AgentShipAction::Commit => "Committing",
                        AgentShipAction::CommitPush => "Committing + pushing",
                        AgentShipAction::CommitPushPr => "Committing + pushing + opening PRs",
                    }
                };
                format!("{operation} {pending_repositories} {repo_word}…")
            }
        } else if self.prepared || self.auto_ship {
            format!(
                "{} {pending_repositories} {repo_word}",
                AgentShipDialog::action_label(action)
            )
        } else {
            "Generate content".to_string()
        };
        let max_dialog_height =
            (f32::from(window.viewport_size().height) - 150.0).clamp(320.0, 720.0);

        v_flex()
            .relative()
            .w_full()
            .max_h(px(max_dialog_height))
            .child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::ShipDialog,
                cx,
            ))
            .child(
                h_flex()
                    .w_full()
                    .flex_1()
                    .min_h(px(0.))
                    .items_start()
                    .child(
                        v_flex()
                            .w(px(250.))
                            .flex_none()
                            .pr_4()
                            .gap_4()
                            .border_r_1()
                            .border_color(crate::ui::design::line(cx))
                            .child(
                                v_flex().gap_2().child(Self::label("Branch", cx)).child(
                                    crate::ui::style::segmented_container_quiet(cx)
                                        .w_full()
                                        .child(Self::segment(
                                            "multi-ship-branch-current",
                                            "Current",
                                            !self.create_branch,
                                            settings_locked,
                                            |this, cx| {
                                                this.create_branch = false;
                                                this.reset_prepared();
                                                if let Some(repository) = this.repositories.iter().find(
                                                    |repository| {
                                                        this.open_pr
                                                            && repository.branch.as_deref().is_some_and(
                                                                |branch| {
                                                                    validate_agent_ship_pr_branches(
                                                                        branch,
                                                                        &repository.pr_base_branch,
                                                                    )
                                                                    .is_err()
                                                                },
                                                            )
                                                    },
                                                ) {
                                                    this.error = Some(format!(
                                                        "{} is already on the selected base branch. Choose New branch to open a pull request.",
                                                        repository.label
                                                    ));
                                                }
                                                cx.notify();
                                            },
                                            cx,
                                        ))
                                        .child(Self::segment(
                                            "multi-ship-branch-new",
                                            "New branch",
                                            self.create_branch,
                                            settings_locked,
                                            |this, cx| {
                                                this.create_branch = true;
                                                this.reset_prepared();
                                                cx.notify();
                                            },
                                            cx,
                                        )),
                                ),
                            )
                            .child(
                                v_flex()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(Self::label("Changes to commit", cx))
                                            .child(
                                                div()
                                                    .id("multi-ship-changes-info")
                                                    .child(
                                                        gpui_component::Icon::new(IconName::Info)
                                                            .size(crate::ui::design::icon_sm())
                                                            .text_color(crate::ui::design::t3(cx)),
                                                    )
                                                    .tooltip(|window, cx| {
                                                        Tooltip::new("“This chat” commits only files this agent edited. “All changes” commits every change in the workspace.")
                                                            .build(window, cx)
                                                    }),
                                            ),
                                    )
                                    .child(
                                        crate::ui::style::segmented_container_quiet(cx)
                                            .w_full()
                                            .child(Self::segment(
                                                "multi-ship-scope-chat",
                                                "This chat",
                                                self.scope == AgentShipScope::Conversation,
                                                settings_locked,
                                                |this, cx| {
                                                    this.scope = AgentShipScope::Conversation;
                                                    this.reset_prepared();
                                                    cx.notify();
                                                },
                                                cx,
                                            ))
                                            .child(Self::segment(
                                                "multi-ship-scope-all",
                                                "All changes",
                                                self.scope == AgentShipScope::All,
                                                settings_locked,
                                                |this, cx| {
                                                    this.scope = AgentShipScope::All;
                                                    this.reset_prepared();
                                                    cx.notify();
                                                },
                                                cx,
                                            )),
                                    ),
                            )
                            .child(Self::check(
                                "multi-ship-push",
                                "Push to remote",
                                self.push || self.open_pr,
                                settings_locked || self.open_pr,
                                |this, _| {
                                    this.push = !this.push;
                                    this.reset_prepared();
                                },
                                cx,
                            ))
                            .child(Self::check(
                                "multi-ship-pr",
                                "Open pull request",
                                self.open_pr,
                                settings_locked,
                                |this, _| {
                                    this.open_pr = !this.open_pr;
                                    if this.open_pr
                                        && this.repositories.iter().any(|repository| {
                                            repository.branch.as_deref().is_some_and(|branch| {
                                                validate_agent_ship_pr_branches(
                                                    branch,
                                                    &repository.pr_base_branch,
                                                )
                                                .is_err()
                                            })
                                        })
                                    {
                                        this.create_branch = true;
                                    }
                                    this.reset_prepared();
                                },
                                cx,
                            ))
                            .child(
                                v_flex()
                                    .gap_1p5()
                                    .pt_1()
                                    .child(Self::label("Repositories", cx))
                                    .children(repository_labels.into_iter().enumerate().map(
                                        |(index, label)| {
                                            crate::ui::style::header_meta_button(
                                                ("multi-ship-repository", index),
                                                cx,
                                            )
                                            .w_full()
                                            .justify_start()
                                            .selected(index == selected_index)
                                            .label(label)
                                            .on_click(
                                                cx.listener(move |this, _, _, cx| {
                                                    this.selected_repository = index;
                                                    cx.notify();
                                                }),
                                            )
                                        },
                                    )),
                            ),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .min_h(px(0.))
                            .pl_5()
                            .gap_0()
                            .overflow_y_scrollbar()
                            .child(
                                h_flex()
                                    .w_full()
                                    .items_center()
                                    .justify_between()
                                    .mb_4()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(repo_label),
                                    )
                                    .when(!self.create_branch, |row| {
                                        row.child(
                                            h_flex()
                                                .items_center()
                                                .gap_1p5()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child(branch_icon(crate::ui::design::t3(cx)))
                                                .child(branch.clone()),
                                        )
                                    }),
                            )
                            .when(self.create_branch, |column| {
                                column.child(
                                    v_flex()
                                        .w_full()
                                        .mb_5()
                                        .gap_2()
                                        .child(Self::label("Branch name", cx))
                                        .child(
                                            h_flex()
                                                .w_full()
                                                .h(crate::ui::design::subhead_h())
                                                .items_center()
                                                .rounded(px(crate::ui::style::RADIUS))
                                                .border_1()
                                                .border_color(crate::ui::design::line(cx))
                                                .bg(crate::ui::style::surface(cx))
                                                .px_3()
                                                .child(
                                                    Input::new(&branch_name)
                                                        .appearance(false)
                                                        .bordered(false)
                                                        .focus_bordered(false)
                                                        .w_full()
                                                        .min_w(px(0.)),
                                                ),
                                        ),
                                )
                            })
                            .child(
                                v_flex()
                                    .w_full()
                                    .mb_5()
                                    .gap_2()
                                    .child(
                                        h_flex()
                                            .id("multi-ship-files-toggle")
                                            .items_center()
                                            .gap_1()
                                            .cursor_pointer()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.files_collapsed = !this.files_collapsed;
                                                cx.notify();
                                            }))
                                            .child(
                                                Icon::new(if self.files_collapsed {
                                                    IconName::ChevronRight
                                                } else {
                                                    IconName::ChevronDown
                                                })
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::t3(cx)),
                                            )
                                            .child(Self::label("Files", cx))
                                            .child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(crate::ui::design::t3(cx))
                                                    .child(format!(
                                                        "{included_here}/{}",
                                                        files.len()
                                                    )),
                                            ),
                                    )
                                    .when(!self.files_collapsed, |section| {
                                        section.child(if files.is_empty() {
                                            div()
                                                .w_full()
                                                .rounded(px(crate::ui::style::RADIUS))
                                                .border_1()
                                                .border_color(crate::ui::design::line(cx))
                                                .bg(crate::ui::style::surface(cx))
                                                .px_3()
                                                .py_2()
                                                .text_size(crate::ui::design::text_body())
                                                .text_color(crate::ui::design::t3(cx))
                                                .child("No changes in scope")
                                                .into_any_element()
                                        } else {
                                            v_flex()
                                                .w_full()
                                                .rounded(px(crate::ui::style::RADIUS))
                                                .border_1()
                                                .border_color(crate::ui::design::line(cx))
                                                .bg(crate::ui::style::surface(cx))
                                                .p_1p5()
                                                .gap_0p5()
                                                .children(files.iter().enumerate().map(
                                                    |(file_index, path)| {
                                                        self.render_file(
                                                            selected_index,
                                                            file_index,
                                                            path,
                                                            cx,
                                                        )
                                                    },
                                                ))
                                                .into_any_element()
                                        })
                                    }),
                            )
                            .child(
                                v_flex()
                                    .w_full()
                                    .mb_5()
                                    .gap_2()
                                    .child(Self::label("Commit message", cx))
                                    .child(
                                        div()
                                            .w_full()
                                            .min_h(px(66.))
                                            .rounded(px(crate::ui::style::RADIUS))
                                            .border_1()
                                            .border_color(crate::ui::design::line(cx))
                                            .bg(crate::ui::style::surface(cx))
                                            .px_3()
                                            .py_2()
                                            .child(
                                                Input::new(&commit_message)
                                                    .appearance(false)
                                                    .bordered(false)
                                                    .focus_bordered(false)
                                                    .w_full()
                                                    .min_w(px(0.)),
                                            ),
                                    ),
                            )
                            .when(self.open_pr, |column| {
                                let options = pr_base_options.clone();
                                let base_dialog = dialog_entity.clone();
                                column
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .mb_5()
                                            .gap_2()
                                            .child(Self::label("Base branch", cx))
                                            .child(
                                                crate::ui::style::dialog_neutral_button(
                                                    ("multi-ship-base", selected_index),
                                                    pr_base_branch.clone(),
                                                    cx,
                                                )
                                                .w_full()
                                                .h(crate::ui::design::subhead_h())
                                                .justify_start()
                                                .dropdown_caret(true)
                                                .dropdown_menu(move |menu, _, _| {
                                                    options.iter().fold(menu, |menu, option| {
                                                        let option = option.clone();
                                                        let dialog = base_dialog.clone();
                                                        menu.item(
                                                            PopupMenuItem::new(option.clone())
                                                                .checked(option == pr_base_branch)
                                                                .on_click(move |_, _, cx| {
                                                                    dialog.update(
                                                                        cx,
                                                                        |this, cx| {
                                                                            this.repositories
                                                                                [selected_index]
                                                                                .pr_base_branch =
                                                                                option.clone();
                                                                            if this.open_pr
                                                                                && this.repositories
                                                                                    [selected_index]
                                                                                    .branch
                                                                                    .as_deref()
                                                                                    .is_some_and(
                                                                                        |branch| {
                                                                                            validate_agent_ship_pr_branches(
                                                                                                branch,
                                                                                                &option,
                                                                                            )
                                                                                            .is_err()
                                                                                        },
                                                                                    )
                                                                            {
                                                                                this.create_branch =
                                                                                    true;
                                                                            }
                                                                            this.reset_prepared();
                                                                            cx.notify();
                                                                        },
                                                                    );
                                                                }),
                                                        )
                                                    })
                                                }),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .mb_5()
                                            .gap_2()
                                            .child(Self::label("Pull request title", cx))
                                            .child(
                                                div()
                                                    .w_full()
                                                    .min_h(px(34.))
                                                    .rounded(px(crate::ui::style::RADIUS))
                                                    .border_1()
                                                    .border_color(crate::ui::design::line(cx))
                                                    .bg(crate::ui::style::surface(cx))
                                                    .px_3()
                                                    .py_1p5()
                                                    .child(
                                                        Input::new(&pr_title)
                                                            .appearance(false)
                                                            .bordered(false)
                                                            .focus_bordered(false)
                                                            .w_full()
                                                            .min_w(px(0.)),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        v_flex()
                                            .w_full()
                                            .mb_5()
                                            .gap_2()
                                            .child(Self::label("Pull request description", cx))
                                            .child(
                                                div()
                                                    .w_full()
                                                    .min_h(px(66.))
                                                    .rounded(px(crate::ui::style::RADIUS))
                                                    .border_1()
                                                    .border_color(crate::ui::design::line(cx))
                                                    .bg(crate::ui::style::surface(cx))
                                                    .px_3()
                                                    .py_2()
                                                    .child(
                                                        Input::new(&pr_description)
                                                            .appearance(false)
                                                            .bordered(false)
                                                            .focus_bordered(false)
                                                            .w_full()
                                                            .min_w(px(0.)),
                                                    ),
                                            ),
                                    )
                            }),
                    ),
            )
            .when_some(self.error.clone(), |dialog, error| {
                dialog.child(
                    div()
                        .w_full()
                        .mt_3()
                        .rounded(crate::ui::design::r_sm())
                        .border_1()
                        .border_color(crate::ui::design::rose(cx))
                        .bg(crate::ui::design::rose(cx).opacity(0.1))
                        .px_3()
                        .py_2()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(error),
                )
            })
            .child(
                h_flex()
                    .w_full()
                    .justify_between()
                    .items_center()
                    .gap_3()
                    .border_t_1()
                    .border_color(crate::ui::design::line(cx))
                    .pt_3()
                    .mt_3()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!(
                                "{included_files} files across {pending_repositories} repos"
                            )),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                crate::ui::style::ghost_button("cancel-multi-ship", "Cancel")
                                    .custom(crate::ui::style::dialog_neutral_variant(cx))
                                    .disabled(self.busy)
                                    .on_click(|_, window, cx| window.close_dialog(cx)),
                            )
                            .child(Self::check(
                                "multi-ship-auto",
                                "Auto",
                                self.auto_ship,
                                self.busy,
                                |this, _| {
                                    this.auto_ship = !this.auto_ship;
                                },
                                cx,
                            ))
                            .when(self.busy, |footer| {
                                footer.child(gpui_component::spinner::Spinner::new().xsmall())
                            })
                            .child(
                                crate::ui::style::ship_button_primary(
                                    "run-multi-ship",
                                    primary_label,
                                    cx,
                                )
                                .disabled(!can_continue)
                                .on_click(cx.listener(
                                    |this, _, window, cx| {
                                        if this.prepared {
                                            let action = this.current_action();
                                            this.run(action, window, cx);
                                        } else {
                                            this.prepare(window, cx);
                                        }
                                    },
                                )),
                            ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::actionable_repository_indices;

    #[test]
    fn retry_skips_completed_and_empty_repositories() {
        assert_eq!(
            actionable_repository_indices([(true, 2), (false, 3), (false, 0), (true, 1)]),
            vec![1],
        );
    }
}
