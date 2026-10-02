use std::path::Path;

use anyhow::{bail, Context, Result};
use git2::{BranchType, Config, ConfigLevel, Oid, Repository};

fn archive_key(branch: &str) -> String {
    format!("branch.{branch}.choroArchivedHead")
}

/// Keep every ref and working file intact; archive only the merged tip in
/// repository-local metadata. New commits make the branch active again.
pub fn archive_merged_branch(repo_path: &Path, branch: &str, merged_head: &str) -> Result<()> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let repo = Repository::open(repo_path)?;
    let merged_head = Oid::from_str(merged_head)?;
    if let Ok(local) = repo.find_branch(branch, BranchType::Local) {
        if local.get().target() != Some(merged_head) {
            bail!("Branch {branch} has local commits beyond the merged PR; it was kept active");
        }
    }
    repo.config()?
        .open_level(ConfigLevel::Local)?
        .set_str(&archive_key(branch), &merged_head.to_string())
        .context("Could not save the branch archive state")?;
    Ok(())
}

pub(super) fn is_archived(config: &Config, branch: &str, tip: Option<Oid>) -> bool {
    config
        .get_string(&archive_key(branch))
        .ok()
        .and_then(|value| Oid::from_str(&value).ok())
        .is_some_and(|archived_tip| Some(archived_tip) == tip)
}

pub(super) fn restore_branch(repo: &Repository, branch: &str) -> Result<()> {
    let mut config = repo.config()?.open_level(ConfigLevel::Local)?;
    if config.get_string(&archive_key(branch)).is_ok() {
        config.set_str(&archive_key(branch), "")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{read::fixtures::*, read_snapshot, write::checkout_branch_or_remote};

    #[test]
    fn archive_keeps_refs_and_files_and_checkout_restores() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let head = repo.head().unwrap().peel_to_commit().unwrap().id();
        repo.branch("feature/archive", &repo.find_commit(head).unwrap(), false)
            .unwrap();
        repo.reference("refs/remotes/origin/feature/archive", head, true, "test")
            .unwrap();
        let before = std::fs::read(workdir(&repo).join("README.md")).unwrap();

        archive_merged_branch(dir.path(), "feature/archive", &head.to_string()).unwrap();
        let snapshot = read_snapshot(dir.path()).unwrap();
        let archived: Vec<_> = snapshot
            .branches
            .iter()
            .filter(|b| b.name.ends_with("feature/archive"))
            .collect();
        assert_eq!(archived.len(), 2);
        assert!(archived.iter().all(|b| b.is_archived));
        assert!(repo
            .find_branch("feature/archive", BranchType::Local)
            .is_ok());
        assert_eq!(
            std::fs::read(workdir(&repo).join("README.md")).unwrap(),
            before
        );

        checkout_branch_or_remote(dir.path(), "feature/archive").unwrap();
        assert!(read_snapshot(dir.path())
            .unwrap()
            .branches
            .iter()
            .all(|b| !b.is_archived));
    }

    #[test]
    fn archive_refuses_unmerged_local_commits_and_new_commits_reactivate() {
        let dir = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(dir.path());
        let branch = repo.head().unwrap().shorthand().unwrap().to_string();
        let merged = repo.head().unwrap().target().unwrap();
        archive_merged_branch(dir.path(), &branch, &merged.to_string()).unwrap();
        std::fs::write(workdir(&repo).join("README.md"), "new work\n").unwrap();
        commit_all(&repo, "new work");
        assert!(
            !read_snapshot(dir.path())
                .unwrap()
                .branches
                .iter()
                .find(|b| b.is_head)
                .unwrap()
                .is_archived
        );
        assert!(archive_merged_branch(dir.path(), &branch, &merged.to_string()).is_err());
    }
}
