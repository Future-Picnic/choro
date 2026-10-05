//! Saved conversation artifacts are independent of provider startup and chat pages.
use super::*;

pub(super) enum ArtifactLoad {
    Loading(Uuid, Option<ProposedPlan>),
    Loaded,
    Failed(String),
}

pub(crate) fn restore_saved_artifacts(
    session: &mut AgentChatSession,
    files: Option<ChangedFilesSummary>,
    plan: Option<ProposedPlan>,
) {
    if let Some(files) = files {
        // Persistence runs in the background. Never roll back an edit made
        // since this read began, including a newer empty (reverted) ledger.
        if files.ledger_revision >= session.changed_files.ledger_revision {
            session.changed_files = files;
        }
    }
    if let Some(plan) = plan {
        let replace = session.latest_plan.as_ref().is_none_or(|current| {
            plan.revision > current.revision
                || plan.revision == current.revision && plan.id == current.id
                    && current.implemented_at.is_none() && plan.implemented_at.is_some()
        });
        if replace {
            session.latest_plan = Some(plan);
        }
    }
}

/// Only new mutation receipts are folded into a delayed baseline. Chat rows
/// cannot replace the saved projection or infer ownership from dirty files.
pub(crate) fn fold_pending_artifact_updates(
    session: &mut AgentChatSession,
    files: &mut ChangedFilesSummary,
) -> Vec<ChangedFilesSummary> {
    let mut pending = Vec::new();
    for item in &mut session.timeline {
        if let AgentChatTimelineItem::ChangedFiles(receipt) = item {
            if receipt.receipt_identity().is_some_and(|identity| !files.applied_receipts.contains(&identity)) {
                receipt.ledger_revision = files.ledger_revision.max(session.changed_files.ledger_revision).saturating_add(1);
                files.merge_turn(receipt);
                pending.push(receipt.clone());
            }
        }
    }
    pending
}

impl AgentChatState {
    /// Viewing saved work must not start/resume a provider, load a transcript,
    /// clear a decision, or touch a composer draft.
    pub(crate) fn ensure_saved_artifacts(&mut self, agent: &AgentRecord, cx: &mut Context<Self>) {
        if self.artifact_loads.contains_key(&agent.id) {
            return;
        }
        let id = agent.id;
        let new_session = !self.sessions.contains_key(&id);
        let session = self.ensure_session(id, agent.title.clone(), cx);
        if new_session {
            session.chat_session_id = agent.chat_session_id.clone();
            session.cli_session_id = agent.cli_session_id.clone();
            session.hidden_from_notifications = agent.hidden_doc_assistant
                || agent.delegation.as_ref().is_some_and(|binding| binding.task_id.is_some());
        }
        let token = Uuid::new_v4();
        let initial_plan = self.sessions.get(&id).and_then(|session| session.latest_plan.clone());
        self.artifact_loads.insert(id, ArtifactLoad::Loading(token, initial_plan));
        let agent = agent.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let store = LocalStore::open_default()?;
                if agent.hidden_doc_assistant {
                    store.ensure_assistant_chat_agent(&agent)?;
                }
                Ok::<_, anyhow::Error>((
                    load_file_ledger_from_store(&store, id)?,
                    load_latest_plan_from_store(&store, id)?,
                ))
            }).await;
            let _ = this.update(cx, |state, cx| {
                state.finish_artifact_load(id, token, result, cx);
            });
        }).detach();
    }

    pub(super) fn finish_artifact_load(
        &mut self,
        id: Uuid,
        token: Uuid,
        result: anyhow::Result<(ChangedFilesSummary, Option<ProposedPlan>)>,
        cx: &mut Context<Self>,
    ) {
        let initial_plan = match self.artifact_loads.get(&id) {
            Some(ArtifactLoad::Loading(active, plan)) if *active == token => plan.clone(),
            _ => return,
        };
        match result {
            Ok((mut files, mut plan)) => {
                if let Some(session) = self.sessions.get_mut(&id) {
                    // New receipts may arrive while an unopened conversation's
                    // saved baseline is loading. Apply those updates on top of
                    // the baseline, rather than replacing either set of files.
                    let pending = fold_pending_artifact_updates(session, &mut files);
                    // A newly produced plan also wins over a delayed read,
                    // even if its revision started from an unloaded baseline.
                    let version = |plan: &ProposedPlan| (plan.id.clone(), plan.markdown.clone(), plan.revision, plan.implemented_at);
                    if session.latest_plan.as_ref().map(version) != initial_plan.as_ref().map(version) {
                        if let Some(current) = session.latest_plan.as_mut() {
                            let saved_revision = plan.as_ref().map_or(0, |saved| saved.revision);
                            if current.revision <= saved_revision {
                                current.revision = saved_revision.saturating_add(1);
                                if let Some(pending) = session.proposed_plan.as_mut().filter(|pending| pending.id == current.id) {
                                    pending.revision = current.revision;
                                }
                                upsert_timeline_proposed_plan(&mut session.timeline, current.clone());
                                persist_timeline_item(id, AgentChatTimelineItem::ProposedPlan(current.clone()), cx);
                            }
                        }
                        plan = None;
                    }
                    for receipt in pending {
                        persist_changed_files_turn(id, receipt, files.clone(), cx);
                    }
                    restore_saved_artifacts(session, Some(files), plan);
                }
                self.artifact_loads.insert(id, ArtifactLoad::Loaded);
            }
            Err(error) => {
                self.artifact_loads.insert(id, ArtifactLoad::Failed(
                    format!("Couldn't load saved Files and Plan: {error:#}"),
                ));
            }
        }
        self.publish_change(id, ChatChangeCategories::CONTENT, cx);
    }

    pub(crate) fn saved_artifacts_loading(&self, id: Uuid) -> bool {
        matches!(self.artifact_loads.get(&id), Some(ArtifactLoad::Loading(..)))
    }

    pub(crate) fn saved_artifacts_error(&self, id: Uuid) -> Option<&str> {
        match self.artifact_loads.get(&id) {
            Some(ArtifactLoad::Failed(error)) => Some(error),
            _ => None,
        }
    }

    pub(crate) fn retry_saved_artifacts(&mut self, agent: &AgentRecord, cx: &mut Context<Self>) {
        if matches!(self.artifact_loads.get(&agent.id), Some(ArtifactLoad::Failed(_))) {
            self.artifact_loads.remove(&agent.id);
            self.ensure_saved_artifacts(agent, cx);
            self.publish_change(agent.id, ChatChangeCategories::CONTENT, cx);
        }
    }
}
