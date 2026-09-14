use std::collections::BTreeMap;
use std::io::Read;

use sha2::{Digest, Sha256};

use super::*;

/// Git observations supplement provider edit events: shell scripts, generators,
/// and delegated tools need not emit a `fileChange` event. Keep these separate
/// from exact attribution because another writer may share this working tree.
#[derive(Default)]
pub(super) struct WorktreeChanges(BTreeMap<PathBuf, ObservedFile>);

#[derive(PartialEq, Eq)]
struct ObservedFile {
    diff: ide_core::git::FileDiff,
    fingerprint: Option<[u8; 32]>,
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
        Self(
            diffs
                .into_iter()
                .map(|diff| {
                    let path = diff.path.clone();
                    let fingerprint = fingerprint(&root.join(&path));
                    (path, ObservedFile { diff, fingerprint })
                })
                .collect(),
        )
    }

    pub(super) fn changes_since(&self, before: &Self) -> Vec<FileChangeStat> {
        self.0
            .iter()
            .filter_map(|(path, state)| {
                if before.0.get(path) == Some(state) {
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
                Some(FileChangeStat::new(path.clone(), additions, deletions).as_count_projection())
            })
            .collect()
    }
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
        let root = dir.path();
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
        std::fs::rename(
            root.join("deleted.txt"),
            dir.path().with_extension("deleted-fixture"),
        )
        .unwrap();
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
}
