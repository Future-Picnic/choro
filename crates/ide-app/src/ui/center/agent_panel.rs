use super::*;

mod center_ship;
mod detail_drawers;
mod detail_footer;
mod detail_section;
mod multi_repo_ship;
mod operations;
mod remote_ship;
mod render;
mod task_actions;

use operations::*;
pub(super) use remote_ship::RemoteShipStatus;
pub(super) use task_actions::{ship_task_comment_body, ShipTaskSeed, ShipTaskUi};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentShipScope {
    Conversation,
    All,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AgentShipAction {
    Commit,
    CommitPush,
    CommitPushPr,
}

fn start_ship_summary_maintenance(
    center: &gpui::WeakEntity<CenterArea>,
    agent_id: Uuid,
    cx: &mut App,
) -> bool {
    let Some(center) = center.upgrade() else {
        return false;
    };
    center.update(cx, |center, cx| {
        center.agent_summary_requests_pending.remove(&agent_id);
        center.request_agent_summary_maintenance(agent_id, cx)
    })
}

struct MultiRepoShipRepository {
    label: String,
    git: Entity<GitState>,
    repo_path: PathBuf,
    tracked_repo_path: PathBuf,
    branch: Option<String>,
    needs_upstream: bool,
    all_files: Vec<PathBuf>,
    conversation_files: Vec<PathBuf>,
    staged_files: Vec<PathBuf>,
    file_kinds: HashMap<PathBuf, ide_core::git::ChangeKind>,
    deselected: HashSet<PathBuf>,
    branch_name: Entity<InputState>,
    commit_message: Entity<InputState>,
    pr_base_branch: String,
    pr_base_branch_options: Vec<String>,
    pr_title: Entity<InputState>,
    pr_description: Entity<InputState>,
    pending_commit: Option<AgentShipPendingCommit>,
    completed: bool,
}

struct MultiRepoShipDialog {
    agent_id: Uuid,
    project_id: ProjectId,
    center: gpui::WeakEntity<CenterArea>,
    agent_title: String,
    generation_agent: ide_core::config::GenerationAgent,
    repositories: Vec<MultiRepoShipRepository>,
    selected_repository: usize,
    files_collapsed: bool,
    create_branch: bool,
    scope: AgentShipScope,
    push: bool,
    open_pr: bool,
    auto_ship: bool,
    busy: bool,
    prepared: bool,
    summary_maintenance_started: bool,
    error: Option<String>,
}

struct AgentShipDialog {
    agent_id: Uuid,
    project_id: ProjectId,
    center: gpui::WeakEntity<CenterArea>,
    git: Entity<GitState>,
    agent_title: String,
    repo_path: PathBuf,
    generation_agent: ide_core::config::GenerationAgent,
    branch: Option<String>,
    needs_upstream: bool,
    create_branch: bool,
    all_files: Vec<PathBuf>,
    conversation_files: Vec<PathBuf>,
    staged_files: Vec<PathBuf>,
    branch_name: Entity<InputState>,
    commit_message: Entity<InputState>,
    pr_base_branch: String,
    pr_base_branch_options: Vec<String>,
    /// Search field + open/closed state for the git-panel-style base-branch picker.
    pr_base_branch_query: Entity<InputState>,
    pr_base_branch_expanded: bool,
    pr_title: Entity<InputState>,
    pr_description: Entity<InputState>,
    scope: AgentShipScope,
    push: bool,
    open_pr: bool,
    onboarding_demo: bool,
    auto_ship: bool,
    file_kinds: std::collections::HashMap<PathBuf, ide_core::git::ChangeKind>,
    deselected: std::collections::HashSet<PathBuf>,
    files_collapsed: bool,
    busy: bool,
    prepared: bool,
    error: Option<String>,
    status: Option<String>,
    pending_commit: Option<AgentShipPendingCommit>,
    summary_maintenance_started: bool,
    /// `(project_root, lane_path)` when shipping a Solo from its lane. PR
    /// tracking then keys on the project root (the lane folder is disposable),
    /// and a successful PR packs the lane up.
    solo_lane: Option<(PathBuf, PathBuf)>,
}

impl AgentShipDialog {
    fn start_summary_maintenance(&mut self, cx: &mut App) {
        if self.summary_maintenance_started {
            return;
        }
        self.summary_maintenance_started =
            start_ship_summary_maintenance(&self.center, self.agent_id, cx);
    }

    fn scope_files(&self) -> &[PathBuf] {
        match self.scope {
            AgentShipScope::Conversation => &self.conversation_files,
            AgentShipScope::All => &self.all_files,
        }
    }

    fn included_files(&self) -> Vec<PathBuf> {
        self.scope_files()
            .iter()
            .filter(|path| !self.deselected.contains(*path))
            .cloned()
            .collect()
    }

    fn ship_kind_color(kind: ide_core::git::ChangeKind, cx: &gpui::App) -> gpui::Hsla {
        use ide_core::git::ChangeKind;
        match kind {
            ChangeKind::Added | ChangeKind::Untracked => crate::ui::design::sage(cx),
            ChangeKind::Deleted | ChangeKind::Conflicted => crate::ui::design::rose(cx),
            _ => crate::ui::design::amber(cx),
        }
    }

    fn render_ship_file(
        &self,
        index: usize,
        path: &PathBuf,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let checked = !self.deselected.contains(path);
        let kind = self.file_kinds.get(path).copied();
        let color = kind
            .map(|k| Self::ship_kind_color(k, cx))
            .unwrap_or_else(|| crate::ui::design::t3(cx));
        let letter = kind.map(|k| k.letter()).unwrap_or("•");
        let toggle_path = path.clone();
        h_flex()
            .w_full()
            .items_center()
            .gap_2p5()
            .px_1()
            .py_0p5()
            .child(
                crate::ui::style::checkbox(("ship-file", index), checked, cx).on_click(
                    cx.listener(move |this, _, _, cx| {
                        if !this.deselected.remove(&toggle_path) {
                            this.deselected.insert(toggle_path.clone());
                        }
                        this.reset_prepared();
                        this.error = None;
                        cx.notify();
                    }),
                ),
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

    fn staged_outside_scope(&self) -> Vec<PathBuf> {
        let included = self
            .included_files()
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        self.staged_files
            .iter()
            .filter(|path| !included.contains(*path))
            .cloned()
            .collect()
    }

    fn current_action(&self) -> AgentShipAction {
        let push = self.push || self.open_pr;
        if !push {
            AgentShipAction::Commit
        } else if self.open_pr {
            AgentShipAction::CommitPushPr
        } else {
            AgentShipAction::CommitPush
        }
    }

    fn action_label(action: AgentShipAction) -> &'static str {
        match action {
            AgentShipAction::Commit => "Commit",
            AgentShipAction::CommitPush => "Commit + push",
            AgentShipAction::CommitPushPr => "Commit + push + PR",
        }
    }

    fn busy_label(action: AgentShipAction, auto_ship: bool, prepared: bool) -> &'static str {
        if auto_ship {
            return match action {
                AgentShipAction::Commit => "Generating + committing…",
                AgentShipAction::CommitPush => "Generating + pushing…",
                AgentShipAction::CommitPushPr => "Generating + PR…",
            };
        }
        if !prepared {
            return "Generating…";
        }
        match action {
            AgentShipAction::Commit => "Committing…",
            AgentShipAction::CommitPush => "Pushing…",
            AgentShipAction::CommitPushPr => "Pushing + PR…",
        }
    }

    fn reset_prepared(&mut self) {
        self.prepared = false;
        self.status = None;
    }

    fn prepare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.branch.is_none() && !self.create_branch {
            self.error = Some("Current Git HEAD is not on a branch. Create a branch first.".into());
            cx.notify();
            return;
        }
        let files = self.included_files();
        if files.is_empty() {
            self.error = Some("No files selected for this scope.".into());
            cx.notify();
            return;
        }
        let staged_outside = self.staged_outside_scope();
        if !staged_outside.is_empty() {
            self.error = Some(format!(
                "Unstage files outside this scope before shipping: {}",
                staged_outside
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            cx.notify();
            return;
        }

        let repo = self.repo_path.clone();
        let current_branch = self.branch.clone();
        let create_branch = self.create_branch;
        let branch_name = self.branch_name.read(cx).value().trim().to_string();
        let commit_message = self.commit_message.read(cx).value().trim().to_string();
        let pr_base_branch = self.pr_base_branch.trim().to_string();
        let pr_title = self.pr_title.read(cx).value().trim().to_string();
        let pr_description = self.pr_description.read(cx).value().trim().to_string();
        let agent_title = self.agent_title.clone();
        let generation_agent = self.generation_agent.clone();
        let action = self.current_action();
        let branch_input = self.branch_name.clone();
        let commit_input = self.commit_message.clone();
        let pr_title_input = self.pr_title.clone();
        let pr_description_input = self.pr_description.clone();
        let window_handle = window.window_handle();

        self.busy = true;
        self.prepared = false;
        self.error = None;
        self.status = Some(match action {
            AgentShipAction::Commit => "Generating commit content…".to_string(),
            AgentShipAction::CommitPush => "Generating commit and branch content…".to_string(),
            AgentShipAction::CommitPushPr => {
                "Generating commit and pull request content…".to_string()
            }
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    prepare_agent_ship_content(
                        &generation_agent,
                        &repo,
                        current_branch.as_deref(),
                        create_branch,
                        &branch_name,
                        &agent_title,
                        &files,
                        &commit_message,
                        &pr_base_branch,
                        &pr_title,
                        &pr_description,
                        action,
                    )
                })
                .await;

            this.update(cx, |dialog, cx| {
                dialog.busy = false;
                match result {
                    Ok(preparation) => {
                        let branch_name = preparation.branch_name.clone();
                        let commit_message = preparation.commit_message.clone();
                        let pr_title = preparation.pr.as_ref().map(|pr| pr.title.clone());
                        let pr_body = preparation.pr.as_ref().map(|pr| pr.body.clone());
                        window_handle
                            .update(cx, |_, window, cx| {
                                if let Some(branch_name) = branch_name {
                                    branch_input.update(cx, |input, cx| {
                                        input.set_value(branch_name, window, cx);
                                        input.set_cursor_position(Position::new(0, 0), window, cx);
                                    });
                                }
                                commit_input.update(cx, |input, cx| {
                                    input.set_value(commit_message, window, cx);
                                });
                                if let Some(pr_title) = pr_title {
                                    pr_title_input.update(cx, |input, cx| {
                                        input.set_value(pr_title, window, cx);
                                    });
                                }
                                if let Some(pr_body) = pr_body {
                                    pr_description_input.update(cx, |input, cx| {
                                        input.set_value(pr_body, window, cx);
                                    });
                                }
                            })
                            .ok();
                        crate::notifications::play_generated_sound();
                        dialog.files_collapsed = true;
                        dialog.prepared = true;
                        dialog.error = None;
                        dialog.status =
                            Some("Review generated content, then ship when ready.".into());
                        crate::ui::onboarding::emit_for_project(
                            dialog.project_id,
                            crate::ui::onboarding::OnboardingEvent::ShipPrepared,
                            cx,
                        );
                    }
                    Err(error) => {
                        dialog.error = Some(format!("{error:#}"));
                        dialog.status = None;
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run(&mut self, action: AgentShipAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if !self.prepared {
            self.error = Some("Generate and review the ship content first.".into());
            cx.notify();
            return;
        }
        if self.branch.is_none() && !self.create_branch {
            self.error = Some("Current Git HEAD is not on a branch. Create a branch first.".into());
            cx.notify();
            return;
        }
        let files = self.included_files();
        if files.is_empty() {
            self.error = Some("No files selected for this scope.".into());
            cx.notify();
            return;
        }
        let staged_outside = self.staged_outside_scope();
        if !staged_outside.is_empty() {
            self.error = Some(format!(
                "Unstage files outside this scope before shipping: {}",
                staged_outside
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            cx.notify();
            return;
        }

        self.start_summary_maintenance(cx);

        if self.onboarding_demo {
            self.run_onboarding_demo_ship(files, window, cx);
            return;
        }

        let repo = self.repo_path.clone();
        // PR status polling must outlive the lane: a Solo's disposable folder
        // can't be the key, the project root can (same GitHub repo).
        let tracked_repo = self
            .solo_lane
            .as_ref()
            .map(|(project_root, _)| project_root.clone())
            .unwrap_or_else(|| self.repo_path.clone());
        let current_branch = self.branch.clone();
        let needs_upstream = self.needs_upstream;
        let create_branch = self.create_branch;
        let branch_name = self.branch_name.read(cx).value().trim().to_string();
        let commit_message = self.commit_message.read(cx).value().trim().to_string();
        let pr_base_branch = self.pr_base_branch.trim().to_string();
        let pr_title = self.pr_title.read(cx).value().trim().to_string();
        let pr_description = self.pr_description.read(cx).value().trim().to_string();
        let pending_commit = self.pending_commit.clone();
        let agent_id = self.agent_id;
        let project_id = self.project_id;
        let center = self.center.clone();
        let git = self.git.clone();
        let window_handle = window.window_handle();
        self.busy = true;
        self.error = None;
        self.status = Some(match action {
            AgentShipAction::Commit => "Committing selected files…".to_string(),
            AgentShipAction::CommitPush => "Committing and pushing selected files…".to_string(),
            AgentShipAction::CommitPushPr => {
                "Committing, pushing, and preparing pull request…".to_string()
            }
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    run_agent_ship_operation(
                        &repo,
                        agent_id,
                        project_id,
                        current_branch.as_deref(),
                        create_branch,
                        &branch_name,
                        needs_upstream,
                        &files,
                        &commit_message,
                        &pr_base_branch,
                        &pr_title,
                        &pr_description,
                        pending_commit,
                        action,
                    )
                })
                .await;

            this.update(cx, |dialog, cx| {
                dialog.busy = false;
                match result {
                    Ok(outcome) => {
                        dialog.pending_commit = None;
                        crate::ui::onboarding::emit_for_project(
                            project_id,
                            crate::ui::onboarding::OnboardingEvent::ShipCompleted,
                            cx,
                        );
                        dialog.status = Some(outcome.message.clone());
                        dialog.error = None;
                        if let Some(center) = center.upgrade() {
                            center.update(cx, |center, cx| {
                                center.attach_ship_commit_to_changed_files(
                                    agent_id,
                                    outcome.snapshot_id,
                                    outcome.commit_sha.clone(),
                                    cx,
                                );
                                center.append_agent_ship_result(agent_id, None, &outcome, cx);
                                if let Some(branch) = outcome.tracked_pr_branch.clone() {
                                    center.track_agent_ship_pr_branch(
                                        agent_id,
                                        tracked_repo.clone(),
                                        branch,
                                        cx,
                                    );
                                }
                                // A Solo's PR is its work leaving home — pack
                                // the lane up (branch and chat survive).
                                if outcome.pr_url.is_some() {
                                    center.finish_solo_ship(agent_id, cx);
                                }
                            });
                        }
                        git.update(cx, |git, cx| {
                            git.last_message = Some(outcome.message);
                            git.last_error = None;
                            git.refresh(cx);
                        });
                        let pr_url = outcome.pr_url.clone();
                        window_handle
                            .update(cx, |_, window, cx| {
                                window.close_dialog(cx);
                                if let Some(url) = pr_url {
                                    crate::ui::git::git_panel::open_url(&url);
                                }
                            })
                            .ok();
                    }
                    Err(error) => {
                        dialog.pending_commit = error.pending_commit.clone();
                        dialog.error = Some(error.to_string());
                        dialog.status = None;
                        git.update(cx, |git, cx| {
                            git.last_error = Some(error.to_string());
                            git.last_error_from_refresh = false;
                            git.refresh(cx);
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_onboarding_demo_ship(
        &mut self,
        files: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let repo = self.repo_path.clone();
        let current_branch = self.branch.clone();
        let branch = self.branch_name.read(cx).value().trim().to_string();
        let commit_message = self.commit_message.read(cx).value().trim().to_string();
        let pr_title = self.pr_title.read(cx).value().trim().to_string();
        let pr_description = self.pr_description.read(cx).value().trim().to_string();
        let agent_id = self.agent_id;
        let project_id = self.project_id;
        let center = self.center.clone();
        let git = self.git.clone();
        let window_handle = window.window_handle();
        self.busy = true;
        self.error = None;
        self.status = Some("Creating the local branch and commit…".into());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let operation_branch = branch.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    run_agent_ship_operation(
                        &repo,
                        agent_id,
                        project_id,
                        current_branch.as_deref(),
                        true,
                        &operation_branch,
                        false,
                        &files,
                        &commit_message,
                        "",
                        "",
                        "",
                        None,
                        AgentShipAction::Commit,
                    )
                })
                .await;

            this.update(cx, |dialog, cx| {
                dialog.busy = false;
                match result {
                    Ok(mut outcome) => {
                        let short_sha = outcome.commit_sha.chars().take(8).collect::<String>();
                        outcome.message = format!(
                            "Committed {short_sha} locally · simulated push and pull request #1"
                        );
                        outcome.action = "Local commit + simulated push + PR".into();
                        outcome.pr_url = Some(
                            "onboarding-demo://choro-playground/pull/1".to_string(),
                        );
                        outcome.pr_base_branch = Some("main".to_string());
                        outcome.pr_title = Some(pr_title.clone());
                        outcome.pr_body = Some(pr_description.clone());
                        outcome.tracked_pr_branch = None;

                        if let Some(center) = center.upgrade() {
                            let indicator_branch = outcome.branch.clone();
                            let indicator_title = pr_title.clone();
                            center.update(cx, |center, cx| {
                                center.attach_ship_commit_to_changed_files(
                                    agent_id,
                                    outcome.snapshot_id,
                                    outcome.commit_sha.clone(),
                                    cx,
                                );
                                center.append_agent_ship_result(agent_id, None, &outcome, cx);
                                center.agent_ship_prs.insert(
                                    agent_id,
                                    crate::ui::git::git_panel::BranchPullRequest {
                                        branch: indicator_branch,
                                        base_branch: "main".into(),
                                        head_oid: Some(outcome.commit_sha.clone()),
                                        number: 1,
                                        title: indicator_title,
                                        url: String::new(),
                                        state: "OPEN".into(),
                                        is_draft: false,
                                        merge_state_status: Some("CLEAN".into()),
                                        review_decision: None,
                                        check_state: crate::ui::git::git_panel::PullRequestCheckState::Unknown,
                                        auto_merge_enabled: false,
                                    },
                                );
                                cx.notify();
                            });
                        }
                        git.update(cx, |git, cx| {
                            git.last_message = Some(outcome.message.clone());
                            git.last_error = None;
                            git.refresh(cx);
                        });
                        crate::ui::onboarding::emit_for_project(
                            project_id,
                            crate::ui::onboarding::OnboardingEvent::ShipCompleted,
                            cx,
                        );
                        window_handle
                            .update(cx, |_, window, cx| window.close_dialog(cx))
                            .ok();
                    }
                    Err(error) => {
                        dialog.error = Some(format!("{error:#}"));
                        dialog.status = None;
                        git.update(cx, |git, cx| {
                            git.last_error = Some(format!("{error:#}"));
                            git.last_error_from_refresh = false;
                            git.refresh(cx);
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn run_auto(&mut self, action: AgentShipAction, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        if self.branch.is_none() && !self.create_branch {
            self.error = Some("Current Git HEAD is not on a branch. Create a branch first.".into());
            cx.notify();
            return;
        }
        let files = self.included_files();
        if files.is_empty() {
            self.error = Some("No files selected for this scope.".into());
            cx.notify();
            return;
        }
        let staged_outside = self.staged_outside_scope();
        if !staged_outside.is_empty() {
            self.error = Some(format!(
                "Unstage files outside this scope before shipping: {}",
                staged_outside
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
            cx.notify();
            return;
        }

        self.start_summary_maintenance(cx);

        let repo = self.repo_path.clone();
        // PR status polling must outlive the lane: a Solo's disposable folder
        // can't be the key, the project root can (same GitHub repo).
        let tracked_repo = self
            .solo_lane
            .as_ref()
            .map(|(project_root, _)| project_root.clone())
            .unwrap_or_else(|| self.repo_path.clone());
        let current_branch = self.branch.clone();
        let needs_upstream = self.needs_upstream;
        let create_branch = self.create_branch;
        let branch_name = self.branch_name.read(cx).value().trim().to_string();
        let commit_message = self.commit_message.read(cx).value().trim().to_string();
        let pr_base_branch = self.pr_base_branch.trim().to_string();
        let pr_title = self.pr_title.read(cx).value().trim().to_string();
        let pr_description = self.pr_description.read(cx).value().trim().to_string();
        let agent_title = self.agent_title.clone();
        let generation_agent = self.generation_agent.clone();
        let pending_commit = self.pending_commit.clone();
        let agent_id = self.agent_id;
        let project_id = self.project_id;
        let center = self.center.clone();
        let git = self.git.clone();
        let window_handle = window.window_handle();

        self.busy = true;
        self.prepared = false;
        self.error = None;
        self.status = Some(match action {
            AgentShipAction::Commit => "Generating content and committing…".to_string(),
            AgentShipAction::CommitPush => {
                "Generating content, committing, and pushing…".to_string()
            }
            AgentShipAction::CommitPushPr => {
                "Generating content, committing, pushing, and preparing PR…".to_string()
            }
        });
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let preparation = prepare_agent_ship_content(
                        &generation_agent,
                        &repo,
                        current_branch.as_deref(),
                        create_branch,
                        &branch_name,
                        &agent_title,
                        &files,
                        &commit_message,
                        &pr_base_branch,
                        &pr_title,
                        &pr_description,
                        action,
                    )?;
                    let resolved_branch = preparation
                        .branch_name
                        .as_deref()
                        .unwrap_or(branch_name.trim());
                    let resolved_pr_title = preparation
                        .pr
                        .as_ref()
                        .map(|pr| pr.title.as_str())
                        .unwrap_or(pr_title.trim());
                    let resolved_pr_body = preparation
                        .pr
                        .as_ref()
                        .map(|pr| pr.body.as_str())
                        .unwrap_or(pr_description.trim());
                    run_agent_ship_operation(
                        &repo,
                        agent_id,
                        project_id,
                        current_branch.as_deref(),
                        create_branch,
                        resolved_branch,
                        needs_upstream,
                        &files,
                        &preparation.commit_message,
                        &pr_base_branch,
                        resolved_pr_title,
                        resolved_pr_body,
                        pending_commit,
                        action,
                    )
                })
                .await;

            this.update(cx, |dialog, cx| {
                dialog.busy = false;
                match result {
                    Ok(outcome) => {
                        dialog.pending_commit = None;
                        crate::ui::onboarding::emit_for_project(
                            project_id,
                            crate::ui::onboarding::OnboardingEvent::ShipCompleted,
                            cx,
                        );
                        dialog.status = Some(outcome.message.clone());
                        dialog.error = None;
                        if let Some(center) = center.upgrade() {
                            center.update(cx, |center, cx| {
                                center.attach_ship_commit_to_changed_files(
                                    agent_id,
                                    outcome.snapshot_id,
                                    outcome.commit_sha.clone(),
                                    cx,
                                );
                                center.append_agent_ship_result(agent_id, None, &outcome, cx);
                                if let Some(branch) = outcome.tracked_pr_branch.clone() {
                                    center.track_agent_ship_pr_branch(
                                        agent_id,
                                        tracked_repo.clone(),
                                        branch,
                                        cx,
                                    );
                                }
                                // A Solo's PR is its work leaving home — pack
                                // the lane up (branch and chat survive).
                                if outcome.pr_url.is_some() {
                                    center.finish_solo_ship(agent_id, cx);
                                }
                            });
                        }
                        git.update(cx, |git, cx| {
                            git.last_message = Some(outcome.message);
                            git.last_error = None;
                            git.refresh(cx);
                        });
                        let pr_url = outcome.pr_url.clone();
                        window_handle
                            .update(cx, |_, window, cx| {
                                window.close_dialog(cx);
                                if let Some(url) = pr_url {
                                    crate::ui::git::git_panel::open_url(&url);
                                }
                            })
                            .ok();
                    }
                    Err(error) => {
                        dialog.pending_commit = error.pending_commit.clone();
                        dialog.error = Some(error.to_string());
                        dialog.status = None;
                        git.update(cx, |git, cx| {
                            git.last_error = Some(error.to_string());
                            git.last_error_from_refresh = false;
                            git.refresh(cx);
                        });
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ship_label(text: &'static str, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .text_size(crate::ui::design::text_ui())
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(crate::ui::design::t1(cx))
            .child(text)
    }

    /// Base-branch picker styled like the Git panel's branch selector: a toggle
    /// button that expands an inline, searchable popup. Select-only — it only
    /// picks the PR base, so there is no checkout/create/fetch behavior.
    fn render_base_branch_selector(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let expanded = self.pr_base_branch_expanded;
        let has_branch = !self.pr_base_branch.trim().is_empty();
        v_flex()
            .w_full()
            .gap_1()
            .child(
                // Matches the current-branch chip above and the Git panel's
                // branch button: branch icon, left-aligned name, chevron.
                h_flex()
                    .id("ship-pr-base-branch")
                    .w_full()
                    .items_center()
                    .gap_1p5()
                    .h(crate::ui::design::subhead_h())
                    .px_2p5()
                    .rounded(px(crate::ui::style::RADIUS))
                    .border_1()
                    .border_color(if expanded {
                        crate::ui::design::accent(cx).opacity(0.6)
                    } else {
                        crate::ui::design::line(cx)
                    })
                    .bg(crate::ui::style::surface(cx))
                    .cursor_pointer()
                    .hover(|row| row.border_color(crate::ui::design::accent(cx).opacity(0.45)))
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.pr_base_branch_expanded = !this.pr_base_branch_expanded;
                        if this.pr_base_branch_expanded {
                            this.pr_base_branch_query
                                .update(cx, |input, cx| input.set_value("", window, cx));
                        }
                        cx.notify();
                    }))
                    .child(branch_icon(crate::ui::design::t3(cx)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(crate::ui::design::text_body())
                            .truncate()
                            .text_color(if has_branch {
                                crate::ui::design::t1(cx)
                            } else {
                                crate::ui::design::t3(cx)
                            })
                            .child(SharedString::from(if has_branch {
                                self.pr_base_branch.clone()
                            } else {
                                "Select base branch".to_string()
                            })),
                    )
                    .child(
                        gpui_component::Icon::new(if expanded {
                            IconName::ChevronUp
                        } else {
                            IconName::ChevronDown
                        })
                        .size(crate::ui::design::icon_sm())
                        .text_color(crate::ui::design::t3(cx)),
                    ),
            )
            .when(expanded, |col| col.child(self.render_base_branch_popup(cx)))
    }

    /// The expanded base-branch popup: a scrollable list of candidate base
    /// branches (each enriched with `author · time · summary` from the git
    /// snapshot) filtered by the search field at the bottom. Mirrors the Git
    /// panel's `render_branch_list` styling.
    fn render_base_branch_popup(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let needle = self
            .pr_base_branch_query
            .read(cx)
            .value()
            .trim()
            .to_lowercase();
        let snapshot_branches = self
            .git
            .read(cx)
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.branches.clone())
            .unwrap_or_default();
        let detail_for = |name: &str| -> SharedString {
            let info = snapshot_branches
                .iter()
                .find(|branch| branch.name == name)
                .or_else(|| {
                    let remote = format!("origin/{name}");
                    snapshot_branches
                        .iter()
                        .find(|branch| branch.name == remote)
                });
            match info {
                Some(info) => {
                    let mut parts: Vec<String> = Vec::new();
                    if !info.tip_author.is_empty() {
                        parts.push(info.tip_author.clone());
                    }
                    let when = crate::ui::git::git_panel::relative_time(info.tip_time);
                    if !when.is_empty() {
                        parts.push(when);
                    }
                    if !info.tip_summary.is_empty() {
                        parts.push(info.tip_summary.clone());
                    }
                    parts.join(" · ").into()
                }
                None => SharedString::default(),
            }
        };

        let selected = self.pr_base_branch.clone();
        let rows: Vec<(String, SharedString, bool)> = self
            .pr_base_branch_options
            .iter()
            .filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle))
            .map(|name| (name.clone(), detail_for(name), *name == selected))
            .collect();

        v_flex()
            .w_full()
            .min_w(px(0.))
            .rounded(crate::ui::design::r_md())
            .border_1()
            .border_color(crate::ui::design::line(cx).opacity(0.42))
            .bg(crate::ui::design::focus(cx))
            .text_color(crate::ui::design::t1(cx))
            .shadow_lg()
            .overflow_hidden()
            .child(
                v_flex()
                    .id("ship-base-branch-scroll")
                    .max_h(px(240.))
                    .overflow_y_scroll()
                    .p_1()
                    .gap_0p5()
                    .when(rows.is_empty(), |list| {
                        list.child(
                            div()
                                .px_2()
                                .py_1()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child("No matching branches"),
                        )
                    })
                    .children(rows.into_iter().enumerate().map(
                        |(ix, (name, detail, is_selected))| {
                            let value = name.clone();
                            let label: SharedString = name.into();
                            h_flex()
                                .id(("ship-base-branch-row", ix))
                                .w_full()
                                .px_2()
                                .py_0p5()
                                .gap_2()
                                .items_center()
                                .rounded(crate::ui::design::r_sm())
                                .cursor_pointer()
                                .when(is_selected, |row| row.bg(crate::ui::design::surface_2(cx)))
                                .hover(|row| row.bg(crate::ui::design::surface_2(cx)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.pr_base_branch = value.clone();
                                    if this.open_pr
                                        && this.branch.as_deref().is_some_and(|branch| {
                                            validate_agent_ship_pr_branches(branch, &value).is_err()
                                        })
                                    {
                                        this.create_branch = true;
                                    }
                                    this.pr_base_branch_expanded = false;
                                    this.reset_prepared();
                                    this.error = None;
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .w(px(18.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(if is_selected {
                                            gpui_component::Icon::new(IconName::Check)
                                                .size(crate::ui::design::icon_sm())
                                                .text_color(crate::ui::design::accent(cx))
                                        } else {
                                            gpui_component::Icon::new(IconName::Replace)
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
                                                .child(label),
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
                        },
                    )),
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
                            .child(Input::new(&self.pr_base_branch_query)),
                    )
                    .child(
                        gpui_component::Icon::new(IconName::Search)
                            .size(crate::ui::design::icon())
                            .text_color(crate::ui::design::t3(cx)),
                    ),
            )
    }

    fn ship_segment(
        id: &'static str,
        label: &'static str,
        selected: bool,
        disabled: bool,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let foreground = crate::ui::design::t1(cx);
        let muted = crate::ui::design::t3(cx);
        let text_color = if selected { foreground } else { muted };

        let mut seg = h_flex()
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
            .text_color(text_color)
            .when(selected, |seg| seg.bg(crate::ui::design::surface_2(cx)))
            .child(div().truncate().child(label));

        if !disabled {
            seg = seg.cursor_pointer().when(!selected, |seg| {
                seg.hover(|seg| {
                    seg.bg(crate::ui::design::hover(cx).opacity(0.48))
                        .text_color(foreground)
                })
            });
            seg = seg.on_click(cx.listener(move |this, _, window, cx| handler(this, window, cx)));
        }

        let spotlight = match id {
            "ship-branch-new" => Some(crate::ui::onboarding::SpotlightTarget::ShipNewBranch),
            "ship-scope-all" => Some(crate::ui::onboarding::SpotlightTarget::ShipAllChanges),
            _ => None,
        };
        if let Some(spotlight) = spotlight {
            seg = seg
                .relative()
                .child(crate::ui::onboarding::target_marker(spotlight, cx));
        }

        seg.into_any_element()
    }

    fn ship_check(
        id: &'static str,
        label: &'static str,
        checked: bool,
        disabled: bool,
        handler: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let foreground = crate::ui::design::t1(cx);
        let muted = crate::ui::design::t3(cx);
        let primary = crate::ui::design::accent(cx);

        let mut row = h_flex()
            .id(id)
            .items_center()
            .gap_1p5()
            .text_size(crate::ui::design::text_ui())
            .text_color(if checked { foreground } else { muted })
            .child(
                div()
                    .size(crate::ui::design::icon())
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(crate::ui::design::r_xs())
                    .border_1()
                    .border_color(if checked {
                        primary
                    } else {
                        crate::ui::design::t3(cx).opacity(0.5)
                    })
                    .when(checked, |b| {
                        b.bg(primary).child(
                            gpui_component::Icon::new(IconName::Check)
                                .size(crate::ui::design::icon_sm())
                                .text_color(crate::ui::design::on_accent(cx)),
                        )
                    }),
            )
            .child(label);

        if !disabled {
            row = row
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| handler(this, window, cx)));
        }

        if id == "ship-toggle-push" {
            row = row.relative().child(crate::ui::onboarding::target_marker(
                crate::ui::onboarding::SpotlightTarget::ShipPush,
                cx,
            ));
        }

        row.into_any_element()
    }
}

pub(super) struct AgentShipOutcome {
    message: String,
    action: String,
    branch: String,
    pr_base_branch: Option<String>,
    pr_url: Option<String>,
    pr_title: Option<String>,
    pr_body: Option<String>,
    tracked_pr_branch: Option<String>,
    snapshot_id: Option<Uuid>,
    commit_sha: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentShipPendingCommit {
    branch: String,
    commit_sha: String,
    commit_sha_short: String,
    snapshot_id: Option<Uuid>,
    commit_message: String,
    files: Vec<PathBuf>,
    pushed: bool,
}

#[derive(Debug)]
struct AgentShipOperationFailure {
    error: anyhow::Error,
    pending_commit: Option<AgentShipPendingCommit>,
}

impl AgentShipOperationFailure {
    fn before_commit(error: impl Into<anyhow::Error>) -> Self {
        Self {
            error: error.into(),
            pending_commit: None,
        }
    }

    fn after_commit(error: impl Into<anyhow::Error>, pending: AgentShipPendingCommit) -> Self {
        Self {
            error: error.into(),
            pending_commit: Some(pending),
        }
    }
}

impl std::fmt::Display for AgentShipOperationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{:#}", self.error)
    }
}

impl std::error::Error for AgentShipOperationFailure {}

impl From<anyhow::Error> for AgentShipOperationFailure {
    fn from(error: anyhow::Error) -> Self {
        Self::before_commit(error)
    }
}

struct AgentShipPreparation {
    branch_name: Option<String>,
    commit_message: String,
    pr: Option<crate::ui::git::git_panel::GeneratedPullRequest>,
}
