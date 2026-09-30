//! Shared status-only observation. No provider waits for these workers.
use super::*;
use std::{
    collections::{HashMap, HashSet},
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct ObserverHandle {
    tx: mpsc::SyncSender<()>,
    topology: Arc<std::sync::atomic::AtomicBool>,
}
fn observers() -> &'static Mutex<HashMap<PathBuf, ObserverHandle>> {
    static ROOTS: OnceLock<Mutex<HashMap<PathBuf, ObserverHandle>>> = OnceLock::new();
    ROOTS.get_or_init(Default::default)
}

pub fn refresh(root: &Path) {
    let root = super::working_directory(root);
    let mut roots = observers().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(handle) = roots.get(&root) {
        let _ = handle.tx.try_send(());
        return;
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let invalidated = Arc::new(std::sync::atomic::AtomicBool::new(true));
    roots.insert(
        root.to_path_buf(),
        ObserverHandle {
            tx: tx.clone(),
            topology: invalidated.clone(),
        },
    );
    let root = root.to_path_buf();
    std::thread::Builder::new()
        .name("choro-workspace-observer".into())
        .spawn(move || {
            let topology = invalidated.clone();
            let refresh_tx=tx.clone();
            let mut metadata_watches=HashMap::new();
            let _watcher =
                crate::watcher::shared_worktree_events(&root)
                    .ok()
                    .map(|(watcher, events)| {
                        std::thread::spawn(move || {
                            while let Some(event) = events.recv() {
                                if event.as_ref().map_or(true, |e| {
                                    e.paths
                                        .iter()
                                        .any(|p| p.file_name().is_some_and(|n| n == ".git"))
                                }) {
                                    topology.store(true, Ordering::Relaxed);
                                }
                                if tx.try_send(()).is_err_and(|e| {
                                    matches!(e, mpsc::TrySendError::Disconnected(_))
                                }) {
                                    break;
                                }
                            }
                        });
                        watcher
                    });
            let mut repositories = Vec::new();
            let mut previous = crate::local_store::LocalStore::open_default()
                .ok()
                .and_then(|s| s.load_workspace_change_snapshot(&root).ok().flatten())
                .unwrap_or_else(|| WorkspaceChangeSnapshot {
                    root: root.clone(),
                    ..Default::default()
                });
            loop {
                let started = now_micros();
                let _permit = crate::git::BackgroundGitPermit::acquire();
                if invalidated.swap(false, Ordering::Relaxed) || repositories.is_empty() {
                    repositories = crate::git::discover_repositories(&root);
                    if repositories.is_empty() {
                        repositories.push(
                            git2::Repository::discover(&root)
                                .ok()
                                .and_then(|r| r.workdir().map(Path::to_path_buf))
                                .unwrap_or_else(|| root.clone()),
                        );
                    }
                }
                for repository in &repositories {
                    if let Ok(repo)=git2::Repository::open(repository) {
                        let git_dir=repo.path().to_path_buf();
                        if !git_dir.starts_with(&root) && !metadata_watches.contains_key(&git_dir) {
                            if let Ok((watcher,events))=crate::watcher::shared_worktree_events(&git_dir) {
                                let tx=refresh_tx.clone();
                                std::thread::spawn(move || {while events.recv().is_some() {if tx.try_send(()).is_err_and(|e|matches!(e,mpsc::TrySendError::Disconnected(_))) {break;}}});
                                metadata_watches.insert(git_dir,watcher);
                            }
                        }
                    }
                }
                let result = status_entries(&root, &repositories);
                let mut next = previous.clone();
                next.revision = next.revision.saturating_add(1);
                match result {
                    Ok(statuses) => {
                        next.paths = statuses.keys().cloned().collect();
                        next.statuses = statuses;
                        next.complete = true;
                        next.error = None;
                        next.observed_at = started;
                    }
                    Err(error) => {
                        next.complete = false;
                        next.error = Some(format!("{error:#}"));
                    }
                }
                if let Ok(store) = crate::local_store::LocalStore::open_default() {
                    if next.complete {
                        let paths: HashSet<_> = next.paths.iter().collect();
                        let mut candidates=store.pending_workspace_change_paths(&root).unwrap_or_default();
                        candidates.extend(previous.paths.iter().cloned());candidates.sort();candidates.dedup();
                        let repos=repositories.iter().filter_map(|p|git2::Repository::open(p).ok()).collect::<Vec<_>>();
                        let clean=candidates.into_iter().filter(|p|!paths.contains(p)).filter(|p| {
                            let absolute=root.join(p);
                            let repo=repos.iter().filter(|r|r.workdir().is_some_and(|r|absolute.starts_with(r))).max_by_key(|r|r.workdir().unwrap().components().count());
                            let Some(repo)=repo else {return false;};
                            let Ok(local)=absolute.strip_prefix(repo.workdir().unwrap()) else {return false;};
                            // An ignored untracked file is absent from status, but
                            // its confirmed edit is still pending and historical.
                            std::fs::symlink_metadata(&absolute).is_err_and(|e|e.kind()==std::io::ErrorKind::NotFound) || repo.index().ok().is_some_and(|i|i.get_path(local,0).is_some()) || !repo.status_should_ignore(local).unwrap_or(true)
                        }).collect::<Vec<_>>();
                        if !clean.is_empty() {
                            let _ = super::settle_repository_paths(&store, &root, &clean, started);
                        }
                    }
                    if let Err(error) = store.save_workspace_change_snapshot(&next) {
                        eprintln!("workspace observation persistence: {error:#}");
                    }
                }
                super::trace("observation",serde_json::json!({"root":root,"paths":next.paths.len(),"elapsed_us":now_micros().saturating_sub(started),"complete":next.complete}));
                previous = next;
                drop(_permit);
                if rx.recv_timeout(Duration::from_secs(60)).is_err() && _watcher.is_none() {
                    invalidated.store(true, Ordering::Relaxed);
                }
                let deadline = Instant::now() + Duration::from_secs(3);
                while Instant::now() < deadline
                    && rx
                        .recv_timeout(
                            Duration::from_millis(750)
                                .min(deadline.saturating_duration_since(Instant::now())),
                        )
                        .is_ok()
                {}
            }
        })
        .expect("start workspace observer");
}

pub fn status_paths(root: &Path, repositories: &[PathBuf]) -> anyhow::Result<Vec<PathBuf>> {
    Ok(status_entries(root, repositories)?.into_keys().collect())
}

fn status_entries(
    root: &Path,
    repositories: &[PathBuf],
) -> anyhow::Result<std::collections::BTreeMap<PathBuf, String>> {
    let _permit = crate::git::BackgroundGitPermit::acquire();
    let started = now_micros();
    anyhow::ensure!(repositories.len() < 128, "repository discovery incomplete");
    let mut paths = std::collections::BTreeMap::new();
    for repo in repositories {
        let output = status_output(repo)?;
        for record in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            anyhow::ensure!(record.len() > 3, "invalid status record");
            #[cfg(unix)]
            let path = {
                use std::os::unix::ffi::OsStrExt;
                PathBuf::from(std::ffi::OsStr::from_bytes(&record[3..]))
            };
            #[cfg(not(unix))]
            let path = PathBuf::from(std::str::from_utf8(&record[3..])?);
            let absolute = repo.join(&path);
            if repositories
                .iter()
                .any(|r| r != repo && r.starts_with(repo) && absolute.starts_with(r))
            {
                continue;
            }
            if let Some(path) = relative_path(root, &absolute) {
                if !crate::git::is_internal_visualization_path(&path) {
                    paths.insert(path, String::from_utf8_lossy(&record[..2]).into_owned());
                }
            }
        }
    }
    super::trace(
        "status",
        serde_json::json!({"root":root,"paths":paths.len(),"elapsed_us":now_micros().saturating_sub(started),"content_bytes_captured":0}),
    );
    Ok(paths)
}

fn status_output(root: &Path) -> anyhow::Result<Vec<u8>> {
    const CAP: usize = 4 * 1024 * 1024;
    let mut child = Command::new("git")
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "core.fsmonitor=false",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut data = Vec::new();
        stdout
            .take((CAP + 1) as u64)
            .read_to_end(&mut data)
            .map(|_| data)
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = reader
        .join()
        .map_err(|_| anyhow::anyhow!("status reader failed"))??;
    anyhow::ensure!(
        status.is_some_and(|s| s.success()),
        "status unavailable or timed out"
    );
    anyhow::ensure!(output.len() <= CAP, "status output limit reached");
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn aliases_share_one_observer_and_status_refresh_does_not_rediscover() {
        let dir = tempfile::tempdir().unwrap();
        let actual = dir.path().join("actual");
        std::fs::create_dir(&actual).unwrap();
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let key = super::super::working_directory(&actual);
        let (tx, rx) = mpsc::sync_channel(1);
        let topology = Arc::new(std::sync::atomic::AtomicBool::new(false));
        observers().lock().unwrap().insert(
            key.clone(),
            ObserverHandle {
                tx,
                topology: topology.clone(),
            },
        );
        refresh(&alias);
        refresh(&actual.join("."));
        invalidate_existing(&alias);
        assert!(!topology.load(Ordering::Relaxed));
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
        rediscover(&alias);
        assert!(topology.load(Ordering::Relaxed));
        assert!(observers().lock().unwrap().contains_key(&key));
        assert!(!observers().lock().unwrap().contains_key(&alias));
        let separate = dir.path().join("separate-worktree");
        std::fs::create_dir(&separate).unwrap();
        assert_ne!(key, super::super::working_directory(&separate));
        observers().lock().unwrap().remove(&key);
    }

    #[test]
    fn status_handles_thousands_of_files_large_assets_ignores_and_nested_repositories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .arg(root)
            .status()
            .unwrap()
            .success());
        std::fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        std::fs::create_dir(root.join("ignored")).unwrap();
        std::fs::File::create(root.join("ignored/video.mov"))
            .unwrap()
            .set_len(3_000_000_000)
            .unwrap();
        std::fs::File::create(root.join("asset.zip"))
            .unwrap()
            .set_len(3_000_000_000)
            .unwrap();
        for i in 0..1442 {
            std::fs::write(root.join(format!("file-{i}.txt")), "text\n").unwrap();
        }
        std::fs::write(root.join("tab\tnewline\n.txt"), "text").unwrap();
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert!(Command::new("git")
            .args(["init", "-q"])
            .arg(&nested)
            .status()
            .unwrap()
            .success());
        std::fs::write(nested.join("other.rs"), "nested").unwrap();
        let paths = status_paths(root, &[root.to_path_buf(), nested]).unwrap();
        assert!(paths.contains(&PathBuf::from("asset.zip")));
        assert!(paths.contains(&PathBuf::from("nested/other.rs")));
        assert!(paths.contains(&PathBuf::from("tab\tnewline\n.txt")));
        assert!(!paths.iter().any(|p| p.starts_with("ignored")));
        assert_eq!(paths.len(), 1446);
    }
}

pub fn invalidate_existing(root: &Path) {
    if let Some(handle) = observers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&super::working_directory(root))
    {
        let _ = handle.tx.try_send(());
    }
}

/// Explicit topology invalidation is separate from ordinary status refreshes.
pub fn rediscover(root: &Path) {
    if let Some(handle) = observers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&super::working_directory(root))
    {
        handle.topology.store(true, Ordering::Relaxed);
        let _ = handle.tx.try_send(());
    }
}

pub(super) fn registered_roots() -> Vec<PathBuf> {
    observers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .keys()
        .cloned()
        .collect()
}
