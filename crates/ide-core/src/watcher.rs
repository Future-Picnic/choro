use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(300);
const WORKTREE_DEBOUNCE: Duration = Duration::from_millis(750);
const IGNORED_WORKTREE_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".next",
    "dist",
    "build",
    ".venv",
    "venv",
    "__pycache__",
    ".turbo",
    ".cache",
    ".parcel-cache",
    "DerivedData",
];

/// Watches a repository's `.git` metadata and emits one debounced tick per
/// burst of changes (branch switch, commit, stage, fetch...).
///
/// Drop the watcher to stop. The receiver yields `()` ticks.
pub struct GitWatcher {
    _watcher: RecommendedWatcher,
}

/// Watches worktree file edits for the active project and emits one debounced
/// tick per burst. Expensive/generated directories are ignored before ticks are
/// sent so background builds do not continuously refresh git status.
pub struct WorktreeWatcher {
    _watcher: RecommendedWatcher,
}

impl GitWatcher {
    pub fn new(repo_path: &Path) -> Result<(Self, mpsc::Receiver<()>)> {
        let git_dir = repo_path.join(".git");
        let (raw_tx, raw_rx) = mpsc::channel::<()>();
        let (tick_tx, tick_rx) = mpsc::channel::<()>();

        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event {
                    if is_relevant(&event) {
                        let _ = raw_tx.send(());
                    }
                }
            })?;

        // `.git` itself non-recursively catches HEAD, index and packed-refs;
        // `refs/` needs recursion for branch updates. Skips `objects/` noise.
        watcher.watch(&git_dir, RecursiveMode::NonRecursive)?;
        let refs = git_dir.join("refs");
        if refs.exists() {
            watcher.watch(&refs, RecursiveMode::Recursive)?;
        }

        // Debounce thread: first event opens a window; quiet period emits one tick.
        std::thread::spawn(move || {
            while raw_rx.recv().is_ok() {
                while raw_rx.recv_timeout(DEBOUNCE).is_ok() {}
                if tick_tx.send(()).is_err() {
                    break;
                }
            }
        });

        Ok((Self { _watcher: watcher }, tick_rx))
    }
}

impl WorktreeWatcher {
    pub fn new(repo_path: &Path) -> Result<(Self, mpsc::Receiver<()>)> {
        let (raw_tx, raw_rx) = mpsc::channel::<()>();
        let (tick_tx, tick_rx) = mpsc::channel::<()>();

        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event {
                    if is_relevant_worktree_event(&event) {
                        let _ = raw_tx.send(());
                    }
                }
            })?;

        watcher.watch(repo_path, RecursiveMode::Recursive)?;

        std::thread::spawn(move || {
            while raw_rx.recv().is_ok() {
                while raw_rx.recv_timeout(WORKTREE_DEBOUNCE).is_ok() {}
                if tick_tx.send(()).is_err() {
                    break;
                }
            }
        });

        Ok((Self { _watcher: watcher }, tick_rx))
    }
}

fn is_relevant(event: &notify::Event) -> bool {
    event.paths.iter().any(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        name == "HEAD"
            || name == "index"
            || name == "packed-refs"
            || name == "FETCH_HEAD"
            || p.components().any(|c| c.as_os_str() == "refs")
    })
}

fn is_relevant_worktree_event(event: &notify::Event) -> bool {
    event.paths.iter().any(|path| {
        let mut saw_file_name = false;
        for component in path.components() {
            let name = component.as_os_str().to_string_lossy();
            if IGNORED_WORKTREE_DIRS
                .iter()
                .any(|ignored| name.eq_ignore_ascii_case(ignored))
            {
                return false;
            }
            if !name.is_empty() {
                saw_file_name = true;
            }
        }
        saw_file_name
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{Event, EventKind};
    use std::time::Duration;

    #[test]
    fn emits_tick_on_branch_change() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create an initial commit so we can branch.
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        let (_watcher, rx) = GitWatcher::new(dir.path()).unwrap();

        let head = repo.head().unwrap().peel_to_commit().unwrap();
        repo.branch("watched", &head, false).unwrap();
        repo.set_head("refs/heads/watched").unwrap();

        let tick = rx.recv_timeout(Duration::from_secs(5));
        assert!(tick.is_ok(), "expected a watcher tick after branch change");
    }

    #[test]
    fn worktree_filter_ignores_noisy_dirs() {
        let noisy = Event {
            kind: EventKind::Any,
            paths: vec![Path::new("/repo/target/debug/app").to_path_buf()],
            attrs: Default::default(),
        };
        assert!(!is_relevant_worktree_event(&noisy));

        let source = Event {
            kind: EventKind::Any,
            paths: vec![Path::new("/repo/src/main.rs").to_path_buf()],
            attrs: Default::default(),
        };
        assert!(is_relevant_worktree_event(&source));
    }
}
