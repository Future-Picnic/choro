//! Evidence belongs to an actor; a filesystem observation does not.
use serde::{Deserialize, Serialize};
use std::{
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

pub mod observer;
pub const MAX_CONTENT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PENDING_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PENDING_EVENTS: usize = 4096;

/// Shared by transport, adapter staging, and persistence. A reservation follows
/// its payload instead of releasing the limit when a channel is drained.
#[derive(Default)]
pub struct EvidenceBudget(Mutex<(usize, usize)>);
impl EvidenceBudget {
    pub fn global() -> &'static Arc<Self> {
        static B: OnceLock<Arc<EvidenceBudget>> = OnceLock::new();
        B.get_or_init(|| Arc::new(Self::default()))
    }
    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Option<EvidenceReservation> {
        let mut used = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if used.0.saturating_add(bytes) > MAX_PENDING_BYTES || used.1 >= MAX_PENDING_EVENTS {
            trace(
                "queue_overflow",
                serde_json::json!({"payload_bytes":bytes,"queued_bytes":used.0,"queued_events":used.1}),
            );
            return None;
        }
        used.0 += bytes;
        used.1 += 1;
        Some(EvidenceReservation {
            budget: self.clone(),
            bytes,
        })
    }
}
pub struct EvidenceReservation {
    budget: Arc<EvidenceBudget>,
    bytes: usize,
}
impl std::fmt::Debug for EvidenceReservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EvidenceReservation")
            .field(&self.bytes)
            .finish()
    }
}
impl Drop for EvidenceReservation {
    fn drop(&mut self) {
        let mut used = self.budget.0.lock().unwrap_or_else(|e| e.into_inner());
        used.0 -= self.bytes;
        used.1 -= 1;
    }
}

/// Adapter previews and multipart updates share the same payload budget.
pub struct PendingEvidence<T> {
    values: std::collections::HashMap<String, (T, EvidenceReservation)>,
}
impl<T> Default for PendingEvidence<T> {
    fn default() -> Self {
        Self {
            values: Default::default(),
        }
    }
}
impl<T> PendingEvidence<T> {
    pub fn insert(&mut self, id: String, value: T, bytes: usize) -> bool {
        self.values.remove(&id);
        let Some(reservation) =
            EvidenceBudget::global().reserve(bytes.saturating_add(id.len()).saturating_add(128))
        else {
            return false;
        };
        self.values.insert(id, (value, reservation));
        true
    }
    pub fn remove(&mut self, id: &str) -> Option<T> {
        self.values.remove(id).map(|(v, _)| v)
    }
    pub fn clear(&mut self) {
        self.values.clear();
    }
}
pub const AGENT_CHANGE_INSTRUCTIONS: &str = r#"When the task permits repository changes, use your dedicated file-editing tools (such as Edit, Write, or apply_patch) to create or modify file contents so Choro can attribute the edits to this conversation and include them in Files and code review. Do not use shell commands, Python scripts, sed, or output redirection to rewrite file contents just for convenience or batching. Use shell-based content editing only when the dedicated tools cannot perform the required change or the user explicitly requests that method; briefly tell the user which files were affected and that these edits may not appear in Choro's Files or review. Shell commands for reading, builds, tests, and copying files without changing their contents (such as cp) remain allowed within the task's permissions; plain copies do not need to appear in the conversation's changed-files list.

Before reverting, overwriting, or shipping existing dirty files, consult Choro get_agent_changes. Its confirmed own edits, other agents' contributions, and unattributed workspace changes are separate. A shared file can contain edits from several writers. Missing or stale evidence does not prove authorship or a clean workspace. Preserve unrelated changes."#;

/// Resolve directory aliases, never file paths (which may have been deleted).
pub fn working_directory(root: &Path) -> PathBuf {
    std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf())
}

pub fn now_micros() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

/// Opt-in local diagnostics; never includes source contents or sends telemetry.
pub fn trace(event: &str, values: serde_json::Value) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var_os("CHORO_CHANGE_METRICS").is_some()) {
        eprintln!("choro_changes {event} {values}");
    }
}

#[derive(Default)]
pub struct DispatchTiming {
    pub prompt: std::sync::atomic::AtomicU64,
    pub interrupt: std::sync::atomic::AtomicU64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangeKey {
    pub project_id: Uuid,
    pub root: PathBuf,
    pub agent_id: Uuid,
    pub generation: String,
    pub turn_id: String,
    pub action_id: String,
    pub path: PathBuf,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Patch,
    Contents,
    ConfirmedPath,
    Observation,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeReceiptState {
    #[default]
    Pending,
    Ready,
    Partial,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MutationEvidence {
    pub key: ChangeKey,
    pub kind: EvidenceKind,
    pub confirmed: bool,
    pub additions: Option<usize>,
    pub deletions: Option<usize>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub before: Option<String>,
    pub after: Option<String>,
    pub patch: Option<crate::git::FileDiff>,
    pub captured_at: u64,
}

impl MutationEvidence {
    pub fn id(&self) -> String {
        serde_json::to_string(&self.key).expect("serializable change key")
    }
    pub fn bound(&mut self) {
        if self
            .before
            .as_ref()
            .is_some_and(|s| s.len() > MAX_CONTENT_BYTES)
        {
            self.before = None;
        }
        if self
            .after
            .as_ref()
            .is_some_and(|s| s.len() > MAX_CONTENT_BYTES)
        {
            self.after = None;
        }
        if self.patch.as_ref().is_some_and(|p| {
            p.hunks
                .iter()
                .flat_map(|h| &h.lines)
                .map(|l| l.text.len())
                .sum::<usize>()
                > MAX_CONTENT_BYTES
        }) {
            self.patch = None;
        }
        if self.patch.is_none() && (self.before.is_none() || self.after.is_none()) {
            self.kind = if self.confirmed {
                EvidenceKind::ConfirmedPath
            } else {
                EvidenceKind::Observation
            };
            self.additions = None;
            self.deletions = None;
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChangeReceipt {
    pub agent_id: Uuid,
    pub generation: String,
    pub turn_id: String,
    pub state: ChangeReceiptState,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WorkspaceChangeSnapshot {
    pub root: PathBuf,
    pub revision: u64,
    pub observed_at: u64,
    pub complete: bool,
    pub error: Option<String>,
    pub paths: Vec<PathBuf>,
    /// Git porcelain index/worktree status, kept separate from authorship.
    #[serde(default)]
    pub statuses: std::collections::BTreeMap<PathBuf, String>,
}

/// Normalize without following file symlinks (including deleted paths).
pub fn relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    let path = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Normal(p) => result.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!result.as_os_str().is_empty()).then_some(result)
}

type Work = Box<dyn FnOnce(anyhow::Result<crate::local_store::LocalStore>) + Send>;
struct Job {
    bytes: usize,
    work: Work,
    _reservation: EvidenceReservation,
}
pub struct ChangeTracker {
    tx: mpsc::SyncSender<Job>,
    bytes: Arc<AtomicUsize>,
    incomplete: mpsc::SyncSender<ChangeReceipt>,
    incomplete_overflow: Arc<std::sync::atomic::AtomicBool>,
}

impl ChangeTracker {
    pub fn global() -> &'static Arc<Self> {
        static TRACKER: OnceLock<Arc<ChangeTracker>> = OnceLock::new();
        TRACKER.get_or_init(|| Self::start(crate::local_store::LocalStore::open_default))
    }

    pub fn with_store(store: crate::local_store::LocalStore) -> Arc<Self> {
        Self::start(move || Ok(store.clone()))
    }

    fn start(
        store: impl Fn() -> anyhow::Result<crate::local_store::LocalStore> + Send + Sync + 'static,
    ) -> Arc<Self> {
        let store = Arc::new(store);
        let factory = store.clone();
        let (incomplete, incomplete_rx) = mpsc::sync_channel::<ChangeReceipt>(MAX_PENDING_EVENTS);
        let incomplete_overflow = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let overflow = incomplete_overflow.clone();
        let (tx, rx) = mpsc::sync_channel::<Job>(MAX_PENDING_EVENTS);
        let bytes = Arc::new(AtomicUsize::new(0));
        let queued = bytes.clone();
        std::thread::Builder::new()
            .name("choro-change-evidence".into())
            .spawn(move || {
                loop {
                    if overflow.swap(false, Ordering::Relaxed) {
                        if factory().and_then(|s| s.mark_change_tracking_overflow()).is_err() { overflow.store(true, Ordering::Relaxed); }
                    }
                    for receipt in incomplete_rx.try_iter() {
                        if let Err(error) = factory().and_then(|s| s.save_change_receipt(&receipt)) { eprintln!("incomplete change receipt: {error:#}"); }
                    }
                    let job = match rx.recv_timeout(std::time::Duration::from_millis(50)) {
                        Ok(job) => job,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let started=now_micros();
                    (job.work)(factory());
                    queued.fetch_sub(job.bytes, Ordering::Relaxed);
                    trace("persist",serde_json::json!({"payload_bytes":job.bytes,"queued_bytes":queued.load(Ordering::Relaxed),"elapsed_us":now_micros().saturating_sub(started)}));
                }
            })
            .expect("start change tracker");
        Arc::new(Self {
            tx,
            bytes,
            incomplete,
            incomplete_overflow,
        })
    }

    pub fn record_incomplete(&self, receipt: ChangeReceipt) {
        // Fixed queue on the existing worker; overload never spawns threads.
        // The UI also receives Partial synchronously from the caller.
        if self.incomplete.try_send(receipt).is_err() {
            self.incomplete_overflow.store(true, Ordering::Relaxed);
            trace(
                "incomplete_receipt_queue_full",
                serde_json::json!({"complete":false}),
            );
        }
    }

    /// Never backpressure a provider. The caller must surface incomplete capture
    /// if this fails. Terminal callbacks may use a separate lightweight worker.
    pub fn enqueue(
        &self,
        bytes: usize,
        work: impl FnOnce(anyhow::Result<crate::local_store::LocalStore>) + Send + 'static,
    ) -> bool {
        let Some(reservation) = EvidenceBudget::global().reserve(bytes) else {
            return false;
        };
        if self
            .bytes
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                n.checked_add(bytes).filter(|n| *n <= MAX_PENDING_BYTES)
            })
            .is_err()
        {
            trace(
                "queue_overflow",
                serde_json::json!({"payload_bytes":bytes,"queued_bytes":self.bytes.load(Ordering::Relaxed)}),
            );
            return false;
        }
        if self
            .tx
            .try_send(Job {
                bytes,
                work: Box::new(work),
                _reservation: reservation,
            })
            .is_err()
        {
            self.bytes.fetch_sub(bytes, Ordering::Relaxed);
            trace(
                "queue_overflow",
                serde_json::json!({"payload_bytes":bytes,"queued_bytes":self.bytes.load(Ordering::Relaxed)}),
            );
            return false;
        }
        true
    }
}

/// Ship operations share this lock even when opened from different chats.
pub fn repository_operation_lock(root: &Path) -> Arc<Mutex<()>> {
    static LOCKS: OnceLock<Mutex<std::collections::HashMap<PathBuf, Arc<Mutex<()>>>>> =
        OnceLock::new();
    let root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(root)
        .or_default()
        .clone()
}

static PENDING_REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub fn pending_revision() -> u64 {
    PENDING_REVISION.load(Ordering::Acquire)
}

type PendingPaths = std::collections::HashMap<(Uuid, PathBuf), std::collections::HashSet<PathBuf>>;
fn pending_cache() -> &'static Mutex<PendingPaths> {
    static P: OnceLock<Mutex<PendingPaths>> = OnceLock::new();
    P.get_or_init(Default::default)
}
pub fn cached_pending_paths(
    agent: Uuid,
    root: &Path,
) -> Option<std::collections::HashSet<PathBuf>> {
    pending_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&(agent, root.to_path_buf()))
        .cloned()
}
pub fn cached_pending_repository_paths(
    agent: Uuid,
    repo: &Path,
) -> Option<std::collections::HashSet<PathBuf>> {
    let cache = pending_cache().lock().unwrap_or_else(|e| e.into_inner());
    let relevant = cache
        .iter()
        .filter(|((id, root), _)| {
            *id == agent && (repo.starts_with(root) || root.starts_with(repo))
        })
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return None;
    }
    Some(
        relevant
            .into_iter()
            .flat_map(|((_, root), paths)| {
                paths
                    .iter()
                    .filter_map(|path| relative_path(repo, &root.join(path)))
            })
            .collect(),
    )
}
pub fn refresh_pending_paths(
    store: &crate::local_store::LocalStore,
    agent: Uuid,
    root: &Path,
) -> anyhow::Result<()> {
    let paths = store.pending_agent_change_paths(agent, root)?;
    let previous = pending_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((agent, root.to_path_buf()), paths.clone());
    if previous.as_ref() != Some(&paths) {
        PENDING_REVISION.fetch_add(1, Ordering::Release);
    }
    Ok(())
}
pub fn settle_repository_paths(
    store: &crate::local_store::LocalStore,
    repo: &Path,
    paths: &[PathBuf],
    through_time: u64,
) -> anyhow::Result<()> {
    let keys = pending_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    let mut roots = store.change_scope_roots()?;
    roots.extend(observer::registered_roots());
    roots.extend(keys.iter().map(|(_, root)| root.clone()));
    roots.sort();
    roots.dedup();
    let actual_repo = working_directory(repo);
    for root in roots {
        let actual_root = working_directory(&root);
        let paths = paths
            .iter()
            .filter_map(|p| relative_path(&actual_root, &actual_repo.join(p)))
            .collect::<Vec<_>>();
        if !paths.is_empty() {
            store.settle_change_paths(&root, &paths, through_time)?;
        }
    }
    for (agent, root) in keys {
        refresh_pending_paths(store, agent, &root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn payload_reservations_cover_all_staging_until_dropped() {
        let budget = Arc::new(EvidenceBudget::default());
        let retained = budget.reserve(MAX_PENDING_BYTES).unwrap();
        assert!(budget.reserve(1).is_none());
        drop(retained);
        let events = (0..MAX_PENDING_EVENTS)
            .map(|_| budget.reserve(0).unwrap())
            .collect::<Vec<_>>();
        assert!(budget.reserve(0).is_none());
        drop(events);
        assert!(budget.reserve(MAX_PENDING_BYTES).is_some());
    }

    #[test]
    fn bounded_queue_rejects_payload_without_waiting_for_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::local_store::LocalStore::open(dir.path().join("data")).unwrap();
        let tracker = ChangeTracker::with_store(store);
        let (tx, rx) = mpsc::channel();
        assert!(tracker.enqueue(MAX_PENDING_BYTES, move |_| {
            let _ = rx.recv();
        }));
        assert!(!tracker.enqueue(1, |_| panic!("over-budget job ran")));
        tx.send(()).unwrap();
    }
    #[test]
    fn paths_cannot_escape_the_bound_worktree() {
        assert_eq!(
            relative_path(Path::new("/repo"), Path::new("/repo/a.rs")),
            Some("a.rs".into())
        );
        for path in ["../other.rs", "/other/a.rs", "a/../../b"] {
            assert!(relative_path(Path::new("/repo"), Path::new(path)).is_none());
        }
    }
}
