use std::sync::Mutex;

/// Serialize full-store snapshots and never let delayed work replace a newer
/// successful save. A required save may only be skipped after a newer commit,
/// not merely because a newer snapshot has been queued.
pub(super) struct SnapshotWriter {
    committed: Mutex<u64>,
}

impl SnapshotWriter {
    pub const fn new() -> Self {
        Self {
            committed: Mutex::new(0),
        }
    }

    pub fn persist<E>(&self, revision: u64, save: impl FnOnce() -> Result<(), E>) -> Result<(), E> {
        let mut committed = self.committed.lock().unwrap_or_else(|e| e.into_inner());
        if revision <= *committed {
            return Ok(());
        }
        save()?;
        *committed = revision;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_launch_cannot_replace_a_newer_committed_snapshot() {
        let writer = SnapshotWriter::new();
        let mut chats = vec![];
        writer
            .persist(2, || {
                chats = vec!["first", "second"];
                Ok::<_, ()>(())
            })
            .unwrap();
        writer
            .persist(1, || {
                chats = vec!["first"];
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(chats, ["first", "second"]);
    }

    #[test]
    fn failed_newer_save_does_not_skip_required_launch_save_or_retry() {
        let writer = SnapshotWriter::new();
        assert!(writer.persist(2, || Err("disk full")).is_err());
        let mut chats = vec![];
        writer
            .persist(1, || {
                chats = vec!["first"];
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(chats, ["first"]);
        writer
            .persist(2, || {
                chats.push("second");
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(chats, ["first", "second"]);
    }

    #[test]
    fn delayed_launch_preserves_newer_chat_messages_in_turso() {
        use ide_core::local_store::LocalStore;
        use ide_core::{AgentAccessMode, AgentKind, AgentModel, AgentRecord, AppConfig, Project};

        let dir = tempfile::tempdir().unwrap();
        let store = LocalStore::open(dir.path().to_path_buf()).unwrap();
        let project = Project::from_path(dir.path().join("project"));
        let model = AgentModel::default_for(AgentKind::Codex);
        let first = AgentRecord::new(
            project.id,
            project.path.clone(),
            "First",
            "One",
            AgentKind::Codex,
            model,
            model.default_effort(),
            AgentAccessMode::FullAccess,
        );
        let second = AgentRecord::new(
            project.id,
            project.path.clone(),
            "Second",
            "Two",
            AgentKind::Codex,
            model,
            model.default_effort(),
            AgentAccessMode::FullAccess,
        );
        let mut config = AppConfig::default();
        config.projects.push(project);
        store.save_workspace_config(&config).unwrap();
        let writer = SnapshotWriter::new();
        writer
            .persist(2, || store.save_agents(&[first.clone(), second.clone()]))
            .unwrap();
        store
            .append_chat_message(second.id, "user", "Keep this conversation", 1, None)
            .unwrap();
        // Before the watermark check, this full-store save deleted `second`
        // and cascaded its transcript; the next message then failed its FK.
        writer.persist(1, || store.save_agents(&[first])).unwrap();
        store
            .append_chat_message(second.id, "assistant", "Still here", 2, None)
            .unwrap();
        assert_eq!(store.load_agents().unwrap().len(), 2);
        let messages = store.load_chat_messages(second.id).unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "Keep this conversation");
    }
}
