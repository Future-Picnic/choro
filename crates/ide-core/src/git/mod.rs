mod accounts;
mod background;
pub use background::BackgroundGitPermit;
pub mod diff;
pub mod ignore;
pub mod log;
pub mod read;
pub mod remote;
pub mod repository;
pub mod snapshot;
pub mod write;
mod worktree_diff;

use std::path::{Component, Path};

/// Generated Codex visualization files are conversation artifacts, not project
/// source. Choro keeps newly generated files outside the repository, while
/// this recognizes the legacy/fallback location used by Codex clients.
pub fn is_internal_visualization_path(path: &Path) -> bool {
    let mut components = path.components();
    matches!(components.next(), Some(Component::Normal(part)) if part == ".codex")
        && matches!(components.next(), Some(Component::Normal(part)) if part == "visualizations")
}

/// Tool-owned dependency and cache directories that are safe to recreate.
///
/// This intentionally excludes ambiguous names such as `build`, `dist`,
/// `target`, `vendor`, and `Pods`, which some projects deliberately commit.
pub const GENERATED_TOOL_DIRECTORIES: &[&str] = &[
    "node_modules",
    ".venv",
    ".gradle",
    ".terraform",
    ".dart_tool",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".angular",
    ".turbo",
    ".parcel-cache",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".nox",
];

/// UI/read backstop for repositories whose local exclude file cannot be
/// updated. Tracked paths are never suppressed by callers.
pub fn is_generated_tool_path(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(
            component,
            Component::Normal(part)
                if GENERATED_TOOL_DIRECTORIES
                    .iter()
                    .any(|name| part == std::ffi::OsStr::new(name))
        )
    })
}

#[cfg(all(target_os = "macos", feature = "app-update-bridge"))]
pub use accounts::start_authenticated_choro_release_updater;
pub use accounts::{
    assign_github_account, assigned_github_account, choro_release_credential,
    configure_selected_github_cli, connected_github_accounts, handle_git_credential,
    open_github_account_login, primary_remote, repository_remotes, ChoroReleaseCredential,
    GitHubAccount, GitRemote,
};
pub use diff::{DiffHunk, DiffLine, FileDiff, LineOrigin};
pub use ignore::{ensure_local_choro_docs_exclude, ensure_local_dependency_excludes};
pub use log::{ahead_behind, commit_diff, list_commits, worktree_diffs, CommitInfo};
pub use read::{read_head, read_snapshot};
pub use remote::{fetch, pull, push, RemoteOutput};
pub use repository::{discover_repositories, workspace_worktree_diffs};
pub use snapshot::{BranchInfo, ChangeKind, GitSnapshot, HeadInfo, LineStats, StatusEntry};

#[cfg(test)]
mod tests {
    use super::{is_generated_tool_path, is_internal_visualization_path};
    use std::path::Path;

    #[test]
    fn recognizes_only_the_codex_visualization_directory() {
        assert!(is_internal_visualization_path(Path::new(
            ".codex/visualizations/2026/chart.html"
        )));
        assert!(!is_internal_visualization_path(Path::new(
            ".codex/settings.json"
        )));
        assert!(!is_internal_visualization_path(Path::new(
            "docs/.codex/visualizations/chart.html"
        )));
    }

    #[test]
    fn recognizes_generated_tool_directories_at_any_depth() {
        assert!(is_generated_tool_path(Path::new(
            "node_modules/pkg/index.js"
        )));
        assert!(is_generated_tool_path(Path::new("api/.venv/bin/python")));
        assert!(is_generated_tool_path(Path::new(
            "infra/.terraform/providers/plugin"
        )));
        assert!(is_generated_tool_path(Path::new(".next/server/app.js")));
        assert!(!is_generated_tool_path(Path::new("src/node_modules.ts")));
        assert!(!is_generated_tool_path(Path::new("vendor/pkg/index.php")));
        assert!(!is_generated_tool_path(Path::new("target/source.rs")));
    }
}
