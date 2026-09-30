use std::path::Path;

use anyhow::{Context, Result};
use git2::{Repository, Sort};

use super::diff::{diffs_from, FileDiff};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub sha: String,
    pub sha_short: String,
    pub summary: String,
    pub author: String,
    /// Unix seconds.
    pub time: i64,
}

/// Most recent commits on HEAD, newest first.
pub fn list_commits(repo_path: &Path, limit: usize) -> Result<Vec<CommitInfo>> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    let mut walk = repo.revwalk()?;
    walk.set_sorting(Sort::TIME)?;
    if walk.push_head().is_err() {
        return Ok(Vec::new()); // unborn HEAD
    }

    let mut commits = Vec::with_capacity(limit);
    for oid in walk.take(limit) {
        let oid = oid?;
        let commit = repo.find_commit(oid)?;
        let sha = oid.to_string();
        commits.push(CommitInfo {
            sha_short: sha[..7].to_string(),
            sha,
            summary: commit
                .summary()
                .ok()
                .flatten()
                .unwrap_or_default()
                .to_string(),
            author: commit.author().name().ok().unwrap_or_default().to_string(),
            time: commit.time().seconds(),
        });
    }
    Ok(commits)
}

/// The full diff a commit introduced (vs its first parent).
pub fn commit_diff(repo_path: &Path, sha: &str) -> Result<Vec<FileDiff>> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    let commit = repo
        .find_commit(git2::Oid::from_str(sha)?)
        .context("commit not found")?;
    let tree = commit.tree()?;
    let parent_tree = commit.parent(0).ok().and_then(|p| p.tree().ok());

    let mut options = git2::DiffOptions::new();
    options.context_lines(3);
    let diff = repo.diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))?;
    diffs_from(&diff)
}

/// Commits `(ahead, behind)` of `local` relative to `base` — powers the
/// "On the side" strip's "M commits ahead" for Solo branches.
pub fn ahead_behind(repo_path: &Path, local: &str, base: &str) -> Result<(usize, usize)> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    let local_oid = repo
        .revparse_single(local)
        .with_context(|| format!("branch not found: {local}"))?
        .id();
    let base_oid = repo
        .revparse_single(base)
        .with_context(|| format!("branch not found: {base}"))?
        .id();
    Ok(repo.graph_ahead_behind(local_oid, base_oid)?)
}

/// All uncommitted changes (staged + unstaged + untracked) vs HEAD —
/// the "project diff" view.
pub fn worktree_diffs(repo_path: &Path) -> Result<Vec<FileDiff>> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    super::worktree_diff::patches(&repo, super::worktree_diff::Base::Head, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;
    use std::fs;

    #[test]
    fn lists_commits_newest_first() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        fs::write(workdir(&repo).join("two.txt"), "2\n").unwrap();
        commit_all(&repo, "second commit");

        let commits = list_commits(dir.path(), 10).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].summary, "second commit");
        assert_eq!(commits[0].sha_short.len(), 7);
        assert!(!commits[0].author.is_empty());
    }

    #[test]
    fn commit_diff_shows_introduced_changes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        fs::write(workdir(&repo).join("two.txt"), "hello two\n").unwrap();
        commit_all(&repo, "add two");

        let commits = list_commits(dir.path(), 1).unwrap();
        let diffs = commit_diff(dir.path(), &commits[0].sha).unwrap();
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].path.to_str(), Some("two.txt"));
    }

    #[test]
    fn worktree_diffs_cover_staged_and_untracked() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("README.md"), "edited\n").unwrap();
        fs::write(root.join("new.txt"), "new\n").unwrap();

        let diffs = worktree_diffs(dir.path()).unwrap();
        let paths: Vec<_> = diffs.iter().filter_map(|d| d.path.to_str()).collect();
        assert!(paths.contains(&"README.md"));
        assert!(paths.contains(&"new.txt"));
    }

    #[test]
    fn worktree_diffs_hide_untracked_codex_visualizations() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        let visualization = root.join(".codex/visualizations/chart.html");
        fs::create_dir_all(visualization.parent().unwrap()).unwrap();
        fs::write(&visualization, "<p>generated</p>\n").unwrap();
        fs::write(root.join("new.txt"), "visible\n").unwrap();

        let diffs = worktree_diffs(dir.path()).unwrap();
        let paths: Vec<_> = diffs.iter().filter_map(|diff| diff.path.to_str()).collect();
        assert_eq!(paths, vec!["new.txt"]);
    }
}
