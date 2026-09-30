use std::collections::{HashMap, HashSet};

use gpui::{App, AppContext, Context, Entity, EventEmitter};
use ide_core::local_store::LocalStore;
use ide_core::{
    IssueTrackerProvider, PersonalTaskPriority, PersonalTaskRecord, PersonalTaskStatus, ProjectId,
    TaskBoard, TaskBoardColumn, TaskDetail, TaskRef, TaskSummary, TaskTrackerClient,
    TaskTrackerConnection,
};
use uuid::Uuid;

use crate::state::Workspace;

pub enum TasksEvent {
    Changed,
    SelectionChanged,
}

/// One open task in the cross-project "My Tasks" view.
#[derive(Clone)]
pub struct MyTaskEntry {
    pub project: ProjectId,
    pub project_name: String,
    pub summary: TaskSummary,
}

pub struct TasksState {
    workspace: Entity<Workspace>,
    boards: HashMap<ProjectId, TaskBoard>,
    board_loading: HashSet<ProjectId>,
    board_errors: HashMap<ProjectId, String>,
    board_request_versions: HashMap<ProjectId, u64>,
    task_tracker_connections: HashMap<ProjectId, Vec<TaskTrackerConnection>>,
    selected_connection: HashMap<ProjectId, uuid::Uuid>,
    selected: HashMap<ProjectId, TaskRef>,
    details: HashMap<String, TaskDetail>,
    detail_loading: HashSet<String>,
    detail_errors: HashMap<String, String>,
    my_tasks: Vec<MyTaskEntry>,
    my_tasks_loading: bool,
    my_tasks_error: Option<String>,
    personal_save_versions: HashMap<Uuid, u64>,
}

impl EventEmitter<TasksEvent> for TasksState {}

impl TasksState {
    pub fn view(workspace: Entity<Workspace>, cx: &mut App) -> Entity<Self> {
        let task_tracker_connections = workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| (project.id, project.task_tracker_connections.clone()))
            .collect();
        cx.new(move |cx| {
            cx.observe(&workspace, |this: &mut Self, _, cx| {
                this.retain_projects(cx);
                this.reload_changed_task_tracker_connections(cx);
                cx.notify();
            })
            .detach();
            Self {
                workspace,
                boards: HashMap::new(),
                board_loading: HashSet::new(),
                board_errors: HashMap::new(),
                board_request_versions: HashMap::new(),
                task_tracker_connections,
                selected_connection: HashMap::new(),
                selected: HashMap::new(),
                details: HashMap::new(),
                detail_loading: HashSet::new(),
                detail_errors: HashMap::new(),
                my_tasks: Vec::new(),
                my_tasks_loading: false,
                my_tasks_error: None,
                personal_save_versions: HashMap::new(),
            }
        })
    }

    pub fn my_tasks(&self) -> Vec<MyTaskEntry> {
        self.my_tasks.clone()
    }

    pub fn my_tasks_loading(&self) -> bool {
        self.my_tasks_loading
    }

    pub fn my_tasks_error(&self) -> Option<String> {
        self.my_tasks_error.clone()
    }

    /// Load open (to-do + in-progress) tasks across every project's configured sources.
    pub fn refresh_my_tasks(&mut self, cx: &mut Context<Self>) {
        if self.my_tasks_loading {
            return;
        }
        let jobs: Vec<(ProjectId, String, TaskTrackerConnection)> = {
            let workspace = self.workspace.read(cx);
            workspace
                .projects
                .iter()
                .flat_map(|project| {
                    let mut connections = vec![personal_board_connection(project.id)];
                    connections.extend(
                        project
                            .task_tracker_connections
                            .iter()
                            .filter(|connection| connection.has_selected_source())
                            .cloned(),
                    );
                    let project_id = project.id;
                    let name = project.name.clone();
                    connections
                        .into_iter()
                        .map(move |connection| (project_id, name.clone(), connection))
                })
                .collect()
        };

        if jobs.is_empty() {
            self.my_tasks = Vec::new();
            self.my_tasks_error = None;
            self.my_tasks_loading = false;
            cx.emit(TasksEvent::Changed);
            cx.notify();
            return;
        }

        self.my_tasks_loading = true;
        self.my_tasks_error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (entries, errors) = cx
                .background_executor()
                .spawn(async move {
                    let mut entries: Vec<MyTaskEntry> = Vec::new();
                    let mut errors: Vec<String> = Vec::new();
                    for (project, project_name, connection) in jobs {
                        let board = if connection.provider == IssueTrackerProvider::Personal {
                            LocalStore::open_default()
                                .and_then(|store| store.load_personal_tasks(project))
                                .map(|tasks| personal_board(project, tasks))
                        } else {
                            TaskTrackerClient::new(connection)
                                .and_then(|client| client.load_board())
                        };
                        match board {
                            Ok(board) => {
                                for summary in board.issues {
                                    if is_open_status(&summary) {
                                        entries.push(MyTaskEntry {
                                            project,
                                            project_name: project_name.clone(),
                                            summary,
                                        });
                                    }
                                }
                            }
                            Err(error) => errors.push(format!("{project_name}: {error:#}")),
                        }
                    }
                    (entries, errors)
                })
                .await;
            this.update(cx, |this, cx| {
                this.my_tasks_loading = false;
                this.my_tasks = entries;
                this.my_tasks_error = if errors.is_empty() {
                    None
                } else {
                    Some(errors.join("\n"))
                };
                cx.emit(TasksEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn board(&self, project: ProjectId) -> Option<TaskBoard> {
        self.boards.get(&project).cloned()
    }

    pub fn board_error(&self, project: ProjectId) -> Option<String> {
        self.board_errors.get(&project).cloned()
    }

    pub fn board_loading(&self, project: ProjectId) -> bool {
        self.board_loading.contains(&project)
    }

    pub fn detail(&self, reference: &TaskRef) -> Option<TaskDetail> {
        self.details.get(&task_cache_key(reference)).cloned()
    }

    pub fn detail_error(&self, reference: &TaskRef) -> Option<String> {
        self.detail_errors.get(&task_cache_key(reference)).cloned()
    }

    pub fn detail_loading(&self, reference: &TaskRef) -> bool {
        self.detail_loading.contains(&task_cache_key(reference))
    }

    pub fn selected_ref(&self, project: ProjectId) -> Option<TaskRef> {
        self.selected.get(&project).cloned().or_else(|| {
            self.boards
                .get(&project)
                .and_then(|board| board.issues.first())
                .map(|task| task.reference.clone())
        })
    }

    pub fn selected_summary(&self, project: ProjectId) -> Option<TaskSummary> {
        let selected = self.selected_ref(project)?;
        self.boards
            .get(&project)?
            .issues
            .iter()
            .find(|task| task.reference.same_issue(&selected))
            .cloned()
    }

    pub fn active_connection_id(&self, project: ProjectId, cx: &App) -> Option<uuid::Uuid> {
        self.configured_connection(project, cx)
            .map(|connection| connection.id)
    }

    pub fn connections_for_project(
        &self,
        project: ProjectId,
        cx: &App,
    ) -> Vec<TaskTrackerConnection> {
        let mut connections = vec![personal_board_connection(project)];
        connections.extend(
            self.workspace
                .read(cx)
                .projects
                .iter()
                .find(|item| item.id == project)
                .map(|project| project.task_tracker_connections.clone())
                .unwrap_or_default(),
        );
        connections
    }

    pub fn configured_connection(
        &self,
        project: ProjectId,
        cx: &App,
    ) -> Option<TaskTrackerConnection> {
        let mut connections = vec![personal_board_connection(project)];
        connections.extend(
            self.workspace
                .read(cx)
                .projects
                .iter()
                .find(|item| item.id == project)?
                .task_tracker_connections
                .clone(),
        );
        if let Some(selected) = self.selected_connection.get(&project) {
            if let Some(connection) = connections
                .iter()
                .find(|connection| connection.id == *selected)
                .cloned()
            {
                return Some(connection);
            }
        }
        connections
            .iter()
            .find(|connection| connection.provider == IssueTrackerProvider::PocketComet)
            .cloned()
            .or_else(|| {
                connections
                    .iter()
                    .find(|connection| {
                        connection.provider == IssueTrackerProvider::Personal
                            || connection.has_selected_source()
                    })
                    .cloned()
            })
    }

    /// Find the connection in `project` that owns `reference` (matched by
    /// provider + site), so opening a cross-project task selects the right source.
    pub fn connection_for_ref(
        &self,
        project: ProjectId,
        reference: &TaskRef,
        cx: &App,
    ) -> Option<uuid::Uuid> {
        self.connection_object_for_ref(project, reference, cx)
            .map(|connection| connection.id)
    }

    /// The connection object (including credentials + settings) that owns a
    /// task. Matched by provider — a task URL's host doesn't line up with the
    /// stored connection URL for every provider (Linear/ClickUp store an API
    /// endpoint, while the task URL is the app/workspace URL) — with `site_url`
    /// only breaking ties when a project has several connections of one provider.
    pub fn connection_object_for_ref(
        &self,
        project: ProjectId,
        reference: &TaskRef,
        cx: &App,
    ) -> Option<TaskTrackerConnection> {
        let normalize = |value: &str| value.trim().trim_end_matches('/').to_ascii_lowercase();
        let same_provider: Vec<TaskTrackerConnection> = self
            .connections_for_project(project, cx)
            .into_iter()
            .filter(|connection| connection.provider == reference.provider)
            .collect();
        if same_provider.len() <= 1 {
            return same_provider.into_iter().next();
        }
        same_provider
            .iter()
            .find(|connection| normalize(&connection.site_url) == normalize(&reference.site_url))
            .cloned()
            .or_else(|| same_provider.into_iter().next())
    }

    pub fn select_connection(
        &mut self,
        project: ProjectId,
        connection_id: uuid::Uuid,
        cx: &mut Context<Self>,
    ) {
        if self.selected_connection.get(&project) == Some(&connection_id) {
            return;
        }
        self.selected_connection.insert(project, connection_id);
        self.boards.remove(&project);
        self.board_errors.remove(&project);
        self.selected.remove(&project);
        cx.emit(TasksEvent::SelectionChanged);
        cx.notify();
    }

    pub fn select_task(&mut self, project: ProjectId, reference: TaskRef, cx: &mut Context<Self>) {
        if self
            .selected
            .get(&project)
            .is_some_and(|current| current.same_issue(&reference))
        {
            return;
        }
        self.selected.insert(project, reference);
        cx.emit(TasksEvent::SelectionChanged);
        cx.notify();
    }

    pub fn refresh_project(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        if self.board_loading.contains(&project) {
            return;
        }
        let Some(connection) = self.configured_connection(project, cx) else {
            self.boards.remove(&project);
            self.board_errors
                .insert(project, "Choose a task board for this project.".to_string());
            cx.emit(TasksEvent::Changed);
            cx.notify();
            return;
        };
        let request_version = self
            .board_request_versions
            .entry(project)
            .and_modify(|version| *version += 1)
            .or_insert(1)
            .to_owned();
        self.board_loading.insert(project);
        self.board_errors.remove(&project);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    if connection.provider == IssueTrackerProvider::Personal {
                        let tasks = LocalStore::open_default()?.load_personal_tasks(project)?;
                        Ok(personal_board(project, tasks))
                    } else {
                        let client = TaskTrackerClient::new(connection)?;
                        client.load_board()
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                if this.board_request_versions.get(&project) != Some(&request_version) {
                    return;
                }
                this.board_loading.remove(&project);
                match result {
                    Ok(board) => {
                        if let Some(selected) = this.selected.get(&project) {
                            if !board
                                .issues
                                .iter()
                                .any(|task| task.reference.same_issue(selected))
                            {
                                this.selected.remove(&project);
                            }
                        }
                        if !this.selected.contains_key(&project) {
                            if let Some(first) = board.issues.first() {
                                this.selected.insert(project, first.reference.clone());
                            }
                        }
                        this.boards.insert(project, board);
                        this.board_errors.remove(&project);
                    }
                    Err(error) => {
                        this.boards.remove(&project);
                        this.board_errors.insert(project, format!("{error:#}"));
                    }
                }
                cx.emit(TasksEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn ensure_detail(
        &mut self,
        project: ProjectId,
        reference: TaskRef,
        cx: &mut Context<Self>,
    ) {
        let key = task_cache_key(&reference);
        if self.details.contains_key(&key) || self.detail_loading.contains(&key) {
            return;
        }
        if reference.provider == IssueTrackerProvider::Personal {
            match load_personal_task_detail(project, &reference) {
                Ok(detail) => {
                    self.details.insert(key.clone(), detail);
                    self.detail_errors.remove(&key);
                }
                Err(error) => {
                    self.detail_errors.insert(key, format!("{error:#}"));
                }
            }
            cx.emit(TasksEvent::Changed);
            cx.notify();
            return;
        }
        let Some(connection) = self.configured_connection(project, cx) else {
            self.detail_errors
                .insert(key, "Choose a task board for this project.".to_string());
            cx.notify();
            return;
        };
        let columns = self
            .boards
            .get(&project)
            .map(|board| board.columns.clone())
            .unwrap_or_default();
        self.detail_loading.insert(key.clone());
        self.detail_errors.remove(&key);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let client = TaskTrackerClient::new(connection)?;
                    client.load_task_detail(&reference, &columns)
                })
                .await;
            this.update(cx, |this, cx| {
                this.detail_loading.remove(&key);
                match result {
                    Ok(detail) => {
                        this.details.insert(key.clone(), detail);
                        this.detail_errors.remove(&key);
                    }
                    Err(error) => {
                        this.detail_errors.insert(key.clone(), format!("{error:#}"));
                    }
                }
                cx.emit(TasksEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn refresh_detail(
        &mut self,
        project: ProjectId,
        reference: TaskRef,
        cx: &mut Context<Self>,
    ) {
        let key = task_cache_key(&reference);
        if self.detail_loading.contains(&key) {
            return;
        }
        self.details.remove(&key);
        self.ensure_detail(project, reference, cx);
    }

    pub fn create_personal_task(
        &mut self,
        project: ProjectId,
        title: String,
        description_markdown: String,
        cx: &mut Context<Self>,
    ) {
        match LocalStore::open_default()
            .and_then(|store| store.create_personal_task(project, title, description_markdown))
        {
            Ok(task) => {
                let reference = task.task_ref();
                self.details
                    .insert(task_cache_key(&reference), task.detail());
                self.selected.insert(project, reference);
                self.refresh_project(project, cx);
            }
            Err(error) => {
                self.board_errors.insert(project, format!("{error:#}"));
            }
        }
        cx.emit(TasksEvent::Changed);
        cx.notify();
    }

    pub fn save_personal_task(&mut self, task: PersonalTaskRecord, cx: &mut Context<Self>) {
        let project = task.project_id;
        let reference = task.task_ref();
        match LocalStore::open_default().and_then(|store| store.upsert_personal_task(&task)) {
            Ok(()) => {
                self.details.remove(&task_cache_key(&reference));
                self.selected.insert(project, reference);
                self.refresh_project(project, cx);
            }
            Err(error) => {
                self.board_errors.insert(project, format!("{error:#}"));
            }
        }
        cx.emit(TasksEvent::Changed);
        cx.notify();
    }

    /// Update only priority on the freshest stored record so changing metadata
    /// cannot overwrite description text that is being autosaved separately.
    pub fn set_personal_task_priority(
        &mut self,
        project: ProjectId,
        task_id: Uuid,
        priority: PersonalTaskPriority,
        cx: &mut Context<Self>,
    ) {
        let result = LocalStore::open_default().and_then(|store| {
            let mut tasks = store.load_personal_tasks(project)?;
            let task = tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .ok_or_else(|| anyhow::anyhow!("Personal task not found"))?;
            task.priority = priority;
            let reference = task.task_ref();
            store.upsert_personal_task(task)?;
            Ok(reference)
        });

        match result {
            Ok(reference) => {
                self.details.remove(&task_cache_key(&reference));
                self.selected.insert(project, reference);
                self.refresh_project(project, cx);
            }
            Err(error) => {
                self.board_errors.insert(project, format!("{error:#}"));
            }
        }
        cx.emit(TasksEvent::Changed);
        cx.notify();
    }

    /// Silent autosave for inline live-doc editing. Reloads the freshest record
    /// and updates only the description, so a concurrent status change isn't
    /// clobbered. No board refresh — description edits don't change the list.
    pub fn autosave_personal_description(
        &mut self,
        project: ProjectId,
        task_id: Uuid,
        description_markdown: String,
        cx: &mut Context<Self>,
    ) {
        let version = self
            .personal_save_versions
            .entry(task_id)
            .and_modify(|version| *version = version.wrapping_add(1))
            .or_insert(1)
            .to_owned();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default().and_then(|store| {
                        store.update_personal_task_description(task_id, &description_markdown)
                    })
                })
                .await;
            this.update(cx, |this, cx| {
                if this.personal_save_versions.get(&task_id) != Some(&version) {
                    return;
                }
                match result {
                    Ok(()) => {
                        let task_id = task_id.to_string();
                        this.details
                            .retain(|_, detail| detail.summary.reference.issue_id != task_id);
                        this.board_errors.remove(&project);
                    }
                    Err(error) => {
                        this.board_errors.insert(project, format!("{error:#}"));
                    }
                }
                cx.emit(TasksEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn archive_personal_task(
        &mut self,
        project: ProjectId,
        task_id: Uuid,
        archived: bool,
        cx: &mut Context<Self>,
    ) {
        match LocalStore::open_default()
            .and_then(|store| store.archive_personal_task(task_id, archived))
        {
            Ok(()) => {
                self.selected.remove(&project);
                self.refresh_project(project, cx);
            }
            Err(error) => {
                self.board_errors.insert(project, format!("{error:#}"));
            }
        }
        cx.emit(TasksEvent::Changed);
        cx.notify();
    }

    pub fn load_personal_task(
        &self,
        project: ProjectId,
        task_id: Uuid,
    ) -> Option<PersonalTaskRecord> {
        LocalStore::open_default()
            .and_then(|store| store.load_personal_tasks(project))
            .ok()
            .and_then(|tasks| tasks.into_iter().find(|task| task.id == task_id))
    }

    fn retain_projects(&mut self, cx: &App) {
        let ids = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| project.id)
            .collect::<HashSet<_>>();
        self.boards.retain(|project, _| ids.contains(project));
        self.board_loading.retain(|project| ids.contains(project));
        self.board_errors.retain(|project, _| ids.contains(project));
        self.board_request_versions
            .retain(|project, _| ids.contains(project));
        self.task_tracker_connections
            .retain(|project, _| ids.contains(project));
        self.selected_connection
            .retain(|project, _| ids.contains(project));
        self.selected.retain(|project, _| ids.contains(project));
    }

    fn reload_changed_task_tracker_connections(&mut self, cx: &mut Context<Self>) {
        let current = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| (project.id, project.task_tracker_connections.clone()))
            .collect::<HashMap<_, _>>();
        let changed = current
            .iter()
            .filter_map(|(project, connections)| {
                (self.task_tracker_connections.get(project) != Some(connections))
                    .then_some(*project)
            })
            .collect::<Vec<_>>();

        self.task_tracker_connections = current;
        if changed.is_empty() {
            return;
        }

        // Credentials and source selection are part of the connection. Any
        // cached board error or in-flight request may therefore describe the
        // old connection and must not survive a settings save.
        self.details.clear();
        self.detail_errors.clear();
        for project in changed {
            self.boards.remove(&project);
            self.board_errors.remove(&project);
            self.board_loading.remove(&project);
            self.selected.remove(&project);
            self.refresh_project(project, cx);
        }
    }
}

pub fn personal_connection_id(project: ProjectId) -> Uuid {
    Uuid::from_u128(project.0.as_u128() ^ 0x7065_7273_6f6e_616c_7461_736b_626f_6172)
}

pub fn personal_board_connection(project: ProjectId) -> TaskTrackerConnection {
    TaskTrackerConnection {
        id: personal_connection_id(project),
        provider: IssueTrackerProvider::Personal,
        name: "Personal Board".to_string(),
        site_url: PersonalTaskRecord::source_site_url(project),
        email: String::new(),
        api_token: String::new(),
        source_id: Some(project.0.to_string()),
        source_name: Some("Personal Board".to_string()),
        source_kind: Some("personal".to_string()),
        provider_config_json: "{}".to_string(),
        filters_json: "{}".to_string(),
        board_id: None,
        board_name: Some("Personal Board".to_string()),
        assignee_filter: None,
        assignee_account_id: None,
        assignee_display_name: None,
    }
}

fn personal_board(project: ProjectId, tasks: Vec<PersonalTaskRecord>) -> TaskBoard {
    let issues = tasks
        .into_iter()
        .filter(|task| !task.archived)
        .map(|task| task.summary())
        .collect::<Vec<_>>();
    TaskBoard {
        connection_id: personal_connection_id(project),
        provider: IssueTrackerProvider::Personal,
        connection_name: "Personal Board".to_string(),
        source_id: project.0.to_string(),
        source_name: "Personal Board".to_string(),
        board_id: 0,
        board_name: "Personal Board".to_string(),
        assignee_filter: None,
        assignee_display_name: None,
        columns: PersonalTaskStatus::ALL
            .into_iter()
            .map(|status| TaskBoardColumn {
                name: status.label().to_string(),
                status_ids: vec![status.status_id().to_string()],
            })
            .collect(),
        issues,
    }
}

fn load_personal_task_detail(
    project: ProjectId,
    reference: &TaskRef,
) -> anyhow::Result<TaskDetail> {
    LocalStore::open_default()?
        .load_personal_tasks(project)?
        .into_iter()
        .find(|task| task.id.to_string() == reference.issue_id)
        .map(|task| task.detail())
        .ok_or_else(|| anyhow::anyhow!("Personal task not found"))
}

/// True for tasks that are still open (to-do / in-progress), i.e. not done/closed.
fn is_open_status(summary: &TaskSummary) -> bool {
    let key = format!(
        "{} {} {}",
        summary.status_category.as_deref().unwrap_or(""),
        summary.status,
        summary.column
    )
    .to_ascii_lowercase();
    !(key.contains("done")
        || key.contains("complete")
        || key.contains("closed")
        || key.contains("resolved")
        || key.contains("archived")
        || key.contains("cancel"))
}

pub fn task_cache_key(reference: &TaskRef) -> String {
    format!(
        "{:?}|{}|{}",
        reference.provider,
        reference
            .site_url
            .trim_end_matches('/')
            .to_ascii_lowercase(),
        reference.issue_key.to_ascii_uppercase()
    )
}
