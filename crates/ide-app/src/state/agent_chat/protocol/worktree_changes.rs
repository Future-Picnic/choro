use std::collections::BTreeMap;
use std::io::Read;

use sha2::{Digest, Sha256};

use super::*;

/// Git observations supplement provider edit events: shell scripts, generators,
/// and delegated tools need not emit a `fileChange` event. Keep these separate
/// from exact attribution because another writer may share this working tree.
#[derive(Default)]
pub(super) struct WorktreeChanges {
    files: BTreeMap<PathBuf, ObservedFile>,
    heads: BTreeMap<PathBuf, Option<String>>,
    root: PathBuf,
}

#[derive(PartialEq, Eq)]
struct ObservedFile {
    diff: ide_core::git::FileDiff,
    fingerprint: Option<[u8; 32]>,
    content: Option<String>,
}

impl WorktreeChanges {
    pub(super) fn capture(agent: &AgentRecord) -> anyhow::Result<Self> {
        let root = agent.runtime_path();
        let diffs = if agent.repository_path.is_none() && !agent.is_active_solo() {
            ide_core::git::workspace_worktree_diffs(&agent.project_path)?
        } else {
            ide_core::git::worktree_diffs(root)?
        };
        Ok(Self::from_diffs(root, diffs))
    }

    fn from_diffs(root: &Path, diffs: Vec<ide_core::git::FileDiff>) -> Self {
        Self {
            files: diffs
                .into_iter()
                .map(|diff| {
                    let path = diff.path.clone();
                    let fingerprint = fingerprint(&root.join(&path));
                    let content = bounded_text(&root.join(&path));
                    (
                        path,
                        ObservedFile {
                            diff,
                            fingerprint,
                            content,
                        },
                    )
                })
                .collect(),
            heads: repository_heads(root),
            root: root.to_path_buf(),
        }
    }

    pub(super) fn changes_since(&self, before: &Self) -> Vec<FileChangeStat> {
        let mut changes = self
            .files
            .iter()
            .filter_map(|(path, state)| {
                if before.files.get(path) == Some(state) {
                    return None;
                }
                let mut additions = 0;
                let mut deletions = 0;
                for line in state.diff.hunks.iter().flat_map(|hunk| &hunk.lines) {
                    match line.origin {
                        ide_core::git::LineOrigin::Add => additions += 1,
                        ide_core::git::LineOrigin::Remove => deletions += 1,
                        ide_core::git::LineOrigin::Context => {}
                    }
                }
                Some(
                    FileChangeStat::new(path.clone(), additions, deletions)
                        .as_count_projection()
                        .with_content_hashes(
                            before
                                .files
                                .get(path)
                                .and_then(|file| hash_string(file.fingerprint)),
                            hash_string(state.fingerprint),
                        )
                        .with_content_projection(None, state.content.clone()),
                )
            })
            .collect::<Vec<_>>();
        for path in before
            .files
            .keys()
            .filter(|path| !self.files.contains_key(*path))
        {
            let repository = before
                .heads
                .keys()
                .filter(|root| path.starts_with(root))
                .max_by_key(|root| root.components().count());
            // A commit also makes Git's dirty entry disappear. It does not undo
            // the chat's work; only clear entries when their HEAD stayed fixed.
            if repository.is_some_and(|root| before.heads.get(root) == self.heads.get(root)) {
                let absolute = self.root.join(path);
                changes.push(
                    FileChangeStat::new(path.clone(), 0, 0)
                        .as_count_projection()
                        .with_cleared_projection(true)
                        .with_content_hashes(None, hash_string(fingerprint(&absolute)))
                        .with_content_projection(None, bounded_text(&absolute)),
                );
            }
        }
        changes.sort_by(|left, right| left.path.cmp(&right.path));
        changes
    }
}

fn repository_heads(root: &Path) -> BTreeMap<PathBuf, Option<String>> {
    let mut repositories = ide_core::git::discover_repositories(root);
    if repositories.is_empty() {
        repositories.push(root.to_path_buf());
    }
    repositories
        .into_iter()
        .map(|repository| {
            let head = Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(["rev-parse", "--verify", "HEAD"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string());
            (
                repository
                    .strip_prefix(root)
                    .unwrap_or(&repository)
                    .to_path_buf(),
                head,
            )
        })
        .collect()
}

fn hash_string(hash: Option<[u8; 32]>) -> Option<String> {
    hash.map(|hash| hash.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn bounded_text(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > 2 * 1024 * 1024 {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn fingerprint(path: &Path) -> Option<[u8; 32]> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    let mut hash = Sha256::new();
    if metadata.is_symlink() {
        hash.update(
            std::fs::read_link(path)
                .ok()?
                .as_os_str()
                .as_encoded_bytes(),
        );
    } else if metadata.is_file() {
        let mut file = std::fs::File::open(path).ok()?;
        let mut buffer = [0; 16 * 1024];
        loop {
            let count = file.read(&mut buffer).ok()?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
    } else {
        return None;
    }
    Some(hash.finalize().into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn snapshot(root: &Path) -> WorktreeChanges {
        WorktreeChanges::from_diffs(root, ide_core::git::workspace_worktree_diffs(root).unwrap())
    }

    #[test]
    fn shell_changes_include_existing_staged_new_deleted_and_binary_files() {
        let dir = tempfile::tempdir().unwrap();
        let root_dir = dir.path().join("repo");
        std::fs::create_dir(&root_dir).unwrap();
        let root = root_dir.as_path();
        git(root, &["init", "-q"]);
        for name in ["existing.txt", "staged.txt", "deleted.txt", "unrelated.txt"] {
            std::fs::write(root.join(name), "before\n").unwrap();
        }
        std::fs::write(root.join("image.bin"), [0, 1, 2]).unwrap();
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );
        std::fs::write(root.join("unrelated.txt"), "other writer\n").unwrap();
        std::fs::write(root.join("existing.txt"), "already dirty\n").unwrap();
        let before = snapshot(root);
        std::fs::write(root.join("existing.txt"), "feature edit\n").unwrap();
        std::fs::write(root.join("staged.txt"), "staged edit\n").unwrap();
        git(root, &["add", "staged.txt"]);
        std::fs::write(root.join("new.txt"), "new\nfile\n").unwrap();
        // Move the fixture out of the repository to exercise a deletion.
        std::fs::rename(root.join("deleted.txt"), dir.path().join("deleted-fixture")).unwrap();
        std::fs::write(root.join("image.bin"), [0, 3, 4]).unwrap();
        let after = snapshot(root);
        let changes = after.changes_since(&before);
        assert_eq!(
            changes
                .iter()
                .map(|file| file.path.to_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "deleted.txt",
                "existing.txt",
                "image.bin",
                "new.txt",
                "staged.txt"
            ]
        );
        assert!(changes.iter().all(|file| file.counts_are_projection));
        assert_eq!((changes[0].additions, changes[0].deletions), (0, 1));
        assert_eq!((changes[3].additions, changes[3].deletions), (2, 0));
        assert!(after.changes_since(&after).is_empty());
        std::fs::write(root.join("image.bin"), [0, 5, 6]).unwrap();
        assert_eq!(
            snapshot(root).changes_since(&after)[0].path,
            Path::new("image.bin")
        );
    }

    #[test]
    fn all_shell_written_files_reach_the_receipt_and_cumulative_ledger() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        std::fs::write(root.join("base.txt"), "base\n").unwrap();
        git(root, &["add", "."]);
        git(
            root,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "base",
            ],
        );
        std::fs::write(root.join("unrelated.txt"), "pre-existing\n").unwrap();
        let before = snapshot(root);
        let mut direct = Vec::new();
        for index in 0..88 {
            let path = format!("feature-{index:02}.txt");
            std::fs::write(root.join(&path), "feature\n").unwrap();
            if index < 24 {
                direct.push(FileChangeStat::new(path, 1, 0));
            }
        }
        let observed = snapshot(root).changes_since(&before);
        let mut receipt = ChangedFilesSummary::attributed("turn-large", direct, observed);
        receipt.reconcile_final_files(root);
        assert_eq!(receipt.files.len(), 24);
        assert_eq!(receipt.observed_files.len(), 64);
        let mut ledger = ChangedFilesSummary::default();
        ledger.merge_turn(&receipt);
        assert_eq!(ledger.files.len() + ledger.observed_files.len(), 88);
        assert_eq!(
            ledger.total_additions() + ledger.total_observed_additions(),
            88
        );
        assert!(!ledger
            .observed_files
            .iter()
            .any(|file| file.path == Path::new("unrelated.txt")));
    }

    #[test]
    fn reverting_a_dirty_file_emits_a_clear_but_committing_it_does_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        git(root, &["add", "."]);
        let commit = [
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "change",
        ];
        git(root, &commit);
        std::fs::write(root.join("file.txt"), "changed\n").unwrap();
        let dirty = snapshot(root);
        std::fs::write(root.join("file.txt"), "base\n").unwrap();
        let reverted = snapshot(root).changes_since(&dirty);
        assert_eq!(reverted.len(), 1);
        assert_eq!(reverted[0].path, Path::new("file.txt"));
        assert!(reverted[0].clears_projection);
        std::fs::write(root.join("file.txt"), "changed\n").unwrap();
        let dirty = snapshot(root);
        git(root, &["add", "."]);
        git(root, &commit);
        assert!(snapshot(root).changes_since(&dirty).is_empty());
    }
}
