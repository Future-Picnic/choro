//! Repository-local safety exclusions managed by Choro.
//!
//! These rules live in Git's common `info/exclude`, not in the project's
//! tracked `.gitignore`. Git therefore applies them to the main worktree and
//! every linked worktree without Choro creating a project change of its own.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result};
use git2::Repository;

const BLOCK_START: &str = "# >>> Choro generated-dependency safety >>>";
const BLOCK_END: &str = "# <<< Choro generated-dependency safety <<<";
const DOCS_BLOCK_START: &str = "# >>> Choro Solo document snapshots >>>";
const DOCS_BLOCK_END: &str = "# <<< Choro Solo document snapshots <<<";

fn managed_block() -> String {
    let mut block = format!(
        "{BLOCK_START}\n\
         # Local-only Git safety. Keeps untracked tool output out of Choro changes.\n"
    );
    for directory in super::GENERATED_TOOL_DIRECTORIES {
        block.push_str(directory);
        block.push_str("/\n");
    }
    block.push_str(BLOCK_END);
    block.push('\n');
    block
}

/// Install Choro's local-only dependency exclusions for a repository.
///
/// Returns `true` when the exclude file changed. The managed block is replaced
/// in place when it already exists, so upgrades stay idempotent and preserve
/// every user-authored rule around it.
pub fn ensure_local_dependency_excludes(repo_path: &Path) -> Result<bool> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    let exclude_path = repo.commondir().join("info").join("exclude");
    let existing = match fs::read_to_string(&exclude_path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", exclude_path.display()));
        }
    };
    let updated = install_managed_block(&existing);
    if updated == existing {
        return Ok(false);
    }
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&exclude_path, updated)
        .with_context(|| format!("failed to write {}", exclude_path.display()))?;
    Ok(true)
}

/// Keep Choro's read-only Solo document snapshots out of Git without changing
/// the project's tracked `.gitignore`. Git still reports a tracked
/// `choro_docs` tree normally; this rule only covers generated snapshots.
pub fn ensure_local_choro_docs_exclude(repo_path: &Path) -> Result<bool> {
    let repo = Repository::open(repo_path).context("not a git repository")?;
    let exclude_path = repo.commondir().join("info").join("exclude");
    let existing = match fs::read_to_string(&exclude_path) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => String::new(),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read {}", exclude_path.display()));
        }
    };
    let updated = install_choro_docs_block(&existing);
    if updated == existing {
        return Ok(false);
    }
    if let Some(parent) = exclude_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&exclude_path, updated)
        .with_context(|| format!("failed to write {}", exclude_path.display()))?;
    Ok(true)
}

fn install_managed_block(existing: &str) -> String {
    let mut without_old = existing.to_string();
    if let Some(start) = existing.find(BLOCK_START) {
        if let Some(relative_end) = existing[start..].find(BLOCK_END) {
            let mut end = start + relative_end + BLOCK_END.len();
            if existing.as_bytes().get(end) == Some(&b'\n') {
                end += 1;
            }
            without_old.replace_range(start..end, "");
        }
    }

    let mut output = without_old.trim_end_matches('\n').to_string();
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(&managed_block());
    output
}

fn install_choro_docs_block(existing: &str) -> String {
    let mut without_old = existing.to_string();
    if let Some(start) = existing.find(DOCS_BLOCK_START) {
        if let Some(relative_end) = existing[start..].find(DOCS_BLOCK_END) {
            let mut end = start + relative_end + DOCS_BLOCK_END.len();
            if existing.as_bytes().get(end) == Some(&b'\n') {
                end += 1;
            }
            without_old.replace_range(start..end, "");
        }
    }

    let mut output = without_old.trim_end_matches('\n').to_string();
    if !output.is_empty() {
        output.push_str("\n\n");
    }
    output.push_str(DOCS_BLOCK_START);
    output.push_str("\n# Generated read-only context for Solo agents.\n/");
    output.push_str(crate::branding::DOCS_DIR_NAME);
    output.push_str("/\n");
    output.push_str(DOCS_BLOCK_END);
    output.push('\n');
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::repo_with_commit;
    use std::process::Command;

    #[test]
    fn installs_local_dependency_ignore_without_touching_project_files() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        fs::create_dir_all(dir.path().join("node_modules/pkg")).unwrap();
        fs::write(dir.path().join("node_modules/pkg/index.js"), "generated\n").unwrap();

        assert!(ensure_local_dependency_excludes(dir.path()).unwrap());
        assert!(!dir.path().join(".gitignore").exists());

        let ignored = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["check-ignore", "--quiet", "--"])
            .arg("node_modules/pkg/index.js")
            .status()
            .unwrap();
        assert!(ignored.success());
    }

    #[test]
    fn managed_block_is_idempotent_and_preserves_user_rules() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let exclude = repo.commondir().join("info").join("exclude");
        fs::write(&exclude, "*.local-cache\n").unwrap();

        assert!(ensure_local_dependency_excludes(dir.path()).unwrap());
        assert!(!ensure_local_dependency_excludes(dir.path()).unwrap());

        let text = fs::read_to_string(exclude).unwrap();
        assert!(text.contains("*.local-cache"));
        assert_eq!(text.matches(BLOCK_START).count(), 1);
        assert_eq!(text.matches("node_modules/").count(), 1);
        assert_eq!(text.matches(".venv/").count(), 1);
        assert_eq!(text.matches(".terraform/").count(), 1);
    }

    #[test]
    fn installs_solo_docs_ignore_without_touching_project_files() {
        let dir = tempfile::tempdir().unwrap();
        let _repo = repo_with_commit(dir.path());
        fs::create_dir_all(dir.path().join("choro_docs")).unwrap();
        fs::write(dir.path().join("choro_docs/spec.choro"), "{}\n").unwrap();

        assert!(ensure_local_choro_docs_exclude(dir.path()).unwrap());
        assert!(!ensure_local_choro_docs_exclude(dir.path()).unwrap());
        assert!(!dir.path().join(".gitignore").exists());

        let ignored = Command::new("git")
            .arg("-C")
            .arg(dir.path())
            .args(["check-ignore", "--quiet", "--"])
            .arg("choro_docs/spec.choro")
            .status()
            .unwrap();
        assert!(ignored.success());
    }
}
