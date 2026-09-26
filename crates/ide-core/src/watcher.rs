use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock, Weak,
};
use std::time::Duration;

use anyhow::Result;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};

const DEBOUNCE: Duration = Duration::from_millis(300);
const WORKTREE_DEBOUNCE: Duration = Duration::from_millis(750);
// A debounce that waits for quiet can be starved forever by a steady event
// stream (a long rebase churning the index, a tool writing continuously).
// Cap how long a tick may be postponed so refreshes keep flowing.
const MAX_DEBOUNCE_WAIT: Duration = Duration::from_secs(2);
const MAX_WORKTREE_DEBOUNCE_WAIT: Duration = Duration::from_secs(3);
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
    _watcher: Arc<SharedWorktreeWatch>,
}

type WatchMessage = std::result::Result<notify::Event, String>;
type Subscriber = (mpsc::SyncSender<WatchMessage>, Arc<AtomicBool>);
pub(crate) struct SharedWorktreeWatch {
    _watcher: Mutex<RecommendedWatcher>,
    subscribers: Arc<Mutex<Vec<Subscriber>>>,
}
pub(crate) struct WorktreeEvents {
    rx: mpsc::Receiver<WatchMessage>,
    overflow: Arc<AtomicBool>,
}
impl WorktreeEvents {
    pub(crate) fn recv(&self) -> Option<WatchMessage> {
        let event = self.rx.recv().ok()?;
        Some(if self.overflow.swap(false, Ordering::Relaxed) {
            Err("filesystem event overflow; reconcile workspace".into())
        } else {
            event
        })
    }
}

/// Git UI and agent tracking share the same OS subscription. Each consumer
/// debounces independently; a slow consumer receives an explicit rescan hint.
pub(crate) fn shared_worktree_events(
    root: &Path,
) -> Result<(Arc<SharedWorktreeWatch>, WorktreeEvents)> {
    static WATCHES: OnceLock<Mutex<HashMap<PathBuf, Weak<SharedWorktreeWatch>>>> = OnceLock::new();
    let root = std::fs::canonicalize(root)?;
    let mut watches = WATCHES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let shared = if let Some(watch) = watches.get(&root).and_then(Weak::upgrade) {
        watch
    } else {
        let subscribers: Arc<Mutex<Vec<Subscriber>>> = Arc::new(Mutex::new(Vec::new()));
        let listeners = subscribers.clone();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let event = event.map_err(|e| e.to_string());
                listeners.lock().unwrap_or_else(|e| e.into_inner()).retain(
                    |(tx, overflow)| match tx.try_send(event.clone()) {
                        Ok(()) => true,
                        Err(mpsc::TrySendError::Full(_)) => {
                            overflow.store(true, Ordering::Relaxed);
                            true
                        }
                        Err(mpsc::TrySendError::Disconnected(_)) => false,
                    },
                );
            })?;
        watcher.watch(&root, RecursiveMode::Recursive)?;
        let shared = Arc::new(SharedWorktreeWatch {
            _watcher: Mutex::new(watcher),
            subscribers,
        });
        watches.insert(root, Arc::downgrade(&shared));
        shared
    };
    let (tx, rx) = mpsc::sync_channel(64);
    let overflow = Arc::new(AtomicBool::new(false));
    shared
        .subscribers
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((tx, overflow.clone()));
    Ok((shared, WorktreeEvents { rx, overflow }))
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
        std::thread::spawn(move || debounce_ticks(raw_rx, tick_tx, DEBOUNCE, MAX_DEBOUNCE_WAIT));

        Ok((Self { _watcher: watcher }, tick_rx))
    }
}

/// Forwards one tick per burst of raw events: a tick fires after `quiet` with
/// no events, or after `max_wait` even if events keep streaming in.
fn debounce_ticks(
    raw_rx: mpsc::Receiver<()>,
    tick_tx: mpsc::Sender<()>,
    quiet: Duration,
    max_wait: Duration,
) {
    while raw_rx.recv().is_ok() {
        let deadline = std::time::Instant::now() + max_wait;
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            if raw_rx.recv_timeout(quiet.min(remaining)).is_err() {
                break;
            }
        }
        if tick_tx.send(()).is_err() {
            break;
        }
    }
}

impl WorktreeWatcher {
    pub fn new(repo_path: &Path) -> Result<(Self, mpsc::Receiver<()>)> {
        let (raw_tx, raw_rx) = mpsc::channel::<()>();
        let (tick_tx, tick_rx) = mpsc::channel::<()>();

        let (watcher, events) = shared_worktree_events(repo_path)?;
        std::thread::spawn(move || {
            while let Some(event) = events.recv() {
                if event
                    .as_ref()
                    .map_or(true, |event| is_relevant_worktree_event(event))
                {
                    if raw_tx.send(()).is_err() {
                        break;
                    }
                }
            }
        });

        std::thread::spawn(move || {
            debounce_ticks(
                raw_rx,
                tick_tx,
                WORKTREE_DEBOUNCE,
                MAX_WORKTREE_DEBOUNCE_WAIT,
            )
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
    fn emits_tick_on_worktree_change() {
        let dir = tempfile::tempdir().unwrap();
        let (_watcher, rx) = WorktreeWatcher::new(dir.path()).unwrap();

        std::fs::write(dir.path().join("live-change.rs"), "fn live() {}\n").unwrap();

        let tick = rx.recv_timeout(Duration::from_secs(5));
        assert!(tick.is_ok(), "expected a watcher tick after a file edit");
    }

    #[test]
    fn debounce_emits_despite_constant_events() {
        let (raw_tx, raw_rx) = mpsc::channel::<()>();
        let (tick_tx, tick_rx) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            debounce_ticks(
                raw_rx,
                tick_tx,
                Duration::from_millis(50),
                Duration::from_millis(150),
            )
        });

        // Events arrive faster than the quiet window for well past the cap —
        // without the max-wait deadline no tick would ever fire.
        let feeder = std::thread::spawn(move || {
            for _ in 0..60 {
                if raw_tx.send(()).is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });

        let tick = tick_rx.recv_timeout(Duration::from_millis(400));
        assert!(tick.is_ok(), "expected a capped tick during an event storm");
        feeder.join().unwrap();
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
