use super::*;

impl GitPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        git_states: Entity<GitStates>,
        agents: Entity<AgentRecords>,
        center: gpui::WeakEntity<crate::ui::center::CenterArea>,
        window: &mut Window,
        cx: &mut App,
    ) -> Entity<Self> {
        let commit_input = cx.new(|cx| {
            InputState::new(window, cx)
                // The commit composer has a tokenized fixed frame. A fixed
                // multiline viewport keeps its internal scroll bounds equal to
                // the visible editor instead of auto-growing behind the action
                // strip and clipping every line after the first two.
                .multi_line(true)
                .placeholder("Enter commit message")
        });
        let branch_query =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search or create branch…"));
        let view = cx.new(|cx| {
            let last_active_project = workspace.read(cx).active;
            let last_active_repository = last_active_project
                .and_then(|project_id| git_states.read(cx).active_repository_path(project_id));
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&git_states, |_, _, cx| cx.notify()).detach();
            // Solo lanes appear/disappear with agent changes.
            cx.observe(&agents, |this: &mut Self, _, cx| {
                this.solo_ahead_checked_at = None;
                cx.notify();
            })
            .detach();
            // Re-filter the branch list as the query changes.
            cx.subscribe(&branch_query, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(PULL_REQUEST_REFRESH_INTERVAL)
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            })
            .detach();
            cx.spawn(async move |this, cx| loop {
                cx.background_executor()
                    .timer(WORKFLOW_REFRESH_INTERVAL)
                    .await;
                if this
                    .update(cx, |panel, cx| {
                        if matches!(panel.tab, GitTab::Workflows | GitTab::PullRequests) {
                            if let Some(git) = panel.active_git(cx) {
                                panel.sync_workflow_runs(git, cx);
                            }
                        }
                    })
                    .is_err()
                {
                    break;
                }
            })
            .detach();
            Self {
                workspace,
                git_states,
                agents,
                center,
                commit_input,
                branch_query,
                branches_expanded: false,
                collapsed_status_folders: HashSet::new(),
                hovered_status_file: None,
                branch_row_hovered: false,
                pr_chip_hovered: false,
                tab: GitTab::default(),
                commit_ai_generating: false,
                commit_ai_error: None,
                git_accounts: Vec::new(),
                git_accounts_loading: false,
                git_accounts_error: None,
                last_active_project,
                last_active_repository,
                last_push_notice_message: None,
                seen_push_notice_messages: HashSet::new(),
                branch_pr_key: None,
                branch_pr_last_message: None,
                branch_pr_checked_at: None,
                branch_pr_fetching: false,
                branch_pr: None,
                repo_pr_key: None,
                repo_prs_last_message: None,
                repo_prs: Vec::new(),
                repo_prs_fetching: false,
                repo_prs_checked_at: None,
                repo_prs_error: None,
                workflow_runs_refreshing: false,
                workflow_runs_checked_at: None,
                workflow_runs_error: None,
                solo_ahead: HashMap::new(),
                solo_ahead_checked_at: None,
                solo_ahead_fetching: false,
                scope_agent: None,
                scope_main: false,
                lane_git: None,
            }
        });
        view.update(cx, |view, cx| view.refresh_git_accounts(cx));
        view
    }

    pub(super) fn active_git(&self, cx: &App) -> Option<Entity<GitState>> {
        // Scoped to a Solo's lane: the whole panel reads the lane's GitState —
        // one substitution point, everything else behaves identically.
        if self.scoped_to_lane() {
            if let Some((_, git)) = &self.lane_git {
                return Some(git.clone());
            }
        }
        let id = self.workspace.read(cx).active.as_ref().copied()?;
        self.git_states.read(cx).get(id)
    }

    /// Render-path accessor: reads the snapshot's cached remote — never asks
    /// git directly (a subprocess per frame is what tanked scroll perf once).
    pub(crate) fn git_account_remote(&self, cx: &App) -> Option<GitRemote> {
        let git = self.active_git(cx)?;
        git.read(cx).snapshot.as_ref()?.primary_remote.clone()
    }

    pub(crate) fn connected_git_accounts(&self) -> Vec<GitHubAccount> {
        self.git_accounts.clone()
    }

    pub(crate) fn selected_git_account(&self, cx: &App) -> Option<String> {
        let git = self.active_git(cx)?;
        git.read(cx).snapshot.as_ref()?.assigned_account.clone()
    }

    pub(crate) fn git_accounts_loading(&self) -> bool {
        self.git_accounts_loading
    }

    pub(crate) fn git_accounts_error(&self) -> Option<String> {
        self.git_accounts_error.clone()
    }

    pub(crate) fn git_repository_error(&self, cx: &App) -> Option<String> {
        let git = self.active_git(cx)?;
        git.read(cx).last_error.clone()
    }

    pub(crate) fn select_git_account(&mut self, account: Option<String>, cx: &mut Context<Self>) {
        let Some(git) = self.active_git(cx) else {
            return;
        };
        let repo = git.read(cx).repo_path.clone();
        cx.spawn(async move |this, cx| {
            // Remote listing and the bindings read-modify-write are file I/O —
            // keep them off the UI thread.
            let result = cx
                .background_executor()
                .spawn(async move {
                    let remotes = ide_core::git::repository_remotes(&repo);
                    remotes
                        .iter()
                        .filter(|remote| remote.is_github_https())
                        .try_for_each(|remote| {
                            ide_core::git::assign_github_account(&repo, remote, account.as_deref())
                        })
                })
                .await;
            this.update(cx, |panel, cx| {
                match result {
                    Ok(()) => {
                        panel.git_accounts_error = None;
                        // The bindings file has no watcher; refresh so the
                        // snapshot's cached account reflects the new choice.
                        git.update(cx, |state, cx| state.refresh(cx));
                    }
                    Err(error) => panel.git_accounts_error = Some(format!("{error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn refresh_git_accounts(&mut self, cx: &mut Context<Self>) {
        if self.git_accounts_loading {
            return;
        }
        self.git_accounts_loading = true;
        self.git_accounts_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { ide_core::git::connected_github_accounts() })
                .await;
            this.update(cx, |panel, cx| {
                panel.git_accounts_loading = false;
                match result {
                    Ok(accounts) => {
                        panel.git_accounts = accounts;
                        panel.git_accounts_error = None;
                    }
                    Err(error) => {
                        panel.git_accounts.clear();
                        panel.git_accounts_error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn connect_git_account(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            // Locating gh can shell out (login shell) and osascript blocks
            // until Terminal is up — never on the UI thread.
            let result = cx
                .background_executor()
                .spawn(async { ide_core::git::open_github_account_login() })
                .await;
            this.update(cx, |panel, cx| {
                panel.git_accounts_error = Some(match result {
                    Ok(()) => "Finish connecting in Terminal, then choose Refresh accounts here."
                        .to_string(),
                    Err(error) => format!("{error:#}"),
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn repository_options(&self, cx: &App) -> Vec<(PathBuf, String, bool)> {
        if self.scoped_to_lane() {
            return Vec::new();
        }
        let Some(project_id) = self.workspace.read(cx).active else {
            return Vec::new();
        };
        let Some(project) = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return Vec::new();
        };
        let active = self.git_states.read(cx).active_repository_path(project_id);
        self.git_states
            .read(cx)
            .repositories(project_id)
            .into_iter()
            .map(|git| {
                let path = git.read(cx).repo_path.clone();
                let label = if path == project.path {
                    project.name.clone()
                } else {
                    path.strip_prefix(&project.path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned()
                };
                let selected = active.as_ref() == Some(&path);
                (path, label, selected)
            })
            .collect()
    }

    pub(crate) fn select_repository(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(project_id) = self.workspace.read(cx).active else {
            return;
        };
        self.git_states.update(cx, |states, cx| {
            states.set_active_repository(project_id, path, cx)
        });
        self.workflow_runs_checked_at = None;
        if matches!(self.tab, GitTab::Workflows | GitTab::PullRequests) {
            if let Some(git) = self.active_git(cx) {
                self.sync_workflow_runs(git, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn generate_commit_message(
        &mut self,
        git: Entity<GitState>,
        use_staged: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.commit_ai_generating {
            return;
        }
        let project_id = self.workspace.read(cx).active;
        let repo = git.read(cx).repo_path.clone();
        let generation_agent = self.workspace.read(cx).generation_agent.clone();
        let input = self.commit_input.clone();
        let window_handle = window.window_handle();
        self.commit_ai_generating = true;
        self.commit_ai_error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { generate_commit_message(&generation_agent, &repo, use_staged) })
                .await;

            this.update(cx, |panel, cx| {
                if panel.workspace.read(cx).active != project_id {
                    return;
                }
                panel.commit_ai_generating = false;
                match result {
                    Ok(message) => {
                        panel.commit_ai_error = None;
                        let message = message.trim().to_string();
                        notifications::play_generated_sound();
                        window_handle
                            .update(cx, |_, window, cx| {
                                input.update(cx, |input, cx| {
                                    input.set_value(message.clone(), window, cx);
                                });
                            })
                            .ok();
                    }
                    Err(error) => {
                        panel.commit_ai_error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn sync_push_notice(
        &mut self,
        git: Entity<GitState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (repo_path, message, base_branch_options) = {
            let state = git.read(cx);
            let Some(message) = state
                .last_message
                .as_deref()
                .filter(|message| {
                    message.starts_with("Pushed ") || message.starts_with("Published ")
                })
                .map(str::to_string)
            else {
                return;
            };
            let default_branch = default_remote_branch(&state.repo_path);
            let base_branch_options = state
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    pull_request_base_branch_options(&default_branch, &snapshot.branches)
                })
                .unwrap_or_else(|| vec![default_branch]);
            (state.repo_path.clone(), message, base_branch_options)
        };
        if self.last_push_notice_message.as_deref() == Some(message.as_str()) {
            return;
        }
        let notice_key = format!("{}:{}", repo_path.display(), message);
        if self.seen_push_notice_messages.contains(&notice_key) {
            self.last_push_notice_message = Some(message);
            return;
        }
        let Some(push) = branch_from_push_message(&message) else {
            return;
        };
        self.last_push_notice_message = Some(message.clone());
        self.seen_push_notice_messages.insert(notice_key);

        if push.kind != PushNoticeKind::Published {
            return;
        }

        let default_branch = base_branch_options
            .first()
            .cloned()
            .unwrap_or_else(|| default_remote_branch(&repo_path));
        if push.branch == default_branch || matches!(push.branch.as_str(), "main" | "master") {
            return;
        }

        let Some(url) = github_pull_request_url(&repo_path, &push.branch, &default_branch) else {
            return;
        };

        let project_id = self.workspace.read(cx).active;
        let branch = push.branch.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let existing_pr = cx
                .background_executor()
                .spawn({
                    let repo_path = repo_path.clone();
                    let branch = branch.clone();
                    async move { existing_pull_request_url(&repo_path, &branch) }
                })
                .await;

            this.update(cx, |panel, cx| {
                if panel.workspace.read(cx).active != project_id || existing_pr.is_some() {
                    return;
                }

                window_handle
                    .update(cx, |_, window, cx| {
                        let notice = PullRequestNotice {
                            message: message.into(),
                            branch,
                            url: Some(url),
                        };
                        let generation_agent = panel.workspace.read(cx).generation_agent.clone();
                        let base_branch_query =
                            cx.new(|cx| InputState::new(window, cx).placeholder("Search branch…"));
                        let dialog = cx.new(|cx| {
                            // Live-filter the branch list as the user types.
                            cx.subscribe(&base_branch_query, |_, _, event: &InputEvent, cx| {
                                if matches!(event, InputEvent::Change) {
                                    cx.notify();
                                }
                            })
                            .detach();
                            PullRequestDialog {
                                notice,
                                git,
                                generation_agent,
                                base_branch: default_branch.clone(),
                                base_branch_options: base_branch_options.clone(),
                                base_branch_expanded: false,
                                base_branch_query: base_branch_query.clone(),
                                ai_enabled: true,
                                generating: false,
                                error: None,
                            }
                        });
                        window.open_dialog(cx, move |dialog_view, _, _| {
                            dialog_view
                                .title("Create pull request")
                                .w(px(640.))
                                .overlay_closable(false)
                                .child(dialog.clone())
                        });
                    })
                    .ok();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn sync_branch_pull_request(
        &mut self,
        git: Entity<GitState>,
        cx: &mut Context<Self>,
    ) {
        let (repo_path, branch, last_message) = {
            let state = git.read(cx);
            let branch = state.snapshot.as_ref().and_then(|s| s.head.branch.clone());
            (state.repo_path.clone(), branch, state.last_message.clone())
        };
        let Some(branch) = branch else {
            self.branch_pr_key = None;
            self.branch_pr = None;
            self.branch_pr_fetching = false;
            self.branch_pr_last_message = None;
            self.branch_pr_checked_at = None;
            return;
        };

        if matches!(branch.as_str(), "main" | "master") {
            self.branch_pr_key = None;
            self.branch_pr = None;
            self.branch_pr_fetching = false;
            self.branch_pr_last_message = None;
            self.branch_pr_checked_at = None;
            return;
        }

        let key = PullRequestLookupKey { repo_path, branch };
        let refresh_interval = if self.branch_pr.is_some() {
            PULL_REQUEST_REFRESH_INTERVAL
        } else {
            PULL_REQUEST_MISSING_REFRESH_INTERVAL
        };
        let stale = self
            .branch_pr_checked_at
            .is_none_or(|checked_at| checked_at.elapsed() > refresh_interval);
        let should_refresh = self.branch_pr_key.as_ref() != Some(&key)
            || self.branch_pr_last_message != last_message
            || stale;
        if !should_refresh || self.branch_pr_fetching {
            return;
        }

        self.branch_pr_key = Some(key.clone());
        self.branch_pr_last_message = last_message;
        self.branch_pr_checked_at = Some(Instant::now());
        self.branch_pr_fetching = true;
        self.branch_pr = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let pr = cx
                .background_executor()
                .spawn({
                    let repo_path = key.repo_path.clone();
                    let branch = key.branch.clone();
                    async move { branch_pull_request(&repo_path, &branch) }
                })
                .await;

            this.update(cx, |panel, cx| {
                if panel.branch_pr_key.as_ref() != Some(&key) {
                    return;
                }
                panel.branch_pr_fetching = false;
                panel.branch_pr = pr;
                if panel.branch_pr.is_none() {
                    let retry_key = key.clone();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(PULL_REQUEST_MISSING_REFRESH_INTERVAL)
                            .await;
                        this.update(cx, |panel, cx| {
                            if panel.branch_pr_key.as_ref() == Some(&retry_key)
                                && panel.branch_pr.is_none()
                                && !panel.branch_pr_fetching
                            {
                                panel.branch_pr_checked_at = None;
                                cx.notify();
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(super) fn sync_repo_pull_requests(
        &mut self,
        git: Entity<GitState>,
        cx: &mut Context<Self>,
    ) {
        if self.tab != GitTab::PullRequests {
            return;
        }

        let (repo_path, last_message) = {
            let state = git.read(cx);
            (state.repo_path.clone(), state.last_message.clone())
        };
        let refresh_interval = if self.repo_prs.is_empty() {
            PULL_REQUEST_MISSING_REFRESH_INTERVAL
        } else {
            PULL_REQUEST_REFRESH_INTERVAL
        };
        let stale = self
            .repo_prs_checked_at
            .is_none_or(|checked_at| checked_at.elapsed() > refresh_interval);
        let should_refresh = self.repo_pr_key.as_ref() != Some(&repo_path)
            || self.repo_prs_last_message != last_message
            || stale;
        if !should_refresh || self.repo_prs_fetching {
            return;
        }

        self.repo_pr_key = Some(repo_path.clone());
        self.repo_prs_last_message = last_message;
        self.repo_prs_fetching = true;
        self.repo_prs_checked_at = Some(Instant::now());
        self.repo_prs_error = None;
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let repo_path = repo_path.clone();
                    async move { repo_pull_requests(&repo_path) }
                })
                .await;

            this.update(cx, |panel, cx| {
                if panel.repo_pr_key.as_ref() != Some(&repo_path) {
                    return;
                }
                panel.repo_prs_fetching = false;
                match result {
                    Ok(prs) => {
                        panel.repo_prs = prs;
                        panel.repo_prs_error = None;
                        if panel.repo_prs.is_empty() {
                            let retry_repo = repo_path.clone();
                            cx.spawn(async move |this, cx| {
                                cx.background_executor()
                                    .timer(PULL_REQUEST_MISSING_REFRESH_INTERVAL)
                                    .await;
                                this.update(cx, |panel, cx| {
                                    if panel.repo_pr_key.as_ref() == Some(&retry_repo)
                                        && panel.repo_prs.is_empty()
                                        && !panel.repo_prs_fetching
                                    {
                                        panel.repo_prs_checked_at = None;
                                        cx.notify();
                                    }
                                })
                                .ok();
                            })
                            .detach();
                        }
                    }
                    Err(error) => {
                        panel.repo_prs.clear();
                        panel.repo_prs_error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
