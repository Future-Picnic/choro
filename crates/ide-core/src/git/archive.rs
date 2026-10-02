//! Regression coverage for preserving local branches, including repositories
//! that received the earlier local archive metadata.
use crate::git::{read::fixtures::*, read_snapshot, write::checkout_branch_or_remote};
use git2::BranchType;

#[test]
fn legacy_archive_metadata_keeps_local_branches_visible_and_checkout_preserves_history() {
    let dir = tempfile::tempdir().unwrap();
    let repo = repo_with_commit(dir.path());
    let original_branch = repo.head().unwrap().shorthand().unwrap().to_string();
    let original_head = repo.head().unwrap().target().unwrap();
    repo.branch(
        "feature/old-data",
        &repo.find_commit(original_head).unwrap(),
        false,
    )
    .unwrap();
    repo.reference(
        "refs/remotes/origin/feature/old-data",
        original_head,
        true,
        "test",
    )
    .unwrap();
    let key = "branch.feature/old-data.choroArchivedHead";
    repo.config()
        .unwrap()
        .set_str(key, &original_head.to_string())
        .unwrap();
    let before = std::fs::read(workdir(&repo).join("README.md")).unwrap();

    let snapshot = read_snapshot(dir.path()).unwrap();
    assert!(snapshot
        .branches
        .iter()
        .any(|b| b.name == "feature/old-data"));
    assert!(snapshot
        .branches
        .iter()
        .any(|b| b.name == "origin/feature/old-data"));
    checkout_branch_or_remote(dir.path(), "feature/old-data").unwrap();
    assert_eq!(
        repo.find_branch("feature/old-data", BranchType::Local)
            .unwrap()
            .get()
            .target(),
        Some(original_head)
    );
    assert_eq!(
        repo.find_branch(&original_branch, BranchType::Local)
            .unwrap()
            .get()
            .target(),
        Some(original_head)
    );
    assert_eq!(
        std::fs::read(workdir(&repo).join("README.md")).unwrap(),
        before
    );
    assert_eq!(
        repo.config().unwrap().get_string(key).unwrap(),
        original_head.to_string()
    );
}
