use super::AttentionKey;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, OnceLock};

type Entries = Vec<(AttentionKey, String)>;
enum Request {
    Save,
    Flush(mpsc::Sender<Result<(), String>>),
}
struct Writer {
    pending: Arc<Mutex<Option<Entries>>>,
    wake: mpsc::SyncSender<Request>,
}
static WRITER: OnceLock<Writer> = OnceLock::new();

fn read(path: &std::path::Path) -> anyhow::Result<Entries> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(error) => Err(error.into()),
    }
}

fn start(path: PathBuf) -> Writer {
    let pending = Arc::new(Mutex::new(None::<Entries>));
    let work = pending.clone();
    let (wake, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("choro-attention-save".into())
        .spawn(move || {
            let mut retry = None;
            loop {
                let request = match receiver.recv_timeout(std::time::Duration::from_secs(5)) {
                    Ok(request) => Some(request),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let snapshot = work
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .or_else(|| retry.take());
                let result = if let Some(snapshot) = snapshot {
                    let result = serde_json::to_vec(&snapshot)
                        .map_err(anyhow::Error::from)
                        .and_then(|bytes| ide_core::studio::atomic(&path, &bytes));
                    if result.is_err() {
                        retry = Some(snapshot);
                    } else {
                        retry = None;
                    }
                    result.map_err(|e| format!("Could not save attention history: {e:#}"))
                } else {
                    Ok(())
                };
                if let Some(Request::Flush(reply)) = request {
                    let _ = reply.send(result.clone());
                }
                if let Err(error) = result {
                    eprintln!("{error}");
                }
            }
        })
        .expect("start attention persistence worker");
    Writer { pending, wake }
}

pub(super) fn initialize() -> Entries {
    let path = ide_core::AppConfig::config_root().join("attention-history.json");
    let entries = read(&path).unwrap_or_else(|error| {
        eprintln!("Could not restore attention history: {error:#}");
        vec![]
    });
    WRITER.get_or_init(|| start(path));
    entries
}

pub(super) fn save(entries: Entries) {
    // Unit tests never initialize the production writer or touch live state.
    if let Some(writer) = WRITER.get() {
        *writer.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(entries);
        let _ = writer.wake.try_send(Request::Save);
    }
}

fn flush_writer(writer: &Writer) -> anyhow::Result<()> {
    let (reply, receiver) = mpsc::channel();
    writer.wake.send(Request::Flush(reply))?;
    receiver.recv()?.map_err(anyhow::Error::msg)
}

pub(super) fn flush() -> anyhow::Result<()> {
    if let Some(writer) = WRITER.get() {
        flush_writer(writer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::{AttentionCategory, AttentionEvent};
    #[test]
    fn latest_acknowledgements_survive_atomic_save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let writer = start(path.clone());
        let event = AttentionEvent::new(
            ide_core::ProjectId(uuid::Uuid::new_v4()),
            uuid::Uuid::new_v4(),
            AttentionCategory::Completed,
            "old",
            1,
            "Chat",
            "Project",
        );
        *writer.pending.lock().unwrap() = Some(vec![(event.key.clone(), "old".into())]);
        writer.wake.try_send(Request::Save).unwrap();
        let latest = vec![(event.key, "new".into())];
        *writer.pending.lock().unwrap() = Some(latest.clone());
        flush_writer(&writer).unwrap();
        assert_eq!(read(&path).unwrap(), latest);
    }
}
