//! Working files can be truncated while a diff is being calculated. Keep those
//! reads out of libgit2's in-process mmap path: Git returns an owned patch, and
//! untracked line counts use owned Rust buffers. Object-only diffs still use git2.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use git2::{
    AttrCheckFlags, AttrValue, Diff, DiffOptions, Patch, Repository, Status, StatusOptions,
};

use super::diff::{diffs_from, FileDiff};
use super::snapshot::LineStats;

const MAX_UNTRACKED_STAT_FILES: usize = 512;
const MAX_STAT_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Base {
    Head,
    Index,
}

fn command(repo: &Repository) -> Result<Command> {
    let mut command = Command::new("git");
    command
        .current_dir(repo.workdir().context("repository has no working tree")?)
        .args(["--no-pager", "--literal-pathspecs"])
        .env("GIT_OPTIONAL_LOCKS", "0");
    Ok(command)
}

fn output(command: &mut Command, no_index: bool) -> Result<Vec<u8>> {
    let output = crate::process::output_with_timeout(command, Duration::from_secs(30))?;
    // --no-index returns 1 for differences, but some Git versions also use 1
    // when a file disappeared. A successful comparison must produce a patch.
    let has_diff = no_index && output.status.code() == Some(1) && !output.stdout.is_empty();
    if !output.status.success() && !has_diff {
        bail!(
            "Git diff failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn tracked_output(
    repo: &Repository,
    base: Base,
    file: Option<&Path>,
    stats: bool,
) -> Result<Vec<u8>> {
    let mut command = command(repo)?;
    if stats {
        command.args(["-c", "core.bigFileThreshold=1048576"]);
    }
    command.args([
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--no-renames",
        "--src-prefix=a/",
        "--dst-prefix=b/",
        "--submodule=short",
    ]);
    if stats {
        command.args(["--numstat", "-z"]);
    } else {
        command.args(["--patch", "--unified=3"]);
    }
    if matches!(base, Base::Head) {
        // Git recognizes the empty tree even when an unborn repository has not
        // written it to disk. Resolve HEAD once so a concurrent commit cannot
        // change the comparison halfway through this command.
        let tree = match repo.head() {
            Ok(head) => head.peel_to_tree()?.id().to_string(),
            Err(error) if error.code() == git2::ErrorCode::UnbornBranch => {
                "4b825dc642cb6eb9a060e54bf8d69288fbee4904".to_owned()
            }
            Err(error) => return Err(error.into()),
        };
        command.arg(tree);
    }
    command.arg("--");
    if let Some(file) = file {
        command.arg(file);
    }
    output(&mut command, false)
}

fn untracked_paths(repo: &Repository, file: Option<&Path>) -> Result<Vec<PathBuf>> {
    let mut options = StatusOptions::new();
    options.include_untracked(true).recurse_untracked_dirs(true);
    let statuses = repo.statuses(Some(&mut options))?;
    let mut paths = Vec::new();
    for entry in statuses.iter() {
        if !entry.status().contains(Status::WT_NEW) {
            continue;
        }
        let path = Path::new(entry.path()?);
        if file.is_some_and(|file| file != path)
            || super::is_internal_visualization_path(path)
            || super::is_generated_tool_path(path)
        {
            continue;
        }
        paths.push(path.to_path_buf());
    }
    Ok(paths)
}

pub(super) fn patches(repo: &Repository, base: Base, file: Option<&Path>) -> Result<Vec<FileDiff>> {
    let bytes = tracked_output(repo, base, file, false)?;
    let mut results = parse_patch(&bytes)?;
    for path in untracked_paths(repo, file)? {
        let absolute = repo
            .workdir()
            .context("repository has no working tree")?
            .join(&path);
        if fs::symlink_metadata(&absolute)?.is_dir() {
            // A nested repository is a single status entry. Workspace discovery
            // reads it separately; never recursively diff its .git directory.
            results.push(FileDiff {
                path,
                ..Default::default()
            });
            continue;
        }
        let mut command = command(repo)?;
        command
            .args([
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--no-textconv",
                "--no-color",
                "--src-prefix=a/",
                "--dst-prefix=b/",
                "--unified=3",
                "--",
                "/dev/null",
            ])
            .arg(&path);
        let bytes = output(&mut command, true)?;
        let mut diffs = parse_patch(&bytes)?;
        // An empty untracked file has no text, but still belongs in the review.
        if diffs.is_empty() {
            diffs.push(FileDiff {
                path: path.clone(),
                ..Default::default()
            });
        }
        for mut diff in diffs {
            diff.path = path.clone();
            results.push(diff);
        }
    }
    results.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(results)
}

fn parse_patch(bytes: &[u8]) -> Result<Vec<FileDiff>> {
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        if line.starts_with(b"diff --git ") && offset > start {
            results.extend(parse_file_patch(&bytes[start..offset])?);
            start = offset;
        }
        offset += line.len();
    }
    results.extend(parse_file_patch(&bytes[start..])?);
    Ok(results)
}

fn parse_file_patch(bytes: &[u8]) -> Result<Vec<FileDiff>> {
    let last_line = bytes
        .rsplit(|byte| *byte == b'\n')
        .find(|line| !line.is_empty())
        .unwrap_or_default();
    let empty_file = last_line.starts_with(b"index ")
        && bytes.split(|byte| *byte == b'\n').take(3).any(|line| {
            line.starts_with(b"new file mode ") || line.starts_with(b"deleted file mode ")
        });
    if empty_file {
        // Git omits text headers for empty added/deleted files. libgit2 accepts
        // these as mail patches with an explicit end marker, but rejects a bare
        // EOF after `index`. Parse each file separately so this valid separator
        // cannot hide a later file. Paths and modes still come from Git's bytes.
        let mut terminated = bytes.to_vec();
        terminated.extend_from_slice(b"-- \n");
        return diffs_from(&Diff::from_buffer(&terminated)?);
    }
    diffs_from(&Diff::from_buffer(bytes)?)
}

pub(super) fn line_stats(repo: &Repository, base: Base) -> Result<HashMap<PathBuf, LineStats>> {
    let bytes = tracked_output(repo, base, None, true)?;
    let mut stats = HashMap::new();
    for record in bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let mut fields = record.splitn(3, |byte| *byte == b'\t');
        let added = fields.next().context("missing Git insertion count")?;
        let removed = fields.next().context("missing Git deletion count")?;
        let path = fields.next().context("missing Git stat path")?;
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            PathBuf::from(std::ffi::OsStr::from_bytes(path))
        };
        #[cfg(not(unix))]
        let path = PathBuf::from(std::str::from_utf8(path)?);
        let count = |bytes: &[u8]| -> Result<usize> {
            if bytes == b"-" {
                Ok(0)
            } else {
                Ok(std::str::from_utf8(bytes)?.parse()?)
            }
        };
        stats.insert(
            path,
            LineStats {
                insertions: count(added)?,
                deletions: count(removed)?,
            },
        );
    }
    let root = repo.workdir().context("repository has no working tree")?;
    for path in untracked_paths(repo, None)?
        .into_iter()
        .take(MAX_UNTRACKED_STAT_FILES)
    {
        // A writer may remove/replace one path after status was read. Keep the
        // remaining totals instead of discarding the entire repository's stats.
        if let Ok(counts) = untracked_line_stats(repo, root, &path) {
            stats.insert(path, counts);
        }
    }
    Ok(stats)
}

fn untracked_line_stats(repo: &Repository, root: &Path, path: &Path) -> Result<LineStats> {
    let absolute = root.join(path);
    let metadata = fs::symlink_metadata(&absolute)?;
    let bytes = if metadata.file_type().is_symlink() {
        fs::read_link(&absolute)?
            .as_os_str()
            .as_encoded_bytes()
            .to_vec()
    } else if metadata.is_file() && metadata.len() <= MAX_STAT_BYTES {
        let mut bytes = Vec::new();
        fs::File::open(&absolute)?
            .take(MAX_STAT_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STAT_BYTES {
            return Ok(LineStats::default());
        }
        bytes
    } else {
        return Ok(LineStats::default());
    };
    let mut options = DiffOptions::new();
    let attr = repo.get_attr(path, "diff", AttrCheckFlags::FILE_THEN_INDEX)?;
    match AttrValue::from_string(attr) {
        AttrValue::False => {
            options.force_binary(true);
        }
        AttrValue::True => {
            options.force_text(true);
        }
        AttrValue::String(driver) => {
            if let Ok(binary) = repo.config()?.get_bool(&format!("diff.{driver}.binary")) {
                options.force_binary(binary).force_text(!binary);
            }
        }
        _ => {}
    }
    let patch = Patch::from_buffers(&[], Some(path), &bytes, Some(path), Some(&mut options))?;
    let (_, insertions, deletions) = patch.line_stats()?;
    Ok(LineStats {
        insertions,
        deletions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;
    use crate::git::LineOrigin;

    #[test]
    fn captured_git_patch_survives_source_truncation() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let path = workdir(&repo).join("README.md");
        let contents = "snapshot line\n".repeat(8192);
        fs::write(&path, &contents).unwrap();
        let bytes = tracked_output(&repo, Base::Head, None, false).unwrap();
        fs::write(&path, []).unwrap();
        let diff = parse_patch(&bytes).unwrap().remove(0);
        assert_eq!(
            diff.hunks
                .iter()
                .flat_map(|h| &h.lines)
                .filter(|line| line.origin == LineOrigin::Add)
                .count(),
            8192
        );
    }

    #[test]
    fn diffs_and_counts_survive_concurrent_file_truncation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let path = workdir(&repo).join("README.md");
        let running = Arc::new(AtomicBool::new(true));
        let writing = running.clone();
        let writer = std::thread::spawn(move || {
            let bytes = "changing line\n".repeat(8192);
            while writing.load(Ordering::Relaxed) {
                fs::write(&path, &bytes).unwrap();
                fs::write(&path, []).unwrap();
                std::thread::sleep(Duration::from_millis(1));
            }
        });
        for _ in 0..8 {
            // A concurrent read may fail normally; it must never crash Choro.
            let _ = patches(&repo, Base::Head, None);
            let _ = line_stats(&repo, Base::Index);
        }
        running.store(false, Ordering::Relaxed);
        writer.join().unwrap();
        assert!(patches(&repo, Base::Head, None).is_ok());
    }

    #[test]
    fn git_process_failure_is_reported_as_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let mut command = command(&repo).unwrap();
        command.args(["diff", "--no-index", "--", "/dev/null", "missing.txt"]);
        assert!(output(&mut command, true)
            .unwrap_err()
            .to_string()
            .contains("Git diff failed"));
    }

    #[test]
    fn unborn_head_includes_staged_and_untracked_files() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path()).unwrap();
        fs::write(dir.path().join("staged.txt"), "staged\n").unwrap();
        fs::write(dir.path().join("new.txt"), "new\n").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();
        let diffs = patches(&repo, Base::Head, None).unwrap();
        assert_eq!(diffs.len(), 2);
        let stats = line_stats(&repo, Base::Head).unwrap();
        assert_eq!(stats.values().map(|stat| stat.insertions).sum::<usize>(), 2);
        assert_eq!(patches(&repo, Base::Index, None).unwrap().len(), 1);
    }

    #[test]
    fn literal_paths_binary_files_and_empty_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let path = Path::new("odd [name]\tline\n.txt");
        fs::write(workdir(&repo).join(path), "before\n").unwrap();
        fs::write(workdir(&repo).join("binary.dat"), [0, 1]).unwrap();
        commit_all(&repo, "add unusual paths");
        fs::write(workdir(&repo).join(path), "after\n").unwrap();
        fs::write(workdir(&repo).join("binary.dat"), [0, 2]).unwrap();
        fs::write(workdir(&repo).join("empty.txt"), "").unwrap();
        let diffs = patches(&repo, Base::Head, None).unwrap();
        assert!(diffs
            .iter()
            .any(|diff| diff.path == Path::new("binary.dat") && diff.is_binary));
        assert!(diffs
            .iter()
            .any(|diff| diff.path == Path::new("empty.txt") && diff.hunks.is_empty()));
        assert_eq!(patches(&repo, Base::Index, Some(path)).unwrap().len(), 1);
        let stats = line_stats(&repo, Base::Head).unwrap();
        assert_eq!(
            stats[path],
            LineStats {
                insertions: 1,
                deletions: 1
            }
        );
    }

    #[test]
    fn empty_added_file_does_not_hide_following_changes() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        fs::write(workdir(&repo).join("A-empty.txt"), "").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("A-empty.txt")).unwrap();
        index.write().unwrap();
        fs::write(workdir(&repo).join("README.md"), "after empty file\n").unwrap();
        let diffs = patches(&repo, Base::Head, None).unwrap();
        assert_eq!(diffs.len(), 2);
        assert_eq!(diffs[0].path, Path::new("A-empty.txt"));
        assert!(diffs[0].hunks.is_empty());
        assert!(diffs[1]
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|line| line.text == "after empty file"));
    }

    #[test]
    fn tracked_filters_and_untracked_binary_attributes_are_respected() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        fs::write(
            workdir(&repo).join(".gitattributes"),
            "*.txt text eol=crlf\n*.dat -diff\n",
        )
        .unwrap();
        fs::write(workdir(&repo).join("text.txt"), "before\r\n").unwrap();
        commit_all(&repo, "attributes");
        fs::write(workdir(&repo).join("text.txt"), "after\r\n").unwrap();
        fs::write(workdir(&repo).join("new.dat"), "binary by attribute\n").unwrap();
        let diffs = patches(&repo, Base::Head, None).unwrap();
        let text = diffs
            .iter()
            .find(|diff| diff.path == Path::new("text.txt"))
            .unwrap();
        assert!(text
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|line| line.origin == LineOrigin::Add && line.text == "after"));
        assert_eq!(
            line_stats(&repo, Base::Head).unwrap()[Path::new("new.dat")],
            LineStats::default()
        );
    }

    #[cfg(unix)]
    #[test]
    fn untracked_symlinks_diff_the_link_instead_of_its_target() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        std::os::unix::fs::symlink("README.md", workdir(&repo).join("link")).unwrap();
        let diff = patches(&repo, Base::Head, Some(Path::new("link")))
            .unwrap()
            .remove(0);
        assert_eq!(diff.hunks[0].lines[0].text, "README.md");
        assert_eq!(
            line_stats(&repo, Base::Head).unwrap()[Path::new("link")].insertions,
            1
        );
    }
}
