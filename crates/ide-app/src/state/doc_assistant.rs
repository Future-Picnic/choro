use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui::{Context, EventEmitter};
use ide_core::doc_assistant;
use ide_core::{
    AgentAccessMode, AgentEffort, AgentKind, AgentModel, DocAssistantRecord, DocAssistantStoreFile,
    ProjectId,
};

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

pub enum DocAssistantEvent {
    Changed,
}

pub struct DocAssistantState {
    records: Vec<DocAssistantRecord>,
    save_scheduled: bool,
}

impl EventEmitter<DocAssistantEvent> for DocAssistantState {}

impl DocAssistantState {
    pub fn load() -> Self {
        let records = DocAssistantStoreFile::load().assistants;
        let store = DocAssistantStoreFile::new(records.clone());
        if let Err(error) = store.save() {
            eprintln!("failed to migrate doc assistants: {error:#}");
        }
        Self {
            records,
            save_scheduled: false,
        }
    }

    pub fn records(&self) -> &[DocAssistantRecord] {
        &self.records
    }

    pub fn record_for(
        &self,
        project: ProjectId,
        relative_doc_path: &Path,
    ) -> Option<DocAssistantRecord> {
        self.records
            .iter()
            .find(|record| {
                record.project_id == project && record.relative_doc_path == relative_doc_path
            })
            .cloned()
    }

    pub fn ensure_record(
        &mut self,
        project: ProjectId,
        relative_doc_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> DocAssistantRecord {
        if let Some(record) = self.record_for(project, &relative_doc_path) {
            return record;
        }
        let record = DocAssistantRecord::new(project, relative_doc_path);
        self.records.push(record.clone());
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
        record
    }

    /// Keep a database-backed specialized assistant (such as a Penpot design
    /// conversation) in the existing chat runtime without making the JSON file
    /// its source of truth.
    pub fn upsert_external_record(
        &mut self,
        record: DocAssistantRecord,
        cx: &mut Context<Self>,
    ) -> DocAssistantRecord {
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|value| value.key() == record.key())
        {
            if *existing != record {
                *existing = record.clone();
                self.schedule_save(cx);
            }
        } else {
            self.records.push(record.clone());
            self.schedule_save(cx);
        }
        cx.emit(DocAssistantEvent::Changed);
        record
    }

    pub fn reset_record(
        &mut self,
        project: ProjectId,
        relative_doc_path: PathBuf,
        cx: &mut Context<Self>,
    ) -> DocAssistantRecord {
        let record = DocAssistantRecord::new(project, relative_doc_path);
        if let Some(existing) = self
            .records
            .iter_mut()
            .find(|existing| existing.key() == record.key())
        {
            *existing = record.clone();
        } else {
            self.records.push(record.clone());
        }
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
        record
    }

    pub fn update_runtime(
        &mut self,
        project: ProjectId,
        relative_doc_path: &Path,
        provider: AgentKind,
        model: AgentModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        if self.record_for(project, relative_doc_path).is_none() {
            self.records.push(DocAssistantRecord::new(
                project,
                relative_doc_path.to_path_buf(),
            ));
        }
        let Some(record) = self.records.iter_mut().find(|record| {
            record.project_id == project && record.relative_doc_path == relative_doc_path
        }) else {
            return;
        };
        let model = if model.belongs_to(provider) {
            model
        } else {
            AgentModel::default_for(provider)
        };
        if record.provider == provider && record.model == model && record.effort == effort {
            return;
        }
        record.provider = provider;
        record.model = model;
        if provider != AgentKind::OpenCode {
            record.external_model_id = None;
            record.external_model_label = None;
            record.external_model_variants.clear();
        }
        record.effort = effort;
        record.updated_at = ide_core::agents::unix_now();
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn update_external_model(
        &mut self,
        project: ProjectId,
        relative_doc_path: &Path,
        model_id: String,
        model_label: String,
        model_variants: Vec<String>,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        if self.record_for(project, relative_doc_path).is_none() {
            self.records.push(DocAssistantRecord::new(
                project,
                relative_doc_path.to_path_buf(),
            ));
        }
        let Some(record) = self.records.iter_mut().find(|record| {
            record.project_id == project && record.relative_doc_path == relative_doc_path
        }) else {
            return;
        };
        let supported = AgentEffort::supported_variants(&model_variants);
        let effort = if supported.is_empty() || supported.contains(&effort) {
            effort
        } else if supported.contains(&AgentEffort::High) {
            AgentEffort::High
        } else {
            supported.first().copied().unwrap_or(AgentEffort::Medium)
        };
        if record.provider == AgentKind::OpenCode
            && record.model == AgentModel::OpenCode
            && record.external_model_id.as_deref() == Some(model_id.as_str())
            && record.external_model_label.as_deref() == Some(model_label.as_str())
            && record.external_model_variants == model_variants
            && record.effort == effort
        {
            return;
        }
        record.provider = AgentKind::OpenCode;
        record.model = AgentModel::OpenCode;
        record.external_model_id = Some(model_id);
        record.external_model_label = Some(model_label);
        record.external_model_variants = model_variants;
        record.effort = effort;
        record.updated_at = ide_core::agents::unix_now();
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn update_access_mode(
        &mut self,
        project: ProjectId,
        relative_doc_path: &Path,
        access_mode: AgentAccessMode,
        cx: &mut Context<Self>,
    ) {
        if self.record_for(project, relative_doc_path).is_none() {
            self.records.push(DocAssistantRecord::new(
                project,
                relative_doc_path.to_path_buf(),
            ));
        }
        let Some(record) = self.records.iter_mut().find(|record| {
            record.project_id == project && record.relative_doc_path == relative_doc_path
        }) else {
            return;
        };
        if record.access_mode == access_mode {
            return;
        }
        record.access_mode = access_mode;
        record.updated_at = ide_core::agents::unix_now();
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn set_cli_session_id(
        &mut self,
        key: &str,
        cli_session_id: String,
        transcript_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.records.iter_mut().find(|record| record.key() == key) else {
            return;
        };
        if record.cli_session_id.as_deref() == Some(cli_session_id.as_str())
            && record.last_transcript_path == transcript_path
        {
            return;
        }
        record.cli_session_id = Some(cli_session_id);
        record.last_transcript_path = transcript_path;
        record.updated_at = ide_core::agents::unix_now();
        self.save_immediately();
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn set_chat_session_ids(
        &mut self,
        key: &str,
        chat_session_id: Option<String>,
        cli_session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.records.iter_mut().find(|record| record.key() == key) else {
            return;
        };
        if record.chat_session_id == chat_session_id && record.cli_session_id == cli_session_id {
            return;
        }
        record.chat_session_id = chat_session_id;
        record.cli_session_id = cli_session_id;
        record.updated_at = ide_core::agents::unix_now();
        self.save_immediately();
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn move_doc_reference(
        &mut self,
        project: ProjectId,
        previous: &Path,
        next: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self
            .records
            .iter_mut()
            .find(|record| record.project_id == project && record.relative_doc_path == previous)
        else {
            return;
        };
        if record.relative_doc_path == next {
            return;
        }
        record.relative_doc_path = next;
        record.updated_at = ide_core::agents::unix_now();
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn remove_doc_reference(
        &mut self,
        project: ProjectId,
        relative_doc_path: &Path,
        cx: &mut Context<Self>,
    ) {
        let before = self.records.len();
        self.records.retain(|record| {
            !(record.project_id == project && record.relative_doc_path == relative_doc_path)
        });
        if self.records.len() == before {
            return;
        }
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn set_pending_proposal_by_key(
        &mut self,
        key: &str,
        proposal: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(record) = self.records.iter_mut().find(|record| record.key() == key) else {
            return;
        };
        if record.pending_proposal == proposal {
            return;
        }
        record.pending_proposal = proposal;
        record.updated_at = ide_core::agents::unix_now();
        self.schedule_save(cx);
        cx.emit(DocAssistantEvent::Changed);
        cx.notify();
    }

    pub fn key_for(project: ProjectId, relative_doc_path: &Path) -> String {
        doc_assistant::doc_assistant_key(project, relative_doc_path)
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let store = this
                .update(cx, |state, _| {
                    state.save_scheduled = false;
                    DocAssistantStoreFile::new(state.records.clone())
                })
                .ok();
            if let Some(store) = store {
                cx.background_executor()
                    .spawn(async move {
                        if let Err(error) = store.save() {
                            eprintln!("failed to save doc assistants: {error:#}");
                        }
                    })
                    .await;
            }
        })
        .detach();
    }

    fn save_immediately(&mut self) {
        self.save_scheduled = false;
        let store = DocAssistantStoreFile::new(self.records.clone());
        if let Err(error) = store.save() {
            eprintln!("failed to save doc assistants: {error:#}");
        }
    }
}
