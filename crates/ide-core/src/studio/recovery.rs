//! Remember only successfully committed, unchanged journals. History stays on
//! disk for undo; new or replaced files still go through full crash recovery.
use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[derive(Clone, PartialEq, Eq)]
struct Fingerprint {
    length: u64,
    modified: SystemTime,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}

fn fingerprint(path: &Path) -> Result<Fingerprint> {
    let metadata = fs::metadata(path)?;
    Ok(Fingerprint {
        length: metadata.len(),
        modified: metadata.modified()?,
        #[cfg(unix)]
        identity: {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        },
    })
}

type CommittedJournals = HashMap<PathBuf, HashMap<PathBuf, Fingerprint>>;
fn committed() -> &'static Mutex<CommittedJournals> {
    static CACHE: OnceLock<Mutex<CommittedJournals>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

impl StudioStore {
    /// Caller holds the cross-process project lock. Never hold the cache lock
    /// during file I/O, parsing, or recovery of another project's transaction.
    pub(super) fn recover_journals(&self) -> Result<usize> {
        let directory = self.cache.join("transactions");
        if !directory.exists() {
            return Ok(0);
        }
        let previous = committed()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&directory)
            .cloned()
            .unwrap_or_default();
        let mut next = HashMap::new();
        let mut parsed = 0;
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.extension().and_then(|v| v.to_str()) != Some("json") {
                continue;
            }
            let before = fingerprint(&path)?;
            if previous.get(&path) == Some(&before) {
                next.insert(path, before);
                continue;
            }
            let mut journal: Journal = serde_json::from_slice(&fs::read(&path)?)?;
            parsed += 1;
            if !journal.committed {
                self.finish_journal(&mut journal, &path)?;
                next.insert(path.clone(), fingerprint(&path)?);
            } else if fingerprint(&path)? == before {
                // Also detect a writer that does not honor the project lock.
                next.insert(path, before);
            }
        }
        committed()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(directory, next);
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_history_is_skipped_but_replaced_journals_are_recovered() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let store = StudioStore::new(&project, root.path().join("cache")).unwrap();
        let design = store.create("Test").unwrap();
        let directory = store.cache.join("transactions");
        fs::create_dir_all(&directory).unwrap();
        // A committed journal is an inert history entry. Turn it back into a
        // pending journal at the same path to model another process's write.
        let path = directory.join("history.json");
        let mut journal = Journal {
            request: StudioTransaction {
                id: Uuid::new_v4(),
                scope_id: Uuid::new_v4(),
                design_id: design.manifest.id,
                expected_revision: design.manifest.revision,
                expected_fingerprint: String::new(),
                operations: vec![],
            },
            before: design.clone(),
            after: design,
            writes: BTreeMap::new(),
            previous: BTreeMap::new(),
            committed: true,
            history: None,
        };
        atomic(&path, &serde_json::to_vec(&journal).unwrap()).unwrap();
        let _lock = store.lock().unwrap();
        assert_eq!(store.recover_journals().unwrap(), 1);
        assert_eq!(store.recover_journals().unwrap(), 0);
        let reopened = StudioStore::new(&project, root.path().join("cache")).unwrap();
        assert_eq!(reopened.recover_journals().unwrap(), 0);
        journal.committed = false;
        atomic(&path, &serde_json::to_vec(&journal).unwrap()).unwrap();
        assert_eq!(reopened.recover_journals().unwrap(), 1);
        let recovered: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert!(recovered.committed);
        assert_eq!(reopened.recover_journals().unwrap(), 0);
        atomic(&path, b"broken journal").unwrap();
        assert!(reopened.recover_journals().is_err());
    }
}
