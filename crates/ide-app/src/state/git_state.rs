use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{AppContext, Context, Entity};
use ide_core::git::{CommitInfo, GitSnapshot};
use ide_core::{GitWatcher, Project, ProjectId, WorktreeWatcher};

use crate::state::{Workspace, WorkspaceEvent};

/// Live git data for one project, refreshed in the background and on
/// `.git` changes (branch switch, commit, stage — even from outside the app).
pub struct GitState {
    pub repo_path: PathBuf,
    pub snapshot: Option<GitSnapshot>,
    pub is_repo: bool,
    pub is_refreshing: bool,
    /// A mutating operation (commit, checkout, push...) is in flight.
    pub is_busy: bool,
    pub last_error: Option<String>,
    /// True when `last_error` came from a background refresh (a snapshot read
    /// failure) rather than a user operation. A successful refresh clears only
    /// its own errors, so operation errors (checkout, push…) persist until the
    /// next operation instead of flashing away under the git watcher.
    pub last_error_from_refresh: bool,
    /// Outcome of the last operation, shown inline in the git panel.
    pub last_message: Option<String>,
    /// Recent commits on HEAD, loaded with each refresh.
    pub history: Vec<CommitInfo>,
    /// Number of entries in the stash.
    pub stash_count: usize,
    refresh_queued: bool,
    _watcher: Option<GitWatcher>,
}

impl GitState {
    pub fn new(repo_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let is_repo = repo_path.join(".git").exists();
        let mut state = Self {
            repo_path,
            snapshot: None,
            is_repo,
            is_refreshing: false,
            is_busy: false,
            last_error: None,
            last_error_from_refresh: false,
            last_message: None,
            history: Vec::new(),
            stash_count: 0,
            refresh_queued: false,
            _watcher: None,
        };
        if is_repo {
            if let Err(error) = ide_core::git::ensure_local_dependency_excludes(&state.repo_path) {
                eprintln!(
                    "failed to install local dependency excludes for {:?}: {error:#}",
                    state.repo_path
                );
            }
            state.start_git_watcher(cx);
            state.refresh(cx);
        }
        state
    }

    fn start_git_watcher(&mut self, cx: &mut Context<Self>) {
        if self._watcher.is_some() {
            return;
        }

        match GitWatcher::new(&self.repo_path) {
            Ok((watcher, ticks)) => {
                // Bridge the blocking receiver to the entity: each tick
                // triggers a refresh on the UI thread.
                cx.spawn(async move |this, cx| {
                    let mut ticks = ticks;
                    loop {
                        let next = cx
                            .background_executor()
                            .spawn(async move { ticks.recv().map(move |_| ticks) })
                            .await;
                        match next {
                            Ok(returned) => {
                                ticks = returned;
                                if this.update(cx, |state, cx| state.refresh(cx)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .detach();
                self._watcher = Some(watcher);
            }
            Err(error) => {
                eprintln!("git watcher failed for {:?}: {error:#}", self.repo_path);
            }
        }
    }

    pub fn branch_label(&self) -> Option<String> {
        self.snapshot.as_ref().map(|s| s.head.display_name())
    }

    /// Runs a blocking git operation on the background executor, then
    /// records the outcome and refreshes. `op` returns an optional success message.
    fn run_op<F>(&mut self, op: F, cx: &mut Context<Self>)
    where
        F: FnOnce(PathBuf) -> anyhow::Result<Option<String>> + Send + 'static,
    {
        if self.is_busy {
            return;
        }
        self.is_busy = true;
        self.last_error = None;
        self.last_error_from_refresh = false;
        self.last_message = None;
        cx.notify();
        let path = self.repo_path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { op(path) })
                .await;
            this.update(cx, |state, cx| {
                state.is_busy = false;
                match result {
                    Ok(message) => state.last_message = message,
                    Err(error) => {
                        state.last_error = Some(format!("{error:#}"));
                        // A user operation failed — keep this error visible; the
                        // refresh below (and the git watcher) must not clear it.
                        state.last_error_from_refresh = false;
                    }
                }
                state.refresh(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Runs the one-time setup for a folder that is not a repository yet.
    /// Detection happens after both success and failure because a multi-step
    /// publish can initialize Git before a later authentication or push error.
    pub(crate) fn run_repository_setup<F>(&mut self, op: F, cx: &mut Context<Self>)
    where
        F: FnOnce(PathBuf) -> anyhow::Result<Option<String>> + Send + 'static,
    {
        if self.is_busy || self.is_repo {
            return;
        }
        self.is_busy = true;
        self.last_error = None;
        self.last_error_from_refresh = false;
        self.last_message = None;
        cx.notify();

        let path = self.repo_path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move { op(path) }
                })
                .await;
            this.update(cx, |state, cx| {
                state.is_busy = false;
                match result {
                    Ok(message) => state.last_message = message,
                    Err(error) => {
                        state.last_error = Some(format!("{error:#}"));
                        state.last_error_from_refresh = false;
                    }
                }

                // A publish consists of init, commit, remote creation, and push.
                // If anything after init fails, keep the usable local repository
                // and transition to the regular source-control view.
                state.is_repo = path.join(".git").exists();
                if state.is_repo {
                    if let Err(error) = ide_core::git::ensure_local_dependency_excludes(&path) {
                        eprintln!(
                            "failed to install local dependency excludes for {path:?}: {error:#}"
                        );
                    }
                    state.start_git_watcher(cx);
                    state.refresh(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn initialize_repository(&mut self, cx: &mut Context<Self>) {
        self.run_repository_setup(
            |repo| {
                let output = std::process::Command::new("git")
                    .args(["-c", "init.defaultBranch=main", "init"])
                    .current_dir(&repo)
                    .output()
                    .map_err(|error| anyhow::anyhow!("Failed to run Git: {error}"))?;
                if !output.status.success() {
                    let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
                    anyhow::bail!(if error.is_empty() {
                        "Failed to initialize the Git repository".to_string()
                    } else {
                        error
                    });
                }
                Ok(Some("Initialized Git repository".to_string()))
            },
            cx,
        );
    }

    pub fn stage(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| ide_core::git::write::stage(&repo, &[&file]).map(|_| None),
            cx,
        );
    }

    pub fn unstage(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| ide_core::git::write::unstage(&repo, &[&file]).map(|_| None),
            cx,
        );
    }

    pub fn stage_all(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .snapshot
            .as_ref()
            .map(|s| {
                s.entries
                    .iter()
                    .filter(|e| e.unstaged.is_some())
                    .map(|e| e.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return;
        }
        self.run_op(
            move |repo| {
                let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
                ide_core::git::write::stage(&repo, &refs).map(|_| None)
            },
            cx,
        );
    }

    pub fn unstage_all(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .snapshot
            .as_ref()
            .map(|s| s.staged().map(|e| e.path.clone()).collect())
            .unwrap_or_default();
        if paths.is_empty() {
            return;
        }
        self.run_op(
            move |repo| {
                let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
                ide_core::git::write::unstage(&repo, &refs).map(|_| None)
            },
            cx,
        );
    }

    /// Discards unstaged changes to one file (restores it from the index).
    pub fn discard(&mut self, file: PathBuf, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let name = file.display().to_string();
                ide_core::git::write::discard(&repo, &[&file])
                    .map(|_| Some(format!("Discarded changes in {name}")))
            },
            cx,
        );
    }

    /// Discards all unstaged and untracked worktree changes.
    pub fn discard_all(&mut self, cx: &mut Context<Self>) {
        let paths: Vec<PathBuf> = self
            .snapshot
            .as_ref()
            .map(|s| {
                s.entries
                    .iter()
                    .filter(|entry| entry.unstaged.is_some())
                    .map(|entry| entry.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        if paths.is_empty() {
            return;
        }
        let count = paths.len();
        self.run_op(
            move |repo| {
                let refs: Vec<&std::path::Path> = paths.iter().map(|p| p.as_path()).collect();
                ide_core::git::write::discard(&repo, &refs)
                    .map(|_| Some(format!("Discarded changes in {count} file(s)")))
            },
            cx,
        );
    }

    /// Creates a branch at HEAD and switches to it.
    pub fn create_branch(&mut self, name: String, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                ide_core::git::write::create_branch(&repo, &name)
                    .map(|_| Some(format!("Created branch {name}")))
            },
            cx,
        );
    }

    /// Stage all tracked changes (not untracked files) and commit — Zed's
    /// "commit tracked" behavior.
    pub fn commit_all(&mut self, message: String, cx: &mut Context<Self>) {
        let tracked: Vec<PathBuf> = self
            .snapshot
            .as_ref()
            .map(|s| {
                s.entries
                    .iter()
                    .filter(|e| {
                        e.unstaged
                            .map(|k| k != ide_core::git::ChangeKind::Untracked)
                            .unwrap_or(false)
                    })
                    .map(|e| e.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.run_op(
            move |repo| {
                if !tracked.is_empty() {
                    let refs: Vec<&std::path::Path> = tracked.iter().map(|p| p.as_path()).collect();
                    ide_core::git::write::stage(&repo, &refs)?;
                }
                ide_core::git::write::commit(&repo, &message)
                    .map(|commit| Some(format!("Committed {}", commit.sha_short)))
            },
            cx,
        );
    }

    /// Soft-reset the last commit, keeping its changes staged.
    pub fn uncommit(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::uncommit(&repo)?;
                if output.success {
                    Ok(Some("Uncommitted — changes kept staged".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn stash_all(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::stash_all(&repo)?;
                if output.success {
                    Ok(Some("Stashed all changes".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn stash_pop(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::stash_pop(&repo)?;
                if output.success {
                    Ok(Some("Popped latest stash".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn stash_apply(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::stash_apply(&repo)?;
                if output.success {
                    Ok(Some("Applied latest stash".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn commit(&mut self, message: String, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                ide_core::git::write::commit(&repo, &message)
                    .map(|commit| Some(format!("Committed {}", commit.sha_short)))
            },
            cx,
        );
    }

    pub fn checkout(&mut self, branch: String, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                ide_core::git::write::checkout_branch_or_remote(&repo, &branch)
                    .map(|local| Some(format!("Switched to {local}")))
            },
            cx,
        );
    }

    pub fn push(&mut self, cx: &mut Context<Self>) {
        let branch = self.snapshot.as_ref().and_then(|s| s.head.branch.clone());
        let needs_upstream = self
            .snapshot
            .as_ref()
            .and_then(|s| s.branches.iter().find(|b| b.is_head))
            .map(|b| b.upstream.is_none())
            .unwrap_or(false);
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::push(&repo, branch.as_deref(), needs_upstream)?;
                if output.success {
                    Ok(Some(match branch {
                        Some(branch) if needs_upstream => format!("Published {branch} to origin"),
                        Some(branch) => format!("Pushed {branch} to origin"),
                        None => "Pushed".to_string(),
                    }))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn pull_rebase(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::pull_rebase(&repo)?;
                if output.success {
                    Ok(Some("Pulled (rebase)".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn push_force(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::push_force(&repo)?;
                if output.success {
                    Ok(Some("Force pushed (with lease)".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn pull(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::pull(&repo)?;
                if output.success {
                    Ok(Some("Pulled".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn fetch(&mut self, cx: &mut Context<Self>) {
        self.run_op(
            move |repo| {
                let output = ide_core::git::remote::fetch(&repo)?;
                if output.success {
                    Ok(Some("Fetched".to_string()))
                } else {
                    anyhow::bail!("{}", output.message())
                }
            },
            cx,
        );
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if !self.is_repo {
            return;
        }
        if self.is_refreshing {
            self.refresh_queued = true;
            return;
        }
        self.is_refreshing = true;
        let path = self.repo_path.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let snapshot = ide_core::git::read_snapshot(&path);
                    let history = ide_core::git::list_commits(&path, 100).unwrap_or_default();
                    let stash_count = ide_core::git::remote::stash_count(&path);
                    (snapshot, history, stash_count)
                })
                .await;
            this.update(cx, |state, cx| {
                let (snapshot, history, stash_count) = result;
                state.is_refreshing = false;
                state.history = history;
                state.stash_count = stash_count;
                match snapshot {
                    Ok(snapshot) => {
                        state.snapshot = Some(snapshot);
                        // Only clear an error the refresh itself produced; an
                        // operation error (checkout, push…) survives so it stays
                        // readable instead of flashing away.
                        if state.last_error_from_refresh {
                            state.last_error = None;
                            state.last_error_from_refresh = false;
                        }
                    }
                    Err(error) => {
                        state.last_error = Some(format!("{error:#}"));
                        state.last_error_from_refresh = true;
                    }
                }
                cx.notify();
                if std::mem::take(&mut state.refresh_queued) {
                    state.refresh(cx);
                }
            })
            .ok();
        })
        .detach();
    }
}

/// Holds one `GitState` per project, kept in sync with the workspace.
pub struct GitStates {
    workspace: Entity<Workspace>,
    states: HashMap<ProjectId, Entity<GitState>>,
    active_worktree: Option<ActiveWorktreeWatcher>,
}

struct ActiveWorktreeWatcher {
    project_id: ProjectId,
    _watcher: WorktreeWatcher,
}

impl GitStates {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&workspace, |this: &mut Self, workspace, event, cx| {
            if matches!(event, WorkspaceEvent::ProjectsChanged) {
                let projects = workspace.read(cx).projects.clone();
                this.sync(&projects, cx);
            }
            if matches!(
                event,
                WorkspaceEvent::ProjectsChanged | WorkspaceEvent::ActiveChanged
            ) {
                let (projects, active) = {
                    let workspace = workspace.read(cx);
                    (workspace.projects.clone(), workspace.active)
                };
                this.sync_active_worktree(&projects, active, cx);
            }
        })
        .detach();

        let projects = workspace.read(cx).projects.clone();
        let active = workspace.read(cx).active;
        let mut this = Self {
            workspace,
            states: HashMap::new(),
            active_worktree: None,
        };
        this.sync(&projects, cx);
        this.sync_active_worktree(&projects, active, cx);
        this
    }

    pub fn get(&self, id: ProjectId) -> Option<Entity<GitState>> {
        self.states.get(&id).cloned()
    }

    pub fn refresh_all(&self, cx: &mut Context<Self>) {
        for state in self.states.values() {
            state.update(cx, |state, cx| state.refresh(cx));
        }
    }

    fn sync(&mut self, projects: &[Project], cx: &mut Context<Self>) {
        self.states
            .retain(|id, _| projects.iter().any(|p| p.id == *id));
        for project in projects {
            self.states.entry(project.id).or_insert_with(|| {
                let path = project.path.clone();
                let state = cx.new(|cx| GitState::new(path, cx));
                // Cascade child updates so observers of GitStates re-render.
                // A folder can become a repository while Choro is open, so the
                // first child update after init also installs the active
                // worktree watcher that startup intentionally skipped.
                cx.observe(&state, |this, _, cx| {
                    let (projects, active) = {
                        let workspace = this.workspace.read(cx);
                        (workspace.projects.clone(), workspace.active)
                    };
                    this.sync_active_worktree(&projects, active, cx);
                    cx.notify();
                })
                .detach();
                state
            });
        }
        cx.notify();
    }

    fn sync_active_worktree(
        &mut self,
        projects: &[Project],
        active: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) {
        if self
            .active_worktree
            .as_ref()
            .map(|watcher| watcher.project_id)
            == active
        {
            return;
        }

        self.active_worktree = None;

        let Some(project_id) = active else {
            return;
        };
        let Some(project) = projects.iter().find(|project| project.id == project_id) else {
            return;
        };
        if !project.path.join(".git").exists() {
            return;
        }

        match WorktreeWatcher::new(&project.path) {
            Ok((watcher, ticks)) => {
                cx.spawn(async move |this, cx| {
                    let mut ticks = ticks;
                    loop {
                        let next = cx
                            .background_executor()
                            .spawn(async move { ticks.recv().map(move |_| ticks) })
                            .await;
                        match next {
                            Ok(returned) => {
                                ticks = returned;
                                if this
                                    .update(cx, |states, cx| {
                                        if states
                                            .active_worktree
                                            .as_ref()
                                            .is_some_and(|watcher| watcher.project_id == project_id)
                                        {
                                            if let Some(state) = states.states.get(&project_id) {
                                                state.update(cx, |state, cx| state.refresh(cx));
                                            }
                                        }
                                    })
                                    .is_err()
                                {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .detach();
                self.active_worktree = Some(ActiveWorktreeWatcher {
                    project_id,
                    _watcher: watcher,
                });
            }
            Err(error) => {
                eprintln!("worktree watcher failed for {:?}: {error:#}", project.path);
            }
        }
    }
}
