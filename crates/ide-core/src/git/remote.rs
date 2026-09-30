use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Result};

const GIT_OPERATION_TIMEOUT: Duration = Duration::from_secs(120);

/// Result of a `git` CLI network operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl RemoteOutput {
    pub fn message(&self) -> String {
        let text = if self.stderr.trim().is_empty() {
            &self.stdout
        } else {
            &self.stderr
        };
        text.trim().to_string()
    }
}

/// Network operations go through the system `git` CLI so the user's
/// ssh-agent, credential helpers and hooks all work as in their shell.
fn run_git(repo_path: &Path, args: &[&str]) -> Result<RemoteOutput> {
    run_git_with_timeout(repo_path, args, GIT_OPERATION_TIMEOUT)
}

fn run_git_with_timeout(
    repo_path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<RemoteOutput> {
    crate::blocking_guard::debug_warn_if_ui_thread("git::remote::run_git");
    let mut command = Command::new("git");
    super::accounts::configure_selected_account(&mut command, repo_path, args);
    command
        .args(args)
        .current_dir(repo_path)
        // Fail fast instead of hanging on an interactive credential prompt.
        .env("GIT_TERMINAL_PROMPT", "0");
    let operation = args.first().copied().unwrap_or("operation");
    let output = crate::process::output_with_timeout(&mut command, timeout).map_err(|error| {
        anyhow!("Git {operation} failed: {error:#}. Check the network or remote and try again.")
    })?;
    Ok(RemoteOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub fn push(repo_path: &Path, branch: Option<&str>, set_upstream: bool) -> Result<RemoteOutput> {
    let mut args = vec!["push"];
    if set_upstream {
        args.push("-u");
        args.push("origin");
        if let Some(branch) = branch {
            args.push(branch);
        }
    }
    run_git(repo_path, &args)
}

pub fn pull(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["pull", "--ff-only"])
}

pub fn pull_rebase(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["pull", "--rebase"])
}

pub fn push_force(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["push", "--force-with-lease"])
}

pub fn fetch(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["fetch", "--prune"])
}

/// Stash all changes (staged, unstaged and untracked).
pub fn stash_all(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["stash", "push", "--include-untracked"])
}

/// Apply the latest stash and remove it from the stash list.
pub fn stash_pop(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["stash", "pop"])
}

/// Apply the latest stash but keep it in the stash list.
pub fn stash_apply(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["stash", "apply"])
}

/// Number of entries in the stash.
pub fn stash_count(repo_path: &Path) -> usize {
    run_git(repo_path, &["stash", "list", "--format=%gd"])
        .map(|out| out.stdout.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0)
}

/// Soft-reset the last commit, keeping its changes staged (Zed's "Uncommit").
pub fn uncommit(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["reset", "--soft", "HEAD~1"])
}

/// Create a Solo agent's lane: a new branch at `base`, checked out into its own
/// working directory at `lane_path`. Goes through the CLI (not git2) so the
/// worktree bookkeeping matches what the user's own `git worktree` sees.
pub fn worktree_add(
    repo_path: &Path,
    lane_path: &Path,
    branch: &str,
    base: &str,
) -> Result<RemoteOutput> {
    let lane = lane_path.to_string_lossy().into_owned();
    run_git(repo_path, &["worktree", "add", "-b", branch, &lane, base])
}

/// Recreate a lane for an existing Solo branch (after a teardown): checks the
/// branch out into `lane_path` without creating anything new.
pub fn worktree_add_existing(
    repo_path: &Path,
    lane_path: &Path,
    branch: &str,
) -> Result<RemoteOutput> {
    let lane = lane_path.to_string_lossy().into_owned();
    run_git(repo_path, &["worktree", "add", &lane, branch])
}

/// Remove a lane's working directory. Without `force` git refuses when the
/// lane has uncommitted changes — that refusal is the teardown guard's
/// backstop, so only pass `force` after the user explicitly confirmed.
pub fn worktree_remove(repo_path: &Path, lane_path: &Path, force: bool) -> Result<RemoteOutput> {
    let lane = lane_path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
        // Git requires --force twice for a locked worktree. Choro only reaches
        // this path after an explicit destructive confirmation or after Rejoin
        // has verified the lane clean and merged its branch.
        args.push("--force");
    }
    args.push(&lane);
    run_git(repo_path, &args)
}

/// Drop stale worktree bookkeeping (e.g. a lane directory deleted from disk).
/// Run before re-adding a lane at a previously used path.
pub fn worktree_prune(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["worktree", "prune"])
}

/// Stage everything (tracked + untracked) — the pre-Rejoin sweep of a lane.
pub fn stage_all(repo_path: &Path) -> Result<RemoteOutput> {
    // The local exclude normally keeps generated dependency trees out already.
    // The negative pathspec is the final backstop if a project-level negation
    // overrides that exclude: Rejoin must never sweep tool-owned output into a
    // commit of its own.
    let exclusions = super::GENERATED_TOOL_DIRECTORIES
        .iter()
        .flat_map(|directory| {
            [
                format!(":(exclude){directory}/**"),
                format!(":(exclude)**/{directory}/**"),
            ]
        })
        .collect::<Vec<_>>();
    let mut args = vec!["add", "-A", "--", "."];
    args.extend(exclusions.iter().map(String::as_str));
    run_git(repo_path, &args)
}

/// Merge `branch` into the current branch (Solo "Rejoin"). `--no-ff` keeps the
/// Solo's work visible as its own merge; `--no-edit` keeps it non-interactive.
pub fn merge(repo_path: &Path, branch: &str) -> Result<RemoteOutput> {
    run_git(repo_path, &["merge", "--no-ff", "--no-edit", branch])
}

/// Abort an in-progress merge, restoring the pre-merge state. Used when a
/// Rejoin hits conflicts — the main tree returns to clean, the lane untouched.
pub fn merge_abort(repo_path: &Path) -> Result<RemoteOutput> {
    run_git(repo_path, &["merge", "--abort"])
}

/// Delete a local branch. `force` uses `-D` (drops unmerged work) — only after
/// explicit user confirmation.
pub fn delete_branch(repo_path: &Path, branch: &str, force: bool) -> Result<RemoteOutput> {
    let flag = if force { "-D" } else { "-d" };
    run_git(repo_path, &["branch", flag, branch])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;

    #[test]
    fn fetch_from_broken_remote_fails_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        repo.remote("origin", "/nonexistent/remote/path").unwrap();
        let result = fetch(dir.path()).unwrap();
        assert!(!result.success);
        assert!(!result.message().is_empty());
    }

    #[test]
    fn push_and_pull_against_local_bare_remote() {
        let remote_dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(remote_dir.path()).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        repo.remote("origin", remote_dir.path().to_str().unwrap())
            .unwrap();
        let branch = crate::git::read_head(dir.path()).unwrap().branch.unwrap();

        let pushed = push(dir.path(), Some(&branch), true).unwrap();
        assert!(pushed.success, "push failed: {}", pushed.message());

        let pulled = pull(dir.path()).unwrap();
        assert!(pulled.success, "pull failed: {}", pulled.message());
    }

    /// `git merge` (CLI) needs an identity to create the merge commit; set it
    /// locally so tests don't depend on the machine's global git config.
    fn configure_identity(repo: &git2::Repository) {
        let mut config = repo.config().unwrap();
        config.set_str("user.name", "Test").unwrap();
        config.set_str("user.email", "test@example.com").unwrap();
    }

    #[test]
    fn worktree_add_creates_branch_and_lane() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let lanes = tempfile::tempdir().unwrap();
        let lane = lanes.path().join("lane");

        let added = worktree_add(dir.path(), &lane, "solo/test", &base).unwrap();
        assert!(added.success, "worktree add failed: {}", added.message());
        assert!(lane.join("README.md").exists());
        let lane_head = crate::git::read_head(&lane).unwrap();
        assert_eq!(lane_head.branch.as_deref(), Some("solo/test"));
        // Main tree stays on its own branch, untouched.
        let main_head = crate::git::read_head(dir.path()).unwrap();
        assert_eq!(main_head.branch.as_deref(), Some(base.as_str()));
        drop(repo);
    }

    #[test]
    fn worktree_remove_refuses_dirty_lane_without_force() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let lanes = tempfile::tempdir().unwrap();
        let lane = lanes.path().join("lane");
        assert!(
            worktree_add(dir.path(), &lane, "solo/dirty", &base)
                .unwrap()
                .success
        );

        std::fs::write(lane.join("uncommitted.txt"), "work in progress\n").unwrap();
        let refused = worktree_remove(dir.path(), &lane, false).unwrap();
        assert!(!refused.success, "dirty lane should refuse removal");
        assert!(lane.exists());

        let forced = worktree_remove(dir.path(), &lane, true).unwrap();
        assert!(
            forced.success,
            "forced removal failed: {}",
            forced.message()
        );
        assert!(!lane.exists());
    }

    #[test]
    fn prune_and_readd_recreates_lane_at_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let lanes = tempfile::tempdir().unwrap();
        let lane = lanes.path().join("lane");
        assert!(
            worktree_add(dir.path(), &lane, "solo/revive", &base)
                .unwrap()
                .success
        );

        // Simulate a teardown that lost the bookkeeping: delete the dir raw.
        std::fs::remove_dir_all(&lane).unwrap();
        assert!(worktree_prune(dir.path()).unwrap().success);

        let readded = worktree_add_existing(dir.path(), &lane, "solo/revive").unwrap();
        assert!(readded.success, "re-add failed: {}", readded.message());
        assert!(lane.join("README.md").exists());
        assert_eq!(
            crate::git::read_head(&lane).unwrap().branch.as_deref(),
            Some("solo/revive")
        );
    }

    #[test]
    fn merge_brings_solo_work_home_and_branch_can_be_deleted() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let lanes = tempfile::tempdir().unwrap();
        let lane = lanes.path().join("lane");
        assert!(
            worktree_add(dir.path(), &lane, "solo/feature", &base)
                .unwrap()
                .success
        );

        std::fs::write(lane.join("feature.txt"), "solo work\n").unwrap();
        let lane_repo = git2::Repository::open(&lane).unwrap();
        commit_all(&lane_repo, "solo: add feature");

        assert_eq!(
            crate::git::ahead_behind(dir.path(), "solo/feature", &base).unwrap(),
            (1, 0)
        );

        let merged = merge(dir.path(), "solo/feature").unwrap();
        assert!(merged.success, "merge failed: {}", merged.message());
        assert!(dir.path().join("feature.txt").exists());

        // Lane must be gone before its branch can be deleted.
        assert!(worktree_remove(dir.path(), &lane, false).unwrap().success);
        let deleted = delete_branch(dir.path(), "solo/feature", false).unwrap();
        assert!(deleted.success, "delete failed: {}", deleted.message());
    }

    #[test]
    fn stage_all_never_stages_untracked_tool_output() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        std::fs::write(dir.path().join("source.ts"), "source\n").unwrap();
        let root_dependency = dir.path().join("node_modules/pkg/index.js");
        let nested_environment = dir.path().join("api/.venv/bin/python");
        let terraform_provider = dir.path().join("infra/.terraform/providers/plugin");
        std::fs::create_dir_all(root_dependency.parent().unwrap()).unwrap();
        std::fs::create_dir_all(nested_environment.parent().unwrap()).unwrap();
        std::fs::create_dir_all(terraform_provider.parent().unwrap()).unwrap();
        std::fs::write(root_dependency, "generated\n").unwrap();
        std::fs::write(nested_environment, "generated\n").unwrap();
        std::fs::write(terraform_provider, "generated\n").unwrap();

        let staged = stage_all(dir.path()).unwrap();
        assert!(staged.success, "stage-all failed: {}", staged.message());

        let mut index = repo.index().unwrap();
        index.read(true).unwrap();
        assert!(index.get_path(Path::new("source.ts"), 0).is_some());
        assert!(index
            .get_path(Path::new("node_modules/pkg/index.js"), 0)
            .is_none());
        assert!(index
            .get_path(Path::new("api/.venv/bin/python"), 0)
            .is_none());
        assert!(index
            .get_path(Path::new("infra/.terraform/providers/plugin"), 0)
            .is_none());
    }

    #[test]
    fn conflicted_merge_aborts_back_to_clean() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        configure_identity(&repo);
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let lanes = tempfile::tempdir().unwrap();
        let lane = lanes.path().join("lane");
        assert!(
            worktree_add(dir.path(), &lane, "solo/clash", &base)
                .unwrap()
                .success
        );

        // Both sides rewrite the same line.
        std::fs::write(lane.join("README.md"), "solo version\n").unwrap();
        let lane_repo = git2::Repository::open(&lane).unwrap();
        commit_all(&lane_repo, "solo: rewrite readme");
        std::fs::write(dir.path().join("README.md"), "main version\n").unwrap();
        commit_all(&repo, "main: rewrite readme");

        let merged = merge(dir.path(), "solo/clash").unwrap();
        assert!(!merged.success, "conflicting merge should fail");
        assert!(!merged.message().is_empty());

        let aborted = merge_abort(dir.path()).unwrap();
        assert!(aborted.success, "abort failed: {}", aborted.message());
        let snapshot = crate::git::read_snapshot(dir.path()).unwrap();
        assert!(
            snapshot.entries.is_empty(),
            "main tree should be clean after abort"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
            "main version\n"
        );
    }

    #[test]
    fn ahead_behind_counts_both_directions() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let base = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let lanes = tempfile::tempdir().unwrap();
        let lane = lanes.path().join("lane");
        assert!(
            worktree_add(dir.path(), &lane, "solo/count", &base)
                .unwrap()
                .success
        );

        let lane_repo = git2::Repository::open(&lane).unwrap();
        std::fs::write(lane.join("one.txt"), "1\n").unwrap();
        commit_all(&lane_repo, "solo: one");
        std::fs::write(lane.join("two.txt"), "2\n").unwrap();
        commit_all(&lane_repo, "solo: two");
        std::fs::write(dir.path().join("main.txt"), "m\n").unwrap();
        commit_all(&repo, "main: advance");

        assert_eq!(
            crate::git::ahead_behind(dir.path(), "solo/count", &base).unwrap(),
            (2, 1)
        );
    }

    #[cfg(unix)]
    #[test]
    fn git_operation_times_out_and_stops_its_hook() {
        use std::os::unix::fs::PermissionsExt as _;

        let remote_dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(remote_dir.path()).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        repo.remote("origin", remote_dir.path().to_str().unwrap())
            .unwrap();
        let hook = dir.path().join(".git/hooks/pre-push");
        std::fs::write(&hook, "#!/bin/sh\nsleep 5\n").unwrap();
        let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&hook, permissions).unwrap();

        let branch = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let error = run_git_with_timeout(
            dir.path(),
            &["push", "-u", "origin", &branch],
            Duration::from_millis(100),
        )
        .unwrap_err();

        assert!(error.to_string().contains("timed out"));
    }
}
