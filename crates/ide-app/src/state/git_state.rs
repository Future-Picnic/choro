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
    _worktree_watcher: Option<WorktreeWatcher>,
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
            _worktree_watcher: None,
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

    /// Creates a Git state that also refreshes when ordinary worktree files
    /// change. The project-level GitStates collection owns this watcher for the
    /// active project; standalone Solo-lane states need to own it themselves.
    pub(crate) fn new_with_worktree_watcher(repo_path: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut state = Self::new(repo_path, cx);
        if state.is_repo {
            state.start_worktree_watcher(cx);
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

    fn start_worktree_watcher(&mut self, cx: &mut Context<Self>) {
        if self._worktree_watcher.is_some() {
            return;
        }

        match WorktreeWatcher::new(&self.repo_path) {
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
                                if this.update(cx, |state, cx| state.refresh(cx)).is_err() {
                                    break;
                                }
                            }
                            Err(_) => break,
                        }
                    }
                })
                .detach();
                self._worktree_watcher = Some(watcher);
            }
            Err(error) => {
                eprintln!(
                    "worktree watcher failed for standalone Git state {:?}: {error:#}",
                    self.repo_path
                );
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

/// Holds one `GitState` per discovered repository, grouped by opened project.
pub struct GitStates {
    workspace: Entity<Workspace>,
    states: HashMap<ProjectId, Vec<(PathBuf, Entity<GitState>)>>,
    active_repositories: HashMap<ProjectId, PathBuf>,
    active_worktree: Option<ActiveWorktreeWatcher>,
    /// Drops stale background-discovery results when projects change again
    /// while a previous discovery walk is still running.
    sync_seq: u64,
}

struct ActiveWorktreeWatcher {
    project_id: ProjectId,
    repo_path: PathBuf,
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
            active_repositories: HashMap::new(),
            active_worktree: None,
            sync_seq: 0,
        };
        this.sync(&projects, cx);
        this.sync_active_worktree(&projects, active, cx);
        this
    }

    pub fn get(&self, id: ProjectId) -> Option<Entity<GitState>> {
        let states = self.states.get(&id)?;
        let selected = self.active_repositories.get(&id);
        selected
            .and_then(|path| states.iter().find(|(repo_path, _)| repo_path == path))
            .or_else(|| states.first())
            .map(|(_, state)| state.clone())
    }

    pub fn repositories(&self, id: ProjectId) -> Vec<Entity<GitState>> {
        self.states
            .get(&id)
            .map(|states| states.iter().map(|(_, state)| state.clone()).collect())
            .unwrap_or_default()
    }

    pub fn get_for_path(&self, id: ProjectId, path: &std::path::Path) -> Option<Entity<GitState>> {
        self.states
            .get(&id)?
            .iter()
            .find(|(repo_path, _)| repo_path == path)
            .map(|(_, state)| state.clone())
    }

    pub fn active_repository_path(&self, id: ProjectId) -> Option<PathBuf> {
        let states = self.states.get(&id)?;
        let selected = self.active_repositories.get(&id);
        selected
            .and_then(|path| states.iter().find(|(repo_path, _)| repo_path == path))
            .or_else(|| states.first())
            .map(|(path, _)| path.clone())
    }

    pub fn set_active_repository(&mut self, id: ProjectId, path: PathBuf, cx: &mut Context<Self>) {
        if self.get_for_path(id, &path).is_none()
            || self.active_repositories.get(&id) == Some(&path)
        {
            return;
        }
        self.active_repositories.insert(id, path);
        self.active_worktree = None;
        let (projects, active) = {
            let workspace = self.workspace.read(cx);
            (workspace.projects.clone(), workspace.active)
        };
        self.sync_active_worktree(&projects, active, cx);
        if let Some(git) = self.get(id) {
            git.update(cx, |git, cx| git.refresh(cx));
        }
        cx.notify();
    }

    pub fn refresh_all(&self, cx: &mut Context<Self>) {
        for states in self.states.values() {
            for (_, state) in states {
                state.update(cx, |state, cx| state.refresh(cx));
            }
        }
    }

    fn sync(&mut self, projects: &[Project], cx: &mut Context<Self>) {
        self.sync_seq = self.sync_seq.wrapping_add(1);
        let seq = self.sync_seq;
        let targets: Vec<(ProjectId, PathBuf)> = projects
            .iter()
            .map(|project| (project.id, project.path.clone()))
            .collect();
        cx.spawn(async move |this, cx| {
            // Discovery walks every project tree (depth-bounded read_dir per
            // directory) — far too slow for the UI thread on large repos.
            let discovered = cx
                .background_executor()
                .spawn(async move {
                    targets
                        .into_iter()
                        .map(|(id, path)| {
                            let mut found = ide_core::git::discover_repositories(&path);
                            // A folder with no repository keeps the existing
                            // setup experience.
                            if found.is_empty() {
                                found.push(path);
                            }
                            (id, found)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| {
                if this.sync_seq == seq {
                    this.apply_discovered(discovered, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_discovered(
        &mut self,
        discovered: Vec<(ProjectId, Vec<PathBuf>)>,
        cx: &mut Context<Self>,
    ) {
        self.states
            .retain(|id, _| discovered.iter().any(|(project_id, _)| project_id == id));
        self.active_repositories
            .retain(|id, _| discovered.iter().any(|(project_id, _)| project_id == id));
        for (project_id, paths) in discovered {
            let existing = self.states.remove(&project_id).unwrap_or_default();
            let mut next = Vec::with_capacity(paths.len());
            for path in paths {
                if let Some((_, state)) = existing.iter().find(|(repo_path, _)| repo_path == &path)
                {
                    next.push((path, state.clone()));
                    continue;
                }
                let state_path = path.clone();
                let state = cx.new(|cx| GitState::new(state_path, cx));
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
                next.push((path, state));
            }
            let selected_is_valid = self
                .active_repositories
                .get(&project_id)
                .is_some_and(|path| next.iter().any(|(repo_path, _)| repo_path == path));
            if !selected_is_valid {
                if let Some(path) = next.first().map(|(path, _)| path.clone()) {
                    self.active_repositories.insert(project_id, path);
                }
            }
            self.states.insert(project_id, next);
        }
        cx.notify();
    }

    fn sync_active_worktree(
        &mut self,
        projects: &[Project],
        active: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) {
        if self.active_worktree.as_ref().is_some_and(|watcher| {
            Some(watcher.project_id) == active
                && self.active_repository_path(watcher.project_id).as_ref()
                    == Some(&watcher.repo_path)
        }) {
            return;
        }

        self.active_worktree = None;

        let Some(project_id) = active else {
            return;
        };
        let Some(_project) = projects.iter().find(|project| project.id == project_id) else {
            return;
        };
        let Some(repo_path) = self.active_repository_path(project_id) else {
            return;
        };
        if !repo_path.join(".git").exists() {
            return;
        }

        match WorktreeWatcher::new(&repo_path) {
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
                                            if let Some(state) = states.get(project_id) {
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
                    repo_path,
                    _watcher: watcher,
                });
            }
            Err(error) => {
                eprintln!("worktree watcher failed for {:?}: {error:#}", repo_path);
            }
        }
    }
}
