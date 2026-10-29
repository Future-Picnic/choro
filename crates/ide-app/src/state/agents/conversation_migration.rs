use super::*;
use ide_core::agents::migration::PreparedConversation;
use ide_core::local_store::StoredChatMessage;

impl AgentRecords {
    /// Reserve identities on the UI thread so concurrent dialogs and subsequent
    /// snapshots see the same records. No provider is launched by migration.
    pub(crate) fn stage_conversation_migration(
        &mut self,
        conversations: Vec<PreparedConversation>,
        cx: &mut Context<Self>,
    ) -> Vec<PreparedConversation> {
        let mut staged = Vec::new();
        for conversation in conversations {
            let incoming = &conversation.agent;
            if self.records.iter().any(|agent| {
                agent.project_id == incoming.project_id
                    && agent.provider == incoming.provider
                    && (agent.cli_session_id == incoming.cli_session_id
                        || agent.chat_session_id == incoming.chat_session_id)
            }) {
                continue;
            }
            self.records.push(incoming.clone());
            self.publish_record_change(incoming.id, cx);
            staged.push(conversation);
        }
        if !staged.is_empty() {
            cx.notify();
        }
        staged
    }
}

pub(crate) fn persist_conversation_migration(
    revision: u64,
    snapshot: AgentStoreFile,
    messages: Vec<StoredChatMessage>,
) -> anyhow::Result<()> {
    let store = LocalStore::open_default()?;
    let mut saved = false;
    AGENT_WRITER.persist(revision, || {
        store.save_conversation_migration(Some(&snapshot.agents), &messages)?;
        saved = true;
        if let Err(error) = snapshot.save() {
            eprintln!("failed to save migration JSON backup: {error:#}");
        }
        Ok::<_, anyhow::Error>(())
    })?;
    if !saved {
        // A newer snapshot includes the reserved records. Never replay stale
        // metadata, but still finish copying their history before success.
        store.save_conversation_migration(None, &messages)?;
    }
    Ok(())
}
