use std::path::{Component, Path};

use anyhow::{anyhow, bail, Context, Result};
use git2::{build::CheckoutBuilder, BranchType, ErrorCode, Repository};

/// Stage files (handles new, modified and deleted paths).
pub fn stage(repo_path: &Path, paths: &[&Path]) -> Result<()> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    let mut index = repo.index()?;
    let workdir = repo.workdir().context("bare repository")?;
    for path in paths {
        if workdir.join(path).exists() {
            index.add_path(path)?;
        } else {
            index.remove_path(path)?;
        }
    }
    index.write()?;
    Ok(())
}

/// Unstage files (reset index entries back to HEAD).
pub fn unstage(repo_path: &Path, paths: &[&Path]) -> Result<()> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    match repo.head() {
        Ok(head) => {
            let commit = head.peel_to_commit()?;
            repo.reset_default(Some(commit.as_object()), paths)?;
        }
        Err(_) => {
            // Unborn HEAD: unstage means removing the entry from the index.
            let mut index = repo.index()?;
            for path in paths {
                index.remove_path(path)?;
            }
            index.write()?;
        }
    }
    Ok(())
}

/// Discard unstaged changes to the given files (restore from index).
pub fn discard(repo_path: &Path, paths: &[&Path]) -> Result<()> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    let index = repo.index()?;
    let workdir = repo.workdir().context("bare repository")?;
    let mut checkout = CheckoutBuilder::new();
    checkout.force().update_index(false);
    let mut has_index_paths = false;
    for path in paths {
        if !safe_repo_relative_path(path) {
            bail!("refusing to discard path outside the repository");
        }
        if (0..=3).any(|stage| index.get_path(path, stage).is_some()) {
            checkout.path(path);
            has_index_paths = true;
        } else {
            let full_path = workdir.join(path);
            match std::fs::symlink_metadata(&full_path) {
                Ok(metadata) if metadata.is_dir() => {
                    std::fs::remove_dir_all(&full_path)
                        .with_context(|| format!("removing {}", path.display()))?;
                }
                Ok(_) => {
                    std::fs::remove_file(&full_path)
                        .with_context(|| format!("removing {}", path.display()))?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("reading {}", path.display()));
                }
            }
        }
    }
    if has_index_paths {
        repo.checkout_index(None, Some(&mut checkout))?;
    }
    Ok(())
}

fn safe_repo_relative_path(path: &Path) -> bool {
    !path.is_absolute()
        && path.components().next().is_some()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitResult {
    pub sha: String,
    pub sha_short: String,
}

pub fn commit(repo_path: &Path, message: &str) -> Result<CommitResult> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    if message.trim().is_empty() {
        bail!("commit message is empty");
    }
    let repo = Repository::open(repo_path)?;
    let signature = repo
        .signature()
        .context("git user.name / user.email not configured")?;
    let mut index = repo.index()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    if let Some(parent) = &parent {
        if parent.tree_id() == tree_id {
            bail!("nothing staged to commit");
        }
    } else if tree.is_empty() {
        bail!("nothing staged to commit");
    }

    let parents: Vec<_> = parent.iter().collect();
    let oid = repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &parents,
    )?;
    let sha = oid.to_string();
    Ok(CommitResult {
        sha_short: sha[..7].to_string(),
        sha,
    })
}

/// Safe checkout: refuses to clobber local changes.
pub fn checkout_branch(repo_path: &Path, name: &str) -> Result<()> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    let reference = repo
        .find_reference(&format!("refs/heads/{name}"))
        .with_context(|| format!("branch '{name}' not found"))?;
    let tree = reference.peel_to_tree()?;

    let mut checkout = CheckoutBuilder::new();
    checkout.safe();
    repo.checkout_tree(tree.as_object(), Some(&mut checkout))
        .map_err(|e| {
            if e.code() == ErrorCode::Conflict {
                anyhow!("checkout would overwrite local changes — commit or stash them first")
            } else {
                e.into()
            }
        })?;
    repo.set_head(&format!("refs/heads/{name}"))?;
    super::archive::restore_branch(&repo, name)?;
    Ok(())
}

/// Checks out a local branch, or creates a local tracking branch from a
/// remote-tracking branch such as `origin/feature/x`.
pub fn checkout_branch_or_remote(repo_path: &Path, name: &str) -> Result<String> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    if repo.find_branch(name, BranchType::Local).is_ok() {
        drop(repo);
        checkout_branch(repo_path, name)?;
        return Ok(name.to_string());
    }

    let remote_branch = repo
        .find_branch(name, BranchType::Remote)
        .with_context(|| format!("branch '{name}' not found"))?;
    let local_name = local_name_for_remote_branch(&remote_branch, name)?;
    if repo.find_branch(&local_name, BranchType::Local).is_err() {
        let commit = remote_branch.get().peel_to_commit()?;
        let mut local_branch = repo.branch(&local_name, &commit, false)?;
        local_branch.set_upstream(Some(name))?;
    }
    drop(remote_branch);
    drop(repo);
    checkout_branch(repo_path, &local_name)?;
    Ok(local_name)
}

fn local_name_for_remote_branch(branch: &git2::Branch<'_>, remote_name: &str) -> Result<String> {
    let mut local_name = remote_name
        .split_once('/')
        .map(|(_, name)| name.to_string())
        .filter(|name| !name.is_empty())
        .with_context(|| format!("remote branch '{remote_name}' is not checkoutable"))?;

    if local_name == "HEAD" {
        let symbolic_target = branch
            .get()
            .symbolic_target()
            .with_context(|| format!("remote branch '{remote_name}' is not checkoutable"))?
            .with_context(|| format!("remote branch '{remote_name}' is not checkoutable"))?;
        let remote_prefix = format!(
            "refs/remotes/{}/",
            remote_name
                .split_once('/')
                .map(|(remote, _)| remote)
                .unwrap_or("")
        );
        local_name = symbolic_target
            .strip_prefix(&remote_prefix)
            .map(str::to_string)
            .filter(|name| !name.is_empty() && name != "HEAD")
            .with_context(|| format!("remote branch '{remote_name}' is not checkoutable"))?;
    }

    Ok(local_name)
}

/// Create a new branch at HEAD and switch to it.
pub fn create_branch(repo_path: &Path, name: &str) -> Result<()> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    let head = repo.head().context("repository has no commits yet")?;
    let commit = head.peel_to_commit()?;
    repo.branch(name, &commit, false)?;
    drop(commit);
    drop(head);
    checkout_branch(repo_path, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;
    use crate::git::{read_snapshot, ChangeKind};
    use std::fs;

    #[test]
    fn stage_commit_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("new.txt"), "data\n").unwrap();

        stage(dir.path(), &[Path::new("new.txt")]).unwrap();
        let snapshot = read_snapshot(dir.path()).unwrap();
        let entry = snapshot
            .entries
            .iter()
            .find(|e| e.path.to_str() == Some("new.txt"))
            .unwrap();
        assert_eq!(entry.staged, Some(ChangeKind::Added));

        let result = commit(dir.path(), "feat: add new.txt").unwrap();
        assert_eq!(result.sha_short.len(), 7);
        assert_eq!(result.sha.len(), 40);
        assert!(read_snapshot(dir.path()).unwrap().entries.is_empty());
    }

    #[test]
    fn stage_deleted_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        fs::remove_file(workdir(&repo).join("README.md")).unwrap();

        stage(dir.path(), &[Path::new("README.md")]).unwrap();
        let snapshot = read_snapshot(dir.path()).unwrap();
        assert_eq!(snapshot.entries[0].staged, Some(ChangeKind::Deleted));
    }

    #[test]
    fn unstage_returns_file_to_unstaged() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("README.md"), "edited\n").unwrap();
        stage(dir.path(), &[Path::new("README.md")]).unwrap();
        unstage(dir.path(), &[Path::new("README.md")]).unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        let entry = &snapshot.entries[0];
        assert_eq!(entry.staged, None);
        assert_eq!(entry.unstaged, Some(ChangeKind::Modified));
    }

    #[test]
    fn commit_with_nothing_staged_fails() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_commit(dir.path());
        assert!(commit(dir.path(), "empty").is_err());
    }

    #[test]
    fn checkout_switches_branch() {
        let dir = tempfile::tempdir().unwrap();
        repo_with_commit(dir.path());
        create_branch(dir.path(), "feature/x").unwrap();
        let head = crate::git::read_head(dir.path()).unwrap();
        assert_eq!(head.branch.as_deref(), Some("feature/x"));
    }

    #[test]
    fn checkout_remote_branch_creates_tracking_local_branch() {
        let remote_dir = tempfile::tempdir().unwrap();
        git2::Repository::init_bare(remote_dir.path()).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        repo.remote("origin", remote_dir.path().to_str().unwrap())
            .unwrap();
        let main_branch = crate::git::read_head(dir.path()).unwrap().branch.unwrap();
        let pushed = crate::git::remote::push(dir.path(), Some(&main_branch), true).unwrap();
        assert!(pushed.success, "push failed: {}", pushed.message());

        create_branch(dir.path(), "feature/scoring").unwrap();
        fs::write(workdir(&repo).join("feature.txt"), "feature\n").unwrap();
        commit_all(&repo, "feat: add scoring");
        let pushed = crate::git::remote::push(dir.path(), Some("feature/scoring"), true).unwrap();
        assert!(pushed.success, "push failed: {}", pushed.message());

        checkout_branch(dir.path(), &main_branch).unwrap();
        repo.find_branch("feature/scoring", BranchType::Local)
            .unwrap()
            .delete()
            .unwrap();

        let snapshot = read_snapshot(dir.path()).unwrap();
        assert!(snapshot
            .branches
            .iter()
            .any(|branch| branch.is_remote && branch.name == "origin/feature/scoring"));

        let local = checkout_branch_or_remote(dir.path(), "origin/feature/scoring").unwrap();
        assert_eq!(local, "feature/scoring");
        let head = crate::git::read_head(dir.path()).unwrap();
        assert_eq!(head.branch.as_deref(), Some("feature/scoring"));
        let repo = Repository::open(dir.path()).unwrap();
        let upstream = repo
            .find_branch("feature/scoring", BranchType::Local)
            .unwrap()
            .upstream()
            .unwrap()
            .name()
            .unwrap()
            .unwrap()
            .to_string();
        assert_eq!(upstream, "origin/feature/scoring");
    }

    #[test]
    fn checkout_refuses_to_clobber_dirty_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        let main_branch = crate::git::read_head(dir.path()).unwrap().branch.unwrap();

        create_branch(dir.path(), "feature/x").unwrap();
        fs::write(root.join("README.md"), "conflicting local edit\n").unwrap();
        commit_all(&repo, "edit on feature/x");

        // Dirty the file with content that differs between branches.
        fs::write(root.join("README.md"), "uncommitted\n").unwrap();
        let result = checkout_branch(dir.path(), &main_branch);
        assert!(result.is_err());

        let result = checkout_branch_or_remote(dir.path(), &main_branch);
        let error = format!("{:#}", result.unwrap_err());
        assert!(
            error.contains("overwrite local changes"),
            "unexpected checkout error: {error}"
        );
    }

    #[test]
    fn discard_restores_file() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let root = workdir(&repo);
        fs::write(root.join("README.md"), "scratch\n").unwrap();

        discard(dir.path(), &[Path::new("README.md")]).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("README.md")).unwrap(),
            "hello\n"
        );
    }
}
