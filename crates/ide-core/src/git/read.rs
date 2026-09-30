use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{BranchType, Diff, Patch, Repository, Status, StatusOptions};

use super::diff::is_internal_untracked_delta;
use super::is_internal_visualization_path;
use super::snapshot::{BranchInfo, ChangeKind, GitSnapshot, HeadInfo, LineStats, StatusEntry};

// Line totals are supporting metadata, never a reason to read an unbounded
// generated tree into memory. Tracked changes remain fully counted; only
// untracked text patches are capped.
const MAX_UNTRACKED_LINE_STAT_FILES: usize = 512;
const MAX_LINE_STAT_FILE_BYTES: i64 = 1024 * 1024;

/// Lightweight read used by the project list (branch label only).
pub fn read_head(repo_path: &Path) -> Result<HeadInfo> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    head_info(&repo)
}

/// Full read for the git panel: HEAD, branches, status.
pub fn read_snapshot(repo_path: &Path) -> Result<GitSnapshot> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    let (insertions, deletions) = worktree_line_stats(&repo);
    let (staged_stats, unstaged_stats) = status_line_stats(&repo);
    let primary_remote = super::accounts::primary_remote_of(&repo);
    let assigned_account = primary_remote
        .as_ref()
        .and_then(|remote| super::accounts::assigned_account_of(&repo, remote));
    Ok(GitSnapshot {
        head: head_info(&repo)?,
        branches: branches(&repo)?,
        entries: status_entries(&repo, &staged_stats, &unstaged_stats)?,
        insertions,
        deletions,
        primary_remote,
        assigned_account,
    })
}

/// +/- line counts of all uncommitted changes vs HEAD.
fn worktree_line_stats(repo: &Repository) -> (usize, usize) {
    super::worktree_diff::line_stats(repo, super::worktree_diff::Base::Head)
        .unwrap_or_default()
        .values()
        .fold((0, 0), |(added, removed), stats| {
            (added + stats.insertions, removed + stats.deletions)
        })
}

/// Per-file line counts for the index side and worktree side independently.
fn status_line_stats(
    repo: &Repository,
) -> (HashMap<PathBuf, LineStats>, HashMap<PathBuf, LineStats>) {
    let head_tree = repo.head().ok().and_then(|head| head.peel_to_tree().ok());

    let mut staged_options = git2::DiffOptions::new();
    staged_options.max_size(MAX_LINE_STAT_FILE_BYTES);
    let staged = repo
        .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut staged_options))
        .ok()
        .map(|mut diff| {
            diff.find_similar(None).ok();
            line_stats_by_path(&diff)
        })
        .unwrap_or_default();

    let unstaged = super::worktree_diff::line_stats(repo, super::worktree_diff::Base::Index)
        .unwrap_or_default();

    (staged, unstaged)
}

fn line_stats_by_path(diff: &Diff<'_>) -> HashMap<PathBuf, LineStats> {
    let mut stats = HashMap::new();
    let mut untracked_files = 0usize;
    for delta_index in 0..diff.deltas().len() {
        let Some(delta) = diff.get_delta(delta_index) else {
            continue;
        };
        if is_internal_untracked_delta(&delta) {
            continue;
        }
        if delta.status() == git2::Delta::Untracked {
            if untracked_files >= MAX_UNTRACKED_LINE_STAT_FILES {
                continue;
            }
            untracked_files += 1;
        }
        let Some(path) = delta.new_file().path().or_else(|| delta.old_file().path()) else {
            continue;
        };
        let Ok(Some(patch)) = Patch::from_diff(diff, delta_index) else {
            continue;
        };
        let Ok((_, insertions, deletions)) = patch.line_stats() else {
            continue;
        };
        let entry = stats
            .entry(path.to_path_buf())
            .or_insert(LineStats::default());
        entry.insertions += insertions;
        entry.deletions += deletions;
    }
    stats
}

fn head_info(repo: &Repository) -> Result<HeadInfo> {
    if repo.head().is_err() {
        // Unborn HEAD: fresh repo with no commits. Read the symbolic target for the name.
        let branch = repo
            .find_reference("HEAD")
            .ok()
            .and_then(|r| r.symbolic_target().ok().flatten().map(str::to_owned))
            .and_then(|t| t.strip_prefix("refs/heads/").map(str::to_owned));
        return Ok(HeadInfo {
            branch,
            oid_short: String::new(),
            detached: false,
            unborn: true,
        });
    }

    let head = repo.head()?;
    let detached = repo.head_detached().unwrap_or(false);
    let oid_short = head
        .peel_to_commit()
        .ok()
        .map(|c| c.id().to_string()[..7].to_string())
        .unwrap_or_default();
    let branch = if detached {
        None
    } else {
        head.shorthand().ok().map(str::to_owned)
    };
    Ok(HeadInfo {
        branch,
        oid_short,
        detached,
        unborn: false,
    })
}

fn branches(repo: &Repository) -> Result<Vec<BranchInfo>> {
    let mut branches = Vec::new();
    for entry in repo.branches(Some(BranchType::Local))? {
        let (branch, _) = entry?;
        let Some(name) = branch.name()?.map(str::to_owned) else {
            continue;
        };
        let is_head = branch.is_head();
        let mut upstream = None;
        let (mut ahead, mut behind) = (0, 0);
        if let Ok(up) = branch.upstream() {
            upstream = up.name()?.map(str::to_owned);
            if let (Some(local), Some(remote)) = (branch.get().target(), up.get().target()) {
                if let Ok((a, b)) = repo.graph_ahead_behind(local, remote) {
                    ahead = a;
                    behind = b;
                }
            }
        }
        let (tip_summary, tip_author, tip_time) = branch
            .get()
            .peel_to_commit()
            .map(|commit| {
                (
                    commit
                        .summary()
                        .ok()
                        .flatten()
                        .unwrap_or_default()
                        .to_string(),
                    commit.author().name().ok().unwrap_or_default().to_string(),
                    commit.time().seconds(),
                )
            })
            .unwrap_or_default();
        branches.push(BranchInfo {
            name,
            is_remote: false,
            is_head,
            upstream,
            ahead,
            behind,
            tip_summary,
            tip_author,
            tip_time,
        });
    }
    for entry in repo.branches(Some(BranchType::Remote))? {
        let (branch, _) = entry?;
        let Some(name) = branch.name()?.map(str::to_owned) else {
            continue;
        };
        let (tip_summary, tip_author, tip_time) = branch
            .get()
            .peel_to_commit()
            .map(|commit| {
                (
                    commit
                        .summary()
                        .ok()
                        .flatten()
                        .unwrap_or_default()
                        .to_string(),
                    commit.author().name().ok().unwrap_or_default().to_string(),
                    commit.time().seconds(),
                )
            })
            .unwrap_or_default();
        branches.push(BranchInfo {
            name,
            is_remote: true,
            is_head: false,
            upstream: None,
            ahead: 0,
            behind: 0,
            tip_summary,
            tip_author,
            tip_time,
        });
    }

    branches.sort_by(|a, b| {
        (!a.is_head, a.is_remote, &a.name).cmp(&(!b.is_head, b.is_remote, &b.name))
    });
    Ok(branches)
}

fn status_entries(
    repo: &Repository,
    staged_stats: &HashMap<PathBuf, LineStats>,
    unstaged_stats: &HashMap<PathBuf, LineStats>,
) -> Result<Vec<StatusEntry>> {
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .exclude_submodules(true);

    let statuses = repo.statuses(Some(&mut options))?;
    let mut entries = Vec::with_capacity(statuses.len());
    for status in statuses.iter() {
        let Ok(path) = status.path() else { continue };
        let flags = status.status();
        if flags == Status::CURRENT || flags.contains(Status::IGNORED) {
            continue;
        }
        if flags == Status::WT_NEW
            && (is_internal_visualization_path(Path::new(path))
                || super::is_generated_tool_path(Path::new(path)))
        {
            continue;
        }
        let entry = StatusEntry {
            path: path.into(),
            staged: staged_kind(flags),
            unstaged: unstaged_kind(flags),
            staged_stats: staged_stats
                .get(Path::new(path))
                .copied()
                .unwrap_or_default(),
            unstaged_stats: unstaged_stats
                .get(Path::new(path))
                .copied()
                .unwrap_or_default(),
        };
        if entry.staged.is_some() || entry.unstaged.is_some() {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

fn staged_kind(flags: Status) -> Option<ChangeKind> {
    if flags.contains(Status::CONFLICTED) {
        return Some(ChangeKind::Conflicted);
    }
    if flags.contains(Status::INDEX_NEW) {
        Some(ChangeKind::Added)
    } else if flags.contains(Status::INDEX_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if flags.contains(Status::INDEX_DELETED) {
        Some(ChangeKind::Deleted)
    } else if flags.contains(Status::INDEX_MODIFIED) || flags.contains(Status::INDEX_TYPECHANGE) {
        Some(ChangeKind::Modified)
    } else {
        None
    }
}

fn unstaged_kind(flags: Status) -> Option<ChangeKind> {
    if flags.contains(Status::WT_NEW) {
        Some(ChangeKind::Untracked)
    } else if flags.contains(Status::WT_RENAMED) {
        Some(ChangeKind::Renamed)
    } else if flags.contains(Status::WT_DELETED) {
        Some(ChangeKind::Deleted)
    } else if flags.contains(Status::WT_MODIFIED) || flags.contains(Status::WT_TYPECHANGE) {
        Some(ChangeKind::Modified)
    } else {
        None
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    use std::fs;
    use std::path::{Path, PathBuf};

    use git2::{Repository, Signature};

    /// Create a repo with one commit containing README.md.
    pub fn repo_with_commit(dir: &Path) -> Repository {
        let repo = Repository::init(dir).unwrap();
        fs::write(dir.join("README.md"), "hello\n").unwrap();
        commit_all(&repo, "initial commit");
        repo
    }

    pub fn commit_all(repo: &Repository, message: &str) -> git2::Oid {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = Signature::now("Test", "test@example.com").unwrap();
        let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
        let parents: Vec<_> = parent.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, message, &tree, &parents)
            .unwrap()
    }

    pub fn workdir(repo: &Repository) -> PathBuf {
        repo.workdir().unwrap().to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use std::{fs, path::PathBuf};

    #[test]
    fn head_of_fresh_repo_is_unborn() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let head = read_head(dir.path()).unwrap();
        assert!(head.unborn);
        assert!(head.branch.is_some());
    }

    #[test]
    fn head_shows_current_branch() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_commit(dir.path());
        let head = read_head(dir.path()).unwrap();
        assert!(!head.unborn);
        assert!(!head.detached);
        assert!(head.branch.is_some());
        assert_eq!(head.oid_short.len(), 7);
    }

    #[test]
    fn non_repo_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_head(dir.path()).is_err());
    }

    #[test]
    fn snapshot_reports_status_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);

        fs::write(root.join("README.md"), "changed\n").unwrap();
        fs::write(root.join("new.txt"), "new\n").unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let readme = snapshot
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("README.md"))
            .unwrap();
        assert_eq!(readme.unstaged, Some(ChangeKind::Modified));
        assert_eq!(readme.staged, None);
        assert_eq!(
            readme.unstaged_stats,
            LineStats {
                insertions: 1,
                deletions: 1,
            }
        );

        let new = snapshot
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("new.txt"))
            .unwrap();
        assert_eq!(new.unstaged, Some(ChangeKind::Untracked));
        assert_eq!(
            new.unstaged_stats,
            LineStats {
                insertions: 1,
                deletions: 0,
            }
        );
        assert_eq!(snapshot.untracked().count(), 1);
    }

    #[test]
    fn snapshot_keeps_staged_and_unstaged_line_stats_separate() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);

        fs::write(root.join("README.md"), "staged\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();
        fs::write(root.join("README.md"), "staged\nworking\n").unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let readme = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("README.md"))
            .unwrap();
        assert_eq!(
            readme.staged_stats,
            LineStats {
                insertions: 1,
                deletions: 1,
            }
        );
        assert_eq!(
            readme.unstaged_stats,
            LineStats {
                insertions: 1,
                deletions: 0,
            }
        );
    }

    #[test]
    fn snapshot_hides_untracked_codex_visualizations() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        let visualization = root.join(".codex/visualizations/2026/chart.html");
        fs::create_dir_all(visualization.parent().unwrap()).unwrap();
        fs::write(&visualization, "<p>generated</p>\n").unwrap();
        fs::write(root.join("new.txt"), "visible\n").unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let paths: Vec<_> = snapshot.entries.iter().map(|entry| &entry.path).collect();
        assert!(!paths
            .iter()
            .any(|path| path == &&PathBuf::from(".codex/visualizations/2026/chart.html")));
        assert!(paths.iter().any(|path| path == &&PathBuf::from("new.txt")));
        assert_eq!((snapshot.insertions, snapshot.deletions), (1, 0));
    }

    #[test]
    fn snapshot_hides_untracked_node_dependencies_without_a_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        let dependency = root.join("node_modules/pkg/index.js");
        fs::create_dir_all(dependency.parent().unwrap()).unwrap();
        fs::write(&dependency, "generated\n").unwrap();
        fs::write(root.join("source.ts"), "visible\n").unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let paths: Vec<_> = snapshot.entries.iter().map(|entry| &entry.path).collect();
        assert!(!paths.iter().any(|path| path.starts_with("node_modules")));
        assert!(paths
            .iter()
            .any(|path| path == &&PathBuf::from("source.ts")));
        assert_eq!((snapshot.insertions, snapshot.deletions), (1, 0));
    }

    #[test]
    fn snapshot_keeps_intentionally_tracked_node_dependencies_visible() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        let dependency = root.join("node_modules/pkg/index.js");
        fs::create_dir_all(dependency.parent().unwrap()).unwrap();
        fs::write(&dependency, "before\n").unwrap();
        commit_all(&repo, "track dependency intentionally");
        fs::write(&dependency, "after\n").unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == Path::new("node_modules/pkg/index.js"))
            .unwrap();
        assert_eq!(entry.unstaged, Some(ChangeKind::Modified));
    }

    #[test]
    fn snapshot_keeps_tracked_codex_visualization_changes_visible() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        let visualization = root.join(".codex/visualizations/chart.html");
        fs::create_dir_all(visualization.parent().unwrap()).unwrap();
        fs::write(&visualization, "before\n").unwrap();
        commit_all(&repo, "track visualization intentionally");
        fs::write(&visualization, "after\n").unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let entry = snapshot
            .entries
            .iter()
            .find(|entry| entry.path == Path::new(".codex/visualizations/chart.html"))
            .unwrap();
        assert_eq!(entry.unstaged, Some(ChangeKind::Modified));
    }

    #[test]
    fn snapshot_lists_branches_with_head_first() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let head_commit = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("feature/x", &head_commit, false).unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        assert_eq!(snapshot.branches.len(), 2);
        assert!(snapshot.branches[0].is_head);
    }
}
