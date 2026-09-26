use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use git2::{Delta, Diff, DiffDelta, DiffOptions, Patch, Repository};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineOrigin {
    Add,
    Remove,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    pub origin: LineOrigin,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileDiff {
    pub path: PathBuf,
    pub hunks: Vec<DiffHunk>,
    pub is_binary: bool,
}

/// Parse provider-supplied Git patches using the same path and hunk semantics
/// as repository diffs (including deletions, renames, and quoted paths).
pub fn parse_unified_diff(patch: &str) -> Result<Vec<FileDiff>> {
    if patch.trim().is_empty() {
        return Ok(Vec::new());
    }
    diffs_from(&Diff::from_buffer(patch.as_bytes())?)
}

/// Diff an attributed edit's captured contents without consulting the shared
/// worktree, where another writer may have changed the same file.
pub fn diff_from_contents(path: &Path, before: &str, after: &str) -> Result<FileDiff> {
    let patch = Patch::from_buffers(
        before.as_bytes(),
        Some(path),
        after.as_bytes(),
        Some(path),
        None,
    )?;
    let mut result = FileDiff {
        path: path.to_path_buf(),
        is_binary: patch.delta().flags().is_binary(),
        ..Default::default()
    };
    extract_patch(&patch, &mut result)?;
    Ok(result)
}

pub(crate) fn is_internal_untracked_delta(delta: &DiffDelta<'_>) -> bool {
    delta.status() == Delta::Untracked
        && delta.new_file().path().is_some_and(|path| {
            super::is_internal_visualization_path(path) || super::is_generated_tool_path(path)
        })
}

/// Diff a single file: worktree-vs-index (unstaged) or index-vs-HEAD (staged).
/// Untracked files are shown as an all-added diff against /dev/null.
pub fn diff_file(repo_path: &Path, file: &Path, staged: bool) -> Result<FileDiff> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path).context("not a git repository")?;

    if !staged {
        return Ok(super::worktree_diff::patches(
            &repo,
            super::worktree_diff::Base::Index,
            Some(file),
        )?
        .into_iter()
        .find(|diff| diff.path == file)
        .unwrap_or_else(|| FileDiff {
            path: file.to_path_buf(),
            ..Default::default()
        }));
    }

    let mut options = DiffOptions::new();
    options
        .pathspec(file)
        .context_lines(3)
        .disable_pathspec_match(true);

    let head_tree = repo.head().ok().and_then(|h| h.peel_to_tree().ok());
    let diff = repo.diff_tree_to_index(head_tree.as_ref(), None, Some(&mut options))?;

    build_file_diff(&diff, file)
}

fn build_file_diff(diff: &Diff, file: &Path) -> Result<FileDiff> {
    let mut result = FileDiff {
        path: file.to_path_buf(),
        ..Default::default()
    };
    for delta_index in 0..diff.deltas().len() {
        extract_delta(diff, delta_index, &mut result)?;
    }
    Ok(result)
}

/// One `FileDiff` per changed file in the diff — used by the project diff
/// and commit views.
pub fn diffs_from(diff: &Diff) -> Result<Vec<FileDiff>> {
    let mut results = Vec::new();
    for delta_index in 0..diff.deltas().len() {
        let Some(delta) = diff.get_delta(delta_index) else {
            continue;
        };
        if is_internal_untracked_delta(&delta) {
            continue;
        }
        let path = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let mut file_diff = FileDiff {
            path,
            ..Default::default()
        };
        extract_delta(diff, delta_index, &mut file_diff)?;
        results.push(file_diff);
    }
    Ok(results)
}

fn extract_delta(diff: &Diff, delta_index: usize, result: &mut FileDiff) -> Result<()> {
    let Some(patch) = Patch::from_diff(diff, delta_index)? else {
        // No textual patch (e.g. binary file).
        if let Some(delta) = diff.get_delta(delta_index) {
            if delta.flags().is_binary() {
                result.is_binary = true;
            }
        }
        return Ok(());
    };

    result.is_binary |= patch.delta().flags().is_binary();
    extract_patch(&patch, result)
}

pub(super) fn extract_patch(patch: &Patch<'_>, result: &mut FileDiff) -> Result<()> {
    for hunk_index in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(hunk_index)?;
        let header = String::from_utf8_lossy(hunk.header())
            .trim_end()
            .to_string();
        let mut lines = Vec::with_capacity(line_count);
        for line_index in 0..line_count {
            let line = patch.line_in_hunk(hunk_index, line_index)?;
            let origin = match line.origin() {
                '+' => LineOrigin::Add,
                '-' => LineOrigin::Remove,
                ' ' => LineOrigin::Context,
                _ => continue, // file/hunk markers, EOF-newline notes
            };
            lines.push(DiffLine {
                origin,
                old_no: line.old_lineno(),
                new_no: line.new_lineno(),
                text: String::from_utf8_lossy(line.content())
                    .trim_end_matches('\n')
                    .to_string(),
            });
        }
        result.hunks.push(DiffHunk { header, lines });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;
    use std::fs;

    fn patch_survives_worktree_truncation(tracked: bool) {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let file = if tracked { "README.md" } else { "new.txt" };
        let path = workdir(&repo).join(file);
        // Cross several OS pages so a file-backed mapping cannot hide the bug
        // in the final partial page. Patch line content outlives this file write.
        let content = "snapshot line\n".repeat(8192);
        fs::write(&path, &content).unwrap();
        let diff = diff_file(dir.path(), Path::new(file), false).unwrap();

        // Another editor/agent can truncate the file immediately after capture.
        // Choro's result must own its bytes independently of that file.
        fs::write(&path, []).unwrap();

        let added = diff
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.origin == LineOrigin::Add)
            .map(|line| format!("{}\n", line.text))
            .collect::<String>();
        assert_eq!(added, content);
    }

    #[test]
    fn tracked_patch_survives_worktree_truncation() {
        patch_survives_worktree_truncation(true);
    }

    #[test]
    fn untracked_patch_survives_worktree_truncation() {
        patch_survives_worktree_truncation(false);
    }

    #[test]
    fn unstaged_diff_shows_added_and_removed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("README.md"), "goodbye\n").unwrap();

        let diff = diff_file(dir.path(), Path::new("README.md"), false).unwrap();
        assert_eq!(diff.hunks.len(), 1);
        let lines = &diff.hunks[0].lines;
        assert!(lines
            .iter()
            .any(|l| l.origin == LineOrigin::Remove && l.text == "hello"));
        assert!(lines
            .iter()
            .any(|l| l.origin == LineOrigin::Add && l.text == "goodbye"));
    }

    #[test]
    fn staged_diff_reflects_index() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("README.md"), "staged change\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();

        let staged = diff_file(dir.path(), Path::new("README.md"), true).unwrap();
        assert!(staged.hunks[0]
            .lines
            .iter()
            .any(|l| l.origin == LineOrigin::Add && l.text == "staged change"));

        let unstaged = diff_file(dir.path(), Path::new("README.md"), false).unwrap();
        assert!(unstaged.hunks.is_empty());
    }

    #[test]
    fn untracked_file_diff_is_all_added() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("new.txt"), "line1\nline2\n").unwrap();

        let diff = diff_file(dir.path(), Path::new("new.txt"), false).unwrap();
        let added: Vec<_> = diff
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.origin == LineOrigin::Add)
            .collect();
        assert_eq!(added.len(), 2);
    }
}
