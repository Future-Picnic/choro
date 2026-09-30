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
}
