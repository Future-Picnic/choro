#![allow(dead_code, reason = "retained agent record update API")]

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, EventEmitter};
use ide_core::local_store::LocalStore;
use ide_core::{
    agents, AgentAccessMode, AgentChangedFile, AgentEffort, AgentKind, AgentModel, AgentRecord,
    AgentRuntimeKind, AgentStatus, AgentStoreFile, ProjectId, TaskRef,
};
use uuid::Uuid;

use super::agent_chat::FileChangeStat;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);

pub enum AgentRecordsEvent {
    Changed,
    SelectionChanged,
}

/// App-owned agent records persisted to local user config.
pub struct AgentRecords {
    records: Vec<AgentRecord>,
    selected: HashMap<ProjectId, Uuid>,
    save_scheduled: bool,
}

impl EventEmitter<AgentRecordsEvent> for AgentRecords {}

impl AgentRecords {
    pub fn load() -> Self {
        let legacy = AgentStoreFile::load().agents;
        let mut records = match LocalStore::open_default().and_then(|store| store.load_agents()) {
            Ok(records) => records,
            Err(error) => {
                eprintln!("failed to load local store agents; using agents.json: {error:#}");
                legacy
            }
        };
        for agent in &mut records {
            if agent.model == AgentModel::CodexDefault {
                agent.model = AgentModel::default_for(AgentKind::Codex);
                agent.effort = agent.model.normalize_effort(agent.effort);
            }
        }
        Self {
            records,
            selected: HashMap::new(),
            save_scheduled: false,
        }
    }

    pub fn records_for_project(&self, project: ProjectId) -> Vec<AgentRecord> {
        agents::agents_for_project(&self.records, project)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn all_records(&self) -> Vec<AgentRecord> {
        self.records.clone()
    }

    pub fn selected_agent_id(&self, project: ProjectId) -> Option<Uuid> {
        self.selected
            .get(&project)
            .copied()
            .filter(|id| {
                self.records
                    .iter()
                    .any(|agent| agent.project_id == project && agent.id == *id)
            })
            .or_else(|| {
                agents::agents_for_project(&self.records, project)
                    .first()
                    .map(|agent| agent.id)
            })
    }

    pub fn explicitly_selected_agent_id(&self, project: ProjectId) -> Option<Uuid> {
        self.selected.get(&project).copied().filter(|id| {
            self.records
                .iter()
                .any(|agent| agent.project_id == project && agent.id == *id)
        })
    }

    /// The agent the user actually opened in this app run. Unlike
    /// `selected_agent`, this never falls back to the most recently updated
    /// record and is therefore safe for filesystem/Git scope decisions.
    pub fn explicitly_selected_agent(&self, project: ProjectId) -> Option<AgentRecord> {
        let id = self.explicitly_selected_agent_id(project)?;
        self.agent(id).cloned()
    }

    pub fn selected_agent(&self, project: ProjectId) -> Option<AgentRecord> {
        let id = self.selected_agent_id(project)?;
        self.agent(id).cloned()
    }

    pub fn agent(&self, id: Uuid) -> Option<&AgentRecord> {
        self.records.iter().find(|agent| agent.id == id)
    }

    pub fn active_doc_implementor(
        &self,
        project: ProjectId,
        relative_doc_path: &std::path::Path,
        implementor_id: Uuid,
    ) -> Option<AgentRecord> {
        agents::active_doc_implementor(&self.records, project, relative_doc_path, implementor_id)
            .cloned()
    }

    /// Implementation agents for a doc, newest run first. Hidden doc
    /// assistants share the same source-doc field but are not implementation
    /// attempts and therefore never appear in this history.
    pub fn doc_implementors(
        &self,
        project: ProjectId,
        relative_doc_path: &std::path::Path,
    ) -> Vec<AgentRecord> {
        let mut matches = self
            .records
            .iter()
            .filter(|agent| {
                agent.project_id == project
                    && !agent.hidden_doc_assistant
                    && agent.source_doc.as_deref() == Some(relative_doc_path)
                    && (agent.started_at.is_some()
                        || agent.cli_session_id.is_some()
                        || agent.chat_session_id.is_some())
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_implementation_history(&mut matches);
        matches
    }

    pub fn select(&mut self, project: ProjectId, id: Uuid, cx: &mut Context<Self>) {
        self.selected.insert(project, id);
        cx.emit(AgentRecordsEvent::SelectionChanged);
        cx.notify();
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_agent(
        &mut self,
        project_id: ProjectId,
        project_path: PathBuf,
        repository_path: Option<PathBuf>,
        title: String,
        doc: String,
        provider: AgentKind,
        runtime: AgentRuntimeKind,
        model: AgentModel,
        effort: AgentEffort,
        access_mode: AgentAccessMode,
        linked_docs: Vec<PathBuf>,
        source_doc: Option<PathBuf>,
        linked_tasks: Vec<TaskRef>,
        source_task: Option<TaskRef>,
        status: AgentStatus,
        cx: &mut Context<Self>,
    ) -> Uuid {
        let mut agent = AgentRecord::new(
            project_id,
            project_path,
            title,
            doc,
            provider,
            model,
            effort,
            access_mode,
        );
        agent.repository_path = repository_path;
        agent.status = status;
        agent.runtime = runtime;
        agent.linked_docs = linked_docs;
        agent.source_doc = source_doc;
        agent.linked_tasks = linked_tasks;
        agent.source_task = source_task;
        let id = agent.id;
        self.records.push(agent);
        self.selected.insert(project_id, id);
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.emit(AgentRecordsEvent::SelectionChanged);
        cx.notify();
        id
    }

    pub fn update_doc(&mut self, id: Uuid, doc: String, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.doc == doc {
            return;
        }
        agent.doc = doc;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_notes(&mut self, id: Uuid, notes: String, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.notes == notes {
            return;
        }
        agent.notes = notes;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_effort(&mut self, id: Uuid, effort: AgentEffort, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.effort == effort {
            return;
        }
        agent.effort = effort;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_model_effort(
        &mut self,
        id: Uuid,
        model: AgentModel,
        effort: AgentEffort,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        let model = if model.belongs_to(agent.provider) {
            model
        } else {
            AgentModel::default_for(agent.provider)
        };
        let effort = agent.normalize_effort_for_model(model, effort);
        if agent.model == model && agent.effort == effort {
            return;
        }
        agent.model = model;
        agent.effort = effort;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_external_model(
        &mut self,
        id: Uuid,
        model_id: String,
        label: String,
        variants: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        agent.set_external_model(model_id, label, variants);
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    /// Mark an agent as a Solo: record its branch, the branch it forked from,
    /// and how much its lane gets prepared with. The lane itself is not
    /// materialized here — `set_lane_path` flips once the worktree exists.
    pub fn configure_solo(
        &mut self,
        id: Uuid,
        branch: String,
        base_branch: Option<String>,
        profile: ide_core::LaneProfile,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        agent.solo_branch = Some(branch);
        agent.solo_base_branch = base_branch;
        agent.solo_rejoined_branch = None;
        agent.lane_profile = Some(profile);
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    /// Where the Solo's work went home — set on Rejoin without overwriting the
    /// branch it originally forked from.
    pub fn set_solo_rejoined_branch(&mut self, id: Uuid, base: String, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.solo_rejoined_branch.as_deref() == Some(base.as_str()) {
            return;
        }
        agent.solo_rejoined_branch = Some(base);
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    /// `Some` when the lane folder is materialized on disk; `None` after a
    /// teardown. `runtime_path()` follows this.
    pub fn set_lane_path(&mut self, id: Uuid, lane_path: Option<PathBuf>, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.lane_path == lane_path {
            return;
        }
        agent.lane_path = lane_path;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_access_mode(
        &mut self,
        id: Uuid,
        access_mode: AgentAccessMode,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.access_mode == access_mode {
            return;
        }
        agent.access_mode = access_mode;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_linked_docs(
        &mut self,
        id: Uuid,
        linked_docs: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.linked_docs == linked_docs {
            return;
        }
        agent.linked_docs = linked_docs;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_linked_tasks(
        &mut self,
        id: Uuid,
        linked_tasks: Vec<TaskRef>,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.linked_tasks == linked_tasks {
            return;
        }
        agent.linked_tasks = linked_tasks;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn active_task_implementor(
        &self,
        project: ProjectId,
        task: &TaskRef,
    ) -> Option<AgentRecord> {
        self.task_implementors(project, task).into_iter().next()
    }

    /// Implementation agents for a task, newest run first.
    pub fn task_implementors(&self, project: ProjectId, task: &TaskRef) -> Vec<AgentRecord> {
        let mut matches = self
            .records
            .iter()
            .filter(|agent| {
                agent.project_id == project
                    && agent
                        .source_task
                        .as_ref()
                        .is_some_and(|source| source.same_issue(task))
                    && (agent.started_at.is_some()
                        || agent.cli_session_id.is_some()
                        || agent.chat_session_id.is_some())
            })
            .cloned()
            .collect::<Vec<_>>();
        sort_implementation_history(&mut matches);
        matches
    }

    pub fn update_ship_pr_branch(
        &mut self,
        id: Uuid,
        repo_path: PathBuf,
        branch: String,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.ship_pr_repo_path.as_ref() == Some(&repo_path)
            && agent.ship_pr_branch.as_deref() == Some(branch.as_str())
        {
            return;
        }
        agent.ship_pr_repo_path = Some(repo_path);
        agent.ship_pr_branch = Some(branch);
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    /// Returns whether the merge actually altered the known per-file stats —
    /// the caller-visible signal that the agent wrote something new.
    pub fn merge_changed_files(
        &mut self,
        id: Uuid,
        files: &[FileChangeStat],
        cx: &mut Context<Self>,
    ) -> bool {
        if files.is_empty() {
            return false;
        }
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return false;
        };

        let mut changed = false;
        for file in files {
            if file.path.as_os_str().is_empty() {
                continue;
            }
            if let Some(existing) = agent
                .changed_files
                .iter_mut()
                .find(|existing| existing.path == file.path)
            {
                if existing.additions != file.additions || existing.deletions != file.deletions {
                    existing.additions = file.additions;
                    existing.deletions = file.deletions;
                    changed = true;
                }
            } else {
                agent.changed_files.push(AgentChangedFile {
                    path: file.path.clone(),
                    additions: file.additions,
                    deletions: file.deletions,
                });
                changed = true;
            }
        }

        if !changed {
            return false;
        }
        agent.changed_files.sort_by(|a, b| a.path.cmp(&b.path));
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
        true
    }

    pub fn move_doc_reference(
        &mut self,
        project: ProjectId,
        previous: &std::path::Path,
        next: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for agent in self
            .records
            .iter_mut()
            .filter(|agent| agent.project_id == project)
        {
            let next_doc = move_doc_mentions(&agent.doc, previous, next);
            if next_doc != agent.doc {
                agent.doc = next_doc;
                agent.updated_at = agents::unix_now();
                changed = true;
            }
            if agent.source_doc.as_deref() == Some(previous) {
                agent.source_doc = Some(next.to_path_buf());
                agent.updated_at = agents::unix_now();
                changed = true;
            }
            let mut linked_changed = false;
            for linked in &mut agent.linked_docs {
                if linked == previous {
                    *linked = next.to_path_buf();
                    linked_changed = true;
                }
            }
            if linked_changed {
                agent.updated_at = agents::unix_now();
                changed = true;
            }
        }
        if changed {
            self.schedule_save(cx);
            cx.emit(AgentRecordsEvent::Changed);
            cx.notify();
        }
    }

    pub fn remove_doc_reference(
        &mut self,
        project: ProjectId,
        relative: &std::path::Path,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for agent in self
            .records
            .iter_mut()
            .filter(|agent| agent.project_id == project)
        {
            if agent.source_doc.as_deref() == Some(relative) {
                agent.source_doc = None;
                agent.updated_at = agents::unix_now();
                changed = true;
            }
            let original_len = agent.linked_docs.len();
            agent.linked_docs.retain(|path| path != relative);
            if agent.linked_docs.len() != original_len {
                agent.updated_at = agents::unix_now();
                changed = true;
            }
        }
        if changed {
            self.schedule_save(cx);
            cx.emit(AgentRecordsEvent::Changed);
            cx.notify();
        }
    }

    pub fn update_title(&mut self, id: Uuid, title: String, cx: &mut Context<Self>) {
        let title = title.trim().to_string();
        if title.is_empty() {
            return;
        }
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.title == title {
            return;
        }
        agent.title = title;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn update_status(&mut self, id: Uuid, status: AgentStatus, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.status == status {
            return;
        }
        agent.status = status;
        agent.updated_at = agents::unix_now();
        self.schedule_save(cx);
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn mark_started(
        &mut self,
        id: Uuid,
        cli_session_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        let now = agents::unix_now();
        agent.started_at = Some(now);
        agent.updated_at = now;
        agent.status = AgentStatus::InProgress;
        let has_session_id = cli_session_id.is_some();
        if has_session_id {
            agent.cli_session_id = cli_session_id;
        }
        if has_session_id {
            self.save_immediately();
        } else {
            self.schedule_save(cx);
        }
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    /// Permanently close this agent's verification lifecycle after the user
    /// declines verification. This is the hard gate; timeline markers remain
    /// presentation/history only.
    pub fn mark_verification_closed(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.verification_closed {
            return;
        }
        let now = agents::unix_now();
        agent.verification_closed = true;
        agent.updated_at = now;
        // A dismissal must survive the next app launch even if the process
        // exits before the normal debounced save runs.
        self.save_immediately();
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    /// Permanently close this agent's verification lifecycle after a
    /// verification reports every stated requirement met.
    pub fn mark_verification_completed(&mut self, id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.verification_completed_at.is_some() && agent.verification_closed {
            return;
        }
        let now = agents::unix_now();
        if agent.verification_completed_at.is_none() {
            agent.verification_completed_at = Some(now);
        }
        agent.verification_closed = true;
        agent.updated_at = now;
        // Both completion evidence and the hard gate must survive restart.
        self.save_immediately();
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn set_cli_session_id(&mut self, id: Uuid, cli_session_id: String, cx: &mut Context<Self>) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.cli_session_id.as_deref() == Some(cli_session_id.as_str()) {
            return;
        }
        agent.cli_session_id = Some(cli_session_id);
        agent.updated_at = agents::unix_now();
        self.save_immediately();
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    pub fn set_chat_session_id(
        &mut self,
        id: Uuid,
        chat_session_id: String,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.records.iter_mut().find(|agent| agent.id == id) else {
            return;
        };
        if agent.chat_session_id.as_deref() == Some(chat_session_id.as_str()) {
            return;
        }
        agent.chat_session_id = Some(chat_session_id);
        agent.updated_at = agents::unix_now();
        self.save_immediately();
        cx.emit(AgentRecordsEvent::Changed);
        cx.notify();
    }

    fn save_immediately(&mut self) {
        self.save_scheduled = false;
        let store = AgentStoreFile::new(self.records.clone());
        if let Err(error) = LocalStore::open_default()
            .and_then(|local_store| local_store.save_agents(&store.agents))
        {
            eprintln!("failed to save agents to local store: {error:#}");
        }
        if let Err(error) = store.save() {
            eprintln!("failed to save agents: {error:#}");
        }
    }

    /// Flush any debounced agent-record changes before application shutdown.
    pub fn save_now(&mut self) {
        self.save_immediately();
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let store = this
                .update(cx, |records, _| {
                    records.save_scheduled = false;
                    AgentStoreFile::new(records.records.clone())
                })
                .ok();
            if let Some(store) = store {
                cx.background_executor()
                    .spawn(async move {
                        if let Err(error) = LocalStore::open_default()
                            .and_then(|local_store| local_store.save_agents(&store.agents))
                        {
                            eprintln!("failed to save agents to local store: {error:#}");
                        }
                        if let Err(error) = store.save() {
                            eprintln!("failed to save agents: {error:#}");
                        }
                    })
                    .await;
            }
        })
        .detach();
    }
}

fn sort_implementation_history(agents: &mut [AgentRecord]) {
    agents.sort_by(|left, right| {
        right
            .created_at
            .cmp(&left.created_at)
            .then_with(|| right.started_at.cmp(&left.started_at))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| right.id.cmp(&left.id))
    });
}

fn move_doc_mentions(prompt: &str, previous: &std::path::Path, next: &std::path::Path) -> String {
    let previous = format!("@@{}", previous.to_string_lossy());
    let next = format!("@@{}", next.to_string_lossy());
    prompt.replace(&previous, &next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::IssueTrackerProvider;

    fn task(site_url: &str, issue_key: &str) -> TaskRef {
        TaskRef {
            provider: IssueTrackerProvider::Jira,
            site_url: site_url.to_string(),
            issue_id: "10001".to_string(),
            issue_key: issue_key.to_string(),
            issue_url: format!("{}/browse/{}", site_url.trim_end_matches('/'), issue_key),
            title: "Add task board".to_string(),
        }
    }

    #[test]
    fn explicit_selection_never_falls_back_to_the_newest_record() {
        let project = ProjectId(Uuid::new_v4());
        let mut older = AgentRecord::new(
            project,
            PathBuf::from("/tmp/app"),
            "Older",
            "Do it",
            AgentKind::Codex,
            AgentModel::CodexDefault,
            AgentEffort::Medium,
            AgentAccessMode::FullAccess,
        );
        older.updated_at = 1;
        let mut newest = older.clone();
        newest.id = Uuid::new_v4();
        newest.title = "Newest Solo".to_string();
        newest.updated_at = 2;
        newest.solo_branch = Some("solo/newest-00000000".to_string());
        let records = AgentRecords {
            records: vec![older, newest.clone()],
            selected: HashMap::new(),
            save_scheduled: false,
        };

        assert_eq!(
            records.selected_agent(project).map(|agent| agent.id),
            Some(newest.id)
        );
        assert!(records.explicitly_selected_agent(project).is_none());
    }

    #[test]
    fn active_task_implementor_matches_provider_site_and_issue_key() {
        let project = ProjectId(Uuid::new_v4());
        let mut agent = AgentRecord::new(
            project,
            PathBuf::from("/tmp/app"),
            "Implement APP-123",
            "Do it",
            AgentKind::Codex,
            AgentModel::CodexDefault,
            AgentEffort::Medium,
            AgentAccessMode::FullAccess,
        );
        agent.started_at = Some(1);
        agent.source_task = Some(task("https://example.atlassian.net/", "APP-123"));
        agent.linked_tasks = vec![agent.source_task.clone().unwrap()];
        let records = AgentRecords {
            records: vec![agent.clone()],
            selected: HashMap::new(),
            save_scheduled: false,
        };

        let matched = records
            .active_task_implementor(project, &task("https://example.atlassian.net", "app-123"))
            .expect("agent should match task");
        assert_eq!(matched.id, agent.id);
        assert!(records
            .active_task_implementor(project, &task("https://other.atlassian.net", "APP-123"))
            .is_none());
    }

    #[test]
    fn implementation_histories_are_newest_first_and_exclude_doc_assistants() {
        let project = ProjectId(Uuid::new_v4());
        let source_task = task("https://example.atlassian.net", "APP-123");
        let source_doc = PathBuf::from("choro_docs/implementation.md");
        let mut older = AgentRecord::new(
            project,
            PathBuf::from("/tmp/app"),
            "Older implementation",
            "Do it",
            AgentKind::Claude,
            AgentModel::ClaudeOpus,
            AgentEffort::High,
            AgentAccessMode::FullAccess,
        );
        older.created_at = 10;
        older.started_at = Some(10);
        older.source_task = Some(source_task.clone());
        older.source_doc = Some(source_doc.clone());

        let mut newest = older.clone();
        newest.id = Uuid::new_v4();
        newest.title = "Newest implementation".into();
        newest.created_at = 20;
        newest.started_at = Some(20);

        let mut assistant = newest.clone();
        assistant.id = Uuid::new_v4();
        assistant.created_at = 30;
        assistant.hidden_doc_assistant = true;
        assistant.source_task = None;

        let records = AgentRecords {
            records: vec![older.clone(), assistant, newest.clone()],
            selected: HashMap::new(),
            save_scheduled: false,
        };

        assert_eq!(
            records
                .task_implementors(project, &source_task)
                .into_iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>(),
            vec![newest.id, older.id]
        );
        assert_eq!(
            records
                .doc_implementors(project, &source_doc)
                .into_iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>(),
            vec![newest.id, older.id]
        );
    }

    #[test]
    fn moving_a_doc_reference_updates_mentions_in_the_stored_launch_prompt() {
        let prompt = "Read @@choro_docs/old-name.choro first.\n\nRelevant docs:\n- choro_docs/old-name.choro\n";
        assert_eq!(
            move_doc_mentions(
                prompt,
                std::path::Path::new("choro_docs/old-name.choro"),
                std::path::Path::new("choro_docs/new-name.choro"),
            ),
            "Read @@choro_docs/new-name.choro first.\n\nRelevant docs:\n- choro_docs/old-name.choro\n"
        );
    }
}
