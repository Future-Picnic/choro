use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};

use git2::Repository;

const MAX_DISCOVERED_REPOSITORIES: usize = 128;
const MAX_DISCOVERY_DEPTH: usize = 8;

/// Find Git working-tree roots at or below `workspace_root`.
///
/// Choro projects are folders, not repository registrations. A project may
/// therefore contain several independent repositories. Discovery deliberately
/// stays inside the opened folder, skips generated/dependency trees, and
/// recognizes both `.git` directories and gitfiles used by worktrees and
/// submodules.
pub fn discover_repositories(workspace_root: &Path) -> Vec<PathBuf> {
    let mut repositories = Vec::new();
    let mut pending = VecDeque::from([(workspace_root.to_path_buf(), 0usize)]);

    while let Some((directory, depth)) = pending.pop_front() {
        if repositories.len() >= MAX_DISCOVERED_REPOSITORIES {
            break;
        }

        if directory.join(".git").exists() && Repository::open(&directory).is_ok() {
            repositories.push(directory.clone());
        }

        if depth >= MAX_DISCOVERY_DEPTH {
            continue;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if should_skip_directory(&name) {
                continue;
            }
            pending.push_back((path, depth + 1));
        }
    }

    repositories.sort_by(|left, right| {
        let left_depth = left.components().count();
        let right_depth = right.components().count();
        left_depth.cmp(&right_depth).then_with(|| left.cmp(right))
    });
    repositories.dedup();
    repositories
}

fn should_skip_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".choro"
            | ".codex"
            | ".claude"
            | "node_modules"
            | ".venv"
            | ".build"
            | ".swiftpm"
            | ".gradle"
            | ".terraform"
            | ".next"
            | ".nuxt"
            | ".svelte-kit"
            | ".angular"
            | ".turbo"
            | "__pycache__"
            | "target"
            | "build"
            | "DerivedData"
            | "SourcePackages"
            | "dist"
            | "vendor"
            | "Pods"
    )
}

/// Return uncommitted diffs from every repository in a workspace, with paths
/// normalized relative to the workspace root.
pub fn workspace_worktree_diffs(workspace_root: &Path) -> anyhow::Result<Vec<super::FileDiff>> {
    let repositories = discover_repositories(workspace_root);
    if repositories.is_empty() {
        return super::worktree_diffs(workspace_root);
    }

    let nested = repositories.clone();
    let mut combined = Vec::new();
    for repository in &repositories {
        let prefix = repository
            .strip_prefix(workspace_root)
            .unwrap_or(repository)
            .to_path_buf();
        for mut diff in super::worktree_diffs(repository)? {
            let absolute = repository.join(&diff.path);
            if nested.iter().any(|candidate| {
                candidate != repository
                    && candidate.starts_with(repository)
                    && absolute.starts_with(candidate)
            }) {
                continue;
            }
            diff.path = prefix.join(diff.path);
            combined.push(diff);
        }
    }
    combined.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(combined)
}

/// Inventory every staged, unstaged, deleted, renamed, and untracked path for
/// review discovery. This reads status only, without loading patches or blobs.
pub fn workspace_changed_paths(workspace_root: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let canonical_root = fs::canonicalize(workspace_root)?;
    let workspace_root = canonical_root.as_path();
    let mut repositories = discover_repositories(workspace_root);
    if repositories.len() >= MAX_DISCOVERED_REPOSITORIES {
        anyhow::bail!(
            "repository discovery limit reached; enumerate remaining repositories with Git"
        );
    }
    if repositories.is_empty() {
        // Also supports opening a project below its repository root.
        let repo = Repository::discover(workspace_root)?;
        repositories.push(
            repo.workdir()
                .ok_or_else(|| anyhow::anyhow!("repository has no working tree"))?
                .to_path_buf(),
        );
    }
    let mut paths = std::collections::BTreeSet::new();
    for root in &repositories {
        let repo = Repository::open(root)?;
        let mut options = git2::StatusOptions::new();
        options.include_untracked(true).recurse_untracked_dirs(true);
        for entry in repo.statuses(Some(&mut options))?.iter() {
            let path = Path::new(entry.path()?);
            if entry.status().contains(git2::Status::WT_NEW)
                && (super::is_internal_visualization_path(path)
                    || super::is_generated_tool_path(path))
            {
                continue;
            }
            let absolute = root.join(path);
            if repositories.iter().any(|nested| {
                nested != root && nested.starts_with(root) && absolute.starts_with(nested)
            }) {
                continue;
            }
            if let Ok(relative) = absolute.strip_prefix(workspace_root) {
                paths.insert(relative.to_path_buf());
            }
        }
    }
    Ok(paths.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn init_repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    fn commit_file(path: &Path, name: &str, contents: &str) {
        fs::write(path.join(name), contents).unwrap();
        for args in [
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
            vec!["add", name],
            vec!["commit", "-q", "-m", "initial"],
        ] {
            let status = Command::new("git")
                .args(args)
                .current_dir(path)
                .status()
                .unwrap();
            assert!(status.success());
        }
    }

    #[test]
    fn discovers_root_and_nested_repositories() {
        let workspace = tempfile::tempdir().unwrap();
        init_repo(workspace.path());
        init_repo(&workspace.path().join("apps/client"));
        init_repo(&workspace.path().join("services/api"));

        let repositories = discover_repositories(workspace.path());
        assert_eq!(repositories.len(), 3);
        assert_eq!(repositories[0], workspace.path());
        assert!(repositories.contains(&workspace.path().join("apps/client")));
        assert!(repositories.contains(&workspace.path().join("services/api")));
    }

    #[test]
    fn ignores_generated_dependency_trees() {
        let workspace = tempfile::tempdir().unwrap();
        init_repo(&workspace.path().join("node_modules/pkg"));
        init_repo(
            &workspace
                .path()
                .join("DerivedData/SourcePackages/checkouts/vendor"),
        );
        init_repo(&workspace.path().join("app"));

        assert_eq!(
            discover_repositories(workspace.path()),
            vec![workspace.path().join("app")]
        );
    }

    #[test]
    fn workspace_diffs_are_prefixed_by_repository() {
        let workspace = tempfile::tempdir().unwrap();
        let client = workspace.path().join("apps/client");
        let api = workspace.path().join("services/api");
        init_repo(&client);
        init_repo(&api);
        commit_file(&client, "client.txt", "before\n");
        commit_file(&api, "api.txt", "before\n");
        fs::write(client.join("client.txt"), "after\n").unwrap();
        fs::write(api.join("api.txt"), "after\n").unwrap();

        let paths = workspace_worktree_diffs(workspace.path())
            .unwrap()
            .into_iter()
            .map(|diff| diff.path)
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("apps/client/client.txt"),
                PathBuf::from("services/api/api.txt"),
            ]
        );
    }

    #[test]
    fn review_inventory_includes_nested_staged_untracked_and_deleted_paths() {
        let workspace = tempfile::tempdir().unwrap();
        let root = workspace.path().join("repo");
        let nested = root.join("api");
        init_repo(&root);
        init_repo(&nested);
        commit_file(&root, "deleted.txt", "before\n");
        commit_file(&root, "staged.txt", "before\n");
        commit_file(&nested, "auth.rs", "before\n");
        fs::rename(
            root.join("deleted.txt"),
            workspace.path().join("deleted-fixture"),
        )
        .unwrap();
        fs::write(root.join("staged.txt"), "after\n").unwrap();
        let repo = Repository::open(&root).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("staged.txt")).unwrap();
        index.write().unwrap();
        fs::write(nested.join("auth.rs"), "after\n").unwrap();
        fs::write(root.join("new\tfile.txt"), "new\n").unwrap();
        assert_eq!(
            workspace_changed_paths(&root).unwrap(),
            vec![
                PathBuf::from("api/auth.rs"),
                PathBuf::from("deleted.txt"),
                PathBuf::from("new\tfile.txt"),
                PathBuf::from("staged.txt"),
            ]
        );
    }
}
