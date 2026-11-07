//! After a pull request merges, bring the local checkout back to the branch it
//! merged into and fast-forward it, so the next piece of work starts from the
//! merged result instead of the finished source branch.
//!
//! Switching uses the `git switch` CLI on purpose: it refuses atomically when
//! the target is checked out in another worktree or when local changes would
//! be overwritten, so a failure never leaves a half-switched checkout.

use std::path::Path;

use anyhow::{bail, Result};
use git2::{BranchType, Repository};

use super::remote::run_git;

/// What the local checkout should do once the merge has landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnPlan {
    /// Already on the base branch; only bring it up to date.
    PullOnly,
    /// Still on the merged source branch; switch to the base, then update it.
    SwitchAndPull,
    /// The checkout has moved to unrelated work since the PR was opened, so
    /// it is left exactly as it is.
    Stay { current: String },
}

/// The local result of [`return_to_base_after_merge`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnOutcome {
    /// The base branch is checked out and fast-forwarded.
    Updated { base: String, switched: bool },
    /// The checkout was on another branch and was not touched.
    Stayed { current: String },
}

/// Decides the local step from the branch currently checked out.
pub fn plan_return_to_base(
    current: Option<&str>,
    merged_branch: &str,
    base_branch: &str,
) -> ReturnPlan {
    match current {
        Some(current) if current == base_branch => ReturnPlan::PullOnly,
        Some(current) if current == merged_branch => ReturnPlan::SwitchAndPull,
        Some(current) => ReturnPlan::Stay {
            current: current.to_string(),
        },
        None => ReturnPlan::Stay {
            current: "a detached HEAD".to_string(),
        },
    }
}

/// Switches to `base_branch` (when still on `merged_branch`) and fast-forwards
/// it from its remote. Never discards local changes and never creates merge
/// commits: anything that is not a clean switch plus fast-forward is reported
/// as an error and left for the user.
pub fn return_to_base_after_merge(
    repo_path: &Path,
    merged_branch: &str,
    base_branch: &str,
) -> Result<ReturnOutcome> {
    let _git_permit = super::BackgroundGitPermit::acquire();
    let base = base_branch.trim();
    if base.is_empty() {
        bail!("The pull request has no base branch to return to");
    }
    let head = super::read_head(repo_path)?;
    let switched = match plan_return_to_base(head.branch.as_deref(), merged_branch.trim(), base) {
        ReturnPlan::Stay { current } => return Ok(ReturnOutcome::Stayed { current }),
        ReturnPlan::PullOnly => false,
        ReturnPlan::SwitchAndPull => {
            switch_to_branch(repo_path, base)?;
            true
        }
    };
    pull_branch(repo_path, base).map_err(|error| {
        if switched {
            anyhow::anyhow!("Switched to {base}, but pulling failed: {error:#}")
        } else {
            anyhow::anyhow!("Couldn't pull {base}: {error:#}")
        }
    })?;
    Ok(ReturnOutcome::Updated {
        base: base.to_string(),
        switched,
    })
}

fn switch_to_branch(repo_path: &Path, base: &str) -> Result<()> {
    // A base that only exists on the remote is created as a tracking branch by
    // `git switch`, which needs an up-to-date remote-tracking ref to find it.
    if !local_branch_exists(repo_path, base)? {
        let fetched = super::remote::fetch(repo_path)?;
        if !fetched.success {
            bail!("Couldn't fetch {base}: {}", fetched.message());
        }
    }
    let output = run_git(repo_path, &["switch", base])?;
    if !output.success {
        bail!("Couldn't switch to {base}: {}", output.message());
    }
    Ok(())
}

fn pull_branch(repo_path: &Path, base: &str) -> Result<()> {
    let output = match upstream_remote(repo_path, base)? {
        Some(_) => run_git(repo_path, &["pull", "--ff-only"])?,
        None => {
            let repo = Repository::open(repo_path)?;
            let Some(remote) = super::accounts::primary_remote_of(&repo) else {
                bail!("{base} has no remote to pull from");
            };
            run_git(repo_path, &["pull", "--ff-only", &remote.name, base])?
        }
    };
    if !output.success {
        bail!("{}", output.message());
    }
    Ok(())
}

fn local_branch_exists(repo_path: &Path, name: &str) -> Result<bool> {
    let repo = Repository::open(repo_path)?;
    let exists = repo.find_branch(name, BranchType::Local).is_ok();
    Ok(exists)
}

fn upstream_remote(repo_path: &Path, name: &str) -> Result<Option<String>> {
    let repo = Repository::open(repo_path)?;
    let Ok(branch) = repo.find_branch(name, BranchType::Local) else {
        return Ok(None);
    };
    let upstream = branch
        .upstream()
        .ok()
        .and_then(|upstream| upstream.name().ok().flatten().map(str::to_string));
    Ok(upstream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::read::fixtures::*;
    use crate::git::write::{checkout_branch, create_branch};
    use std::fs;

    #[test]
    fn plan_switches_only_from_the_merged_branch() {
        assert_eq!(
            plan_return_to_base(Some("feat/x"), "feat/x", "main"),
            ReturnPlan::SwitchAndPull
        );
        assert_eq!(
            plan_return_to_base(Some("main"), "feat/x", "main"),
            ReturnPlan::PullOnly
        );
        assert_eq!(
            plan_return_to_base(Some("feat/other"), "feat/x", "main"),
            ReturnPlan::Stay {
                current: "feat/other".into()
            }
        );
        assert_eq!(
            plan_return_to_base(None, "feat/x", "main"),
            ReturnPlan::Stay {
                current: "a detached HEAD".into()
            }
        );
    }

    /// A clone of a bare remote whose base branch received a merge the local
    /// copy has not seen yet, with the local checkout still on `feat/x`.
    struct MergedFixture {
        _remote: tempfile::TempDir,
        _other: tempfile::TempDir,
        local: tempfile::TempDir,
        base: String,
        merged_tip: git2::Oid,
    }

    fn merged_fixture() -> MergedFixture {
        let remote = tempfile::tempdir().unwrap();
        Repository::init_bare(remote.path()).unwrap();
        let remote_url = remote.path().to_str().unwrap().to_string();

        let local = tempfile::tempdir().unwrap();
        let repo = repo_with_commit(local.path());
        repo.remote("origin", &remote_url).unwrap();
        let base = crate::git::read_head(local.path()).unwrap().branch.unwrap();
        let pushed = crate::git::remote::push(local.path(), Some(&base), true).unwrap();
        assert!(pushed.success, "push failed: {}", pushed.message());
        create_branch(local.path(), "feat/x").unwrap();

        // Simulate GitHub merging the PR: the base moves ahead on the remote.
        let other = tempfile::tempdir().unwrap();
        let other_repo = repo_with_commit(other.path());
        other_repo.remote("origin", &remote_url).unwrap();
        let fetched = crate::git::remote::fetch(other.path()).unwrap();
        assert!(fetched.success, "fetch failed: {}", fetched.message());
        let remote_base = other_repo
            .find_reference(&format!("refs/remotes/origin/{base}"))
            .unwrap()
            .peel_to_commit()
            .unwrap();
        other_repo
            .reset(remote_base.as_object(), git2::ResetType::Hard, None)
            .unwrap();
        fs::write(other.path().join("merged.txt"), "merged\n").unwrap();
        let merged_tip = commit_all(&other_repo, "Merge pull request #1");
        let pushed = crate::git::remote::push(other.path(), Some(&base), true).unwrap();
        assert!(pushed.success, "push failed: {}", pushed.message());

        MergedFixture {
            _remote: remote,
            _other: other,
            local,
            base,
            merged_tip,
        }
    }

    fn local_head_oid(path: &Path) -> git2::Oid {
        Repository::open(path)
            .unwrap()
            .head()
            .unwrap()
            .target()
            .unwrap()
    }

    #[test]
    fn switches_from_the_merged_branch_and_fast_forwards_the_base() {
        let fixture = merged_fixture();
        let outcome =
            return_to_base_after_merge(fixture.local.path(), "feat/x", &fixture.base).unwrap();

        assert_eq!(
            outcome,
            ReturnOutcome::Updated {
                base: fixture.base.clone(),
                switched: true
            }
        );
        let head = crate::git::read_head(fixture.local.path()).unwrap();
        assert_eq!(head.branch.as_deref(), Some(fixture.base.as_str()));
        assert_eq!(local_head_oid(fixture.local.path()), fixture.merged_tip);
    }

    #[test]
    fn leaves_unrelated_branches_alone() {
        let fixture = merged_fixture();
        create_branch(fixture.local.path(), "feat/other").unwrap();

        let outcome =
            return_to_base_after_merge(fixture.local.path(), "feat/x", &fixture.base).unwrap();

        assert_eq!(
            outcome,
            ReturnOutcome::Stayed {
                current: "feat/other".into()
            }
        );
        let head = crate::git::read_head(fixture.local.path()).unwrap();
        assert_eq!(head.branch.as_deref(), Some("feat/other"));
    }

    #[test]
    fn pulls_when_already_on_the_base() {
        let fixture = merged_fixture();
        checkout_branch(fixture.local.path(), &fixture.base).unwrap();

        let outcome =
            return_to_base_after_merge(fixture.local.path(), "feat/x", &fixture.base).unwrap();

        assert_eq!(
            outcome,
            ReturnOutcome::Updated {
                base: fixture.base.clone(),
                switched: false
            }
        );
        assert_eq!(local_head_oid(fixture.local.path()), fixture.merged_tip);
    }

    #[test]
    fn keeps_the_checkout_when_local_changes_would_be_overwritten() {
        let fixture = merged_fixture();
        let local = Repository::open(fixture.local.path()).unwrap();
        fs::write(fixture.local.path().join("README.md"), "feature edit\n").unwrap();
        commit_all(&local, "feat: edit readme");
        fs::write(fixture.local.path().join("README.md"), "uncommitted\n").unwrap();

        let error = return_to_base_after_merge(fixture.local.path(), "feat/x", &fixture.base)
            .unwrap_err()
            .to_string();

        assert!(
            error.contains("Couldn't switch"),
            "unexpected error: {error}"
        );
        let head = crate::git::read_head(fixture.local.path()).unwrap();
        assert_eq!(head.branch.as_deref(), Some("feat/x"));
        assert_eq!(
            fs::read_to_string(fixture.local.path().join("README.md")).unwrap(),
            "uncommitted\n"
        );
    }
}
