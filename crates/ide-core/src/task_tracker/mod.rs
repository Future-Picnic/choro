use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::local_store::LocalStore;
use crate::project::ProjectId;

const PROVIDER_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const PROVIDER_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

fn provider_http_client(user_agent: &'static str) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .user_agent(user_agent)
        .connect_timeout(PROVIDER_CONNECT_TIMEOUT)
        .timeout(PROVIDER_REQUEST_TIMEOUT)
        .build()
        .context("failed to build task tracker HTTP client")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueTrackerProvider {
    Jira,
    Linear,
    ClickUp,
    Asana,
    PocketComet,
    Personal,
}

impl IssueTrackerProvider {
    pub const EXTERNAL: [Self; 4] = [Self::Jira, Self::Linear, Self::ClickUp, Self::Asana];

    pub fn label(self) -> &'static str {
        match self {
            Self::Jira => "Jira",
            Self::Linear => "Linear",
            Self::ClickUp => "ClickUp",
            Self::Asana => "Asana",
            Self::PocketComet => "PocketComet",
            Self::Personal => "Personal Board",
        }
    }

    pub fn is_external(self) -> bool {
        !matches!(self, Self::Personal)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTrackerConnection {
    pub id: Uuid,
    pub provider: IssueTrackerProvider,
    pub name: String,
    /// Provider base/workspace URL. Jira uses the site URL; provider APIs with a
    /// fixed endpoint may leave this empty and use the default endpoint.
    pub site_url: String,
    /// Jira Cloud account email. Other providers ignore this field.
    pub email: String,
    pub api_token: String,
    /// Provider-neutral selected source id: Jira board, Linear team, ClickUp
    /// list, Asana project.
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub source_name: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    #[serde(default = "default_task_json")]
    pub provider_config_json: String,
    #[serde(default = "default_task_json")]
    pub filters_json: String,
    /// Legacy Jira board fields retained for migration and compatibility.
    #[serde(default)]
    pub board_id: Option<i64>,
    #[serde(default)]
    pub board_name: Option<String>,
    #[serde(default)]
    pub assignee_filter: Option<String>,
    #[serde(default)]
    pub assignee_account_id: Option<String>,
    #[serde(default)]
    pub assignee_display_name: Option<String>,
}

impl std::fmt::Debug for TaskTrackerConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TaskTrackerConnection")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("name", &self.name)
            .field(
                "site_url",
                &crate::redaction::redact_sensitive_text(&self.site_url),
            )
            .field("email", &self.email)
            .field("api_token", &"[REDACTED]")
            .field("source_id", &self.source_id)
            .field("source_name", &self.source_name)
            .field("source_kind", &self.source_kind)
            .field(
                "provider_config_json",
                &crate::redaction::redact_sensitive_text(&self.provider_config_json),
            )
            .field(
                "filters_json",
                &crate::redaction::redact_sensitive_text(&self.filters_json),
            )
            .field("board_id", &self.board_id)
            .field("board_name", &self.board_name)
            .field("assignee_filter", &self.assignee_filter)
            .field("assignee_account_id", &self.assignee_account_id)
            .field("assignee_display_name", &self.assignee_display_name)
            .finish()
    }
}

impl TaskTrackerConnection {
    pub fn new_jira(
        name: impl Into<String>,
        site_url: impl Into<String>,
        email: impl Into<String>,
        api_token: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            provider: IssueTrackerProvider::Jira,
            name: name.into(),
            site_url: site_url.into(),
            email: email.into(),
            api_token: api_token.into(),
            source_id: None,
            source_name: None,
            source_kind: None,
            provider_config_json: default_task_json(),
            filters_json: default_task_json(),
            board_id: None,
            board_name: None,
            assignee_filter: None,
            assignee_account_id: None,
            assignee_display_name: None,
        }
    }

    pub fn new_external(
        provider: IssueTrackerProvider,
        name: impl Into<String>,
        site_url: impl Into<String>,
        api_token: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            provider,
            name: name.into(),
            site_url: site_url.into(),
            email: String::new(),
            api_token: api_token.into(),
            source_id: None,
            source_name: None,
            source_kind: None,
            provider_config_json: default_task_json(),
            filters_json: default_task_json(),
            board_id: None,
            board_name: None,
            assignee_filter: None,
            assignee_account_id: None,
            assignee_display_name: None,
        }
    }

    pub fn normalized_site_url(&self) -> String {
        normalize_site_url(&self.site_url)
    }

    pub fn expanded_api_token(&self) -> Result<String> {
        crate::env::expand_env_vars(&self.api_token)
    }

    pub(crate) fn redact_diagnostic(&self, diagnostic: &str) -> String {
        let expanded = self.expanded_api_token().ok();
        crate::redaction::redact_sensitive_text_with(
            diagnostic,
            std::iter::once(self.api_token.as_str()).chain(expanded.iter().map(String::as_str)),
        )
    }

    pub fn assignee_filter_jql(&self) -> Option<String> {
        if let Some(account_id) = self
            .assignee_account_id
            .as_ref()
            .map(|account_id| account_id.trim())
            .filter(|account_id| !account_id.is_empty())
        {
            return Some(format!("assignee = {}", jira_jql_operand(account_id)));
        }
        jira_assignee_filter_to_jql(self.assignee_filter.as_deref().unwrap_or_default())
    }

    pub fn assignee_label(&self) -> Option<&str> {
        self.assignee_display_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or_else(|| {
                self.assignee_account_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|account_id| !account_id.is_empty())
            })
    }

    pub fn selected_source_id(&self) -> Option<String> {
        self.source_id
            .as_ref()
            .map(|id| id.trim())
            .filter(|id| !id.is_empty())
            .map(ToString::to_string)
            .or_else(|| self.board_id.map(|id| id.to_string()))
    }

    pub fn selected_source_name(&self) -> Option<String> {
        self.source_name
            .as_ref()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                self.board_name
                    .as_ref()
                    .map(|name| name.trim())
                    .filter(|name| !name.is_empty())
                    .map(ToString::to_string)
            })
    }

    pub fn has_selected_source(&self) -> bool {
        self.selected_source_id().is_some()
    }

    /// Status (by display name) to suggest when a PR is shipped for a task on
    /// this connection. Stored in `provider_config_json` so it needs no schema
    /// change. `None` means "no suggestion configured".
    pub fn pr_done_status(&self) -> Option<String> {
        serde_json::from_str::<Value>(&self.provider_config_json)
            .ok()
            .as_ref()
            .and_then(|value| value.get("pr_done_status"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    /// Returns a copy of this connection with the PR-done status suggestion set
    /// (or cleared). Immutable — never mutates `self`.
    pub fn with_pr_done_status(&self, status: Option<String>) -> Self {
        let mut config = serde_json::from_str::<Value>(&self.provider_config_json)
            .ok()
            .filter(Value::is_object)
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        if let Some(map) = config.as_object_mut() {
            match status
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            {
                Some(value) => {
                    map.insert("pr_done_status".to_string(), Value::String(value));
                }
                None => {
                    map.remove("pr_done_status");
                }
            }
        }
        Self {
            provider_config_json: config.to_string(),
            ..self.clone()
        }
    }
}

fn default_task_json() -> String {
    "{}".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskRef {
    pub provider: IssueTrackerProvider,
    pub site_url: String,
    pub issue_id: String,
    pub issue_key: String,
    pub issue_url: String,
    pub title: String,
}

impl TaskRef {
    pub fn same_issue(&self, other: &TaskRef) -> bool {
        self.provider == other.provider
            && normalize_site_url(&self.site_url) == normalize_site_url(&other.site_url)
            && self.issue_key.eq_ignore_ascii_case(&other.issue_key)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBoard {
    pub connection_id: Uuid,
    pub provider: IssueTrackerProvider,
    pub connection_name: String,
    pub source_id: String,
    pub source_name: String,
    pub board_id: i64,
    pub board_name: String,
    pub assignee_filter: Option<String>,
    pub assignee_display_name: Option<String>,
    pub columns: Vec<TaskBoardColumn>,
    pub issues: Vec<TaskSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTrackerSource {
    pub id: String,
    pub name: String,
    pub source_type: String,
}

impl TaskTrackerSource {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        source_type: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            source_type: source_type.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskBoardColumn {
    pub name: String,
    pub status_ids: Vec<String>,
}

/// A status a task can be moved to, resolved for a specific provider.
///
/// `apply_id` is the provider-specific token used to *apply* the change: a Jira
/// transition id, a Linear workflow-state id, a ClickUp status name, or a
/// Personal-board status id. `name` is always the human-readable target status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskStatusOption {
    pub apply_id: String,
    pub name: String,
    /// Coarse category ("new" | "in progress" | "done") for coloring, when known.
    pub category: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSummary {
    pub reference: TaskRef,
    pub status_id: String,
    pub status: String,
    pub status_category: Option<String>,
    pub column: String,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub issue_type: Option<String>,
    pub labels: Vec<String>,
    pub updated: Option<String>,
    pub created: Option<String>,
}

/// A read-only PocketComet project snapshot pushed over Choro's authenticated
/// loopback integration. Choro stores it inside the managed task connection so
/// the source remains available between refreshes without PocketComet secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTaskSourceSnapshot {
    pub device_id: String,
    pub workspace_id: String,
    pub pocketcomet_project_id: String,
    pub project_name: String,
    #[serde(default)]
    pub statuses: Vec<PocketCometTaskStatus>,
    pub assignees: Vec<PocketCometTaskAssignee>,
    pub tasks: Vec<PocketCometTask>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTaskStatus {
    pub id: String,
    pub name: String,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTaskAssignee {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTask {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status_id: String,
    pub status: String,
    pub status_category: String,
    pub list_id: String,
    pub list_name: String,
    pub assignee_id: Option<String>,
    pub assignee_name: Option<String>,
    pub priority: String,
    pub labels: Vec<String>,
    #[serde(default)]
    pub comments: Vec<PocketCometTaskComment>,
    #[serde(default)]
    pub attachments: Vec<PocketCometTaskAttachment>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTaskComment {
    pub id: String,
    pub author_name: String,
    pub body: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTaskAttachment {
    pub id: String,
    pub file_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
    pub asset_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PocketCometTaskAction {
    pub action_id: String,
    pub device_id: String,
    pub workspace_id: String,
    pub pocketcomet_project_id: String,
    pub task_id: String,
    pub created_at: u64,
    #[serde(flatten)]
    pub command: PocketCometTaskActionCommand,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PocketCometTaskActionCommand {
    SetStatus { status_id: String },
    AddComment { body: String },
}

#[derive(Default, Serialize, Deserialize)]
struct PocketCometTaskActionQueue {
    actions: Vec<PocketCometTaskAction>,
}

fn pocketcomet_task_action_queue_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn pocketcomet_task_action_queue_path() -> Result<PathBuf> {
    Ok(LocalStore::open_default()?
        .app_data_dir()
        .join("task-actions")
        .join("pocketcomet.json"))
}

fn read_pocketcomet_task_action_queue(path: &Path) -> Result<PocketCometTaskActionQueue> {
    if !path.is_file() {
        return Ok(PocketCometTaskActionQueue::default());
    }
    let bytes = fs::read(path).context("failed to read PocketComet task actions")?;
    serde_json::from_slice(&bytes).context("PocketComet task actions are unreadable")
}

fn write_pocketcomet_task_action_queue(
    path: &Path,
    queue: &PocketCometTaskActionQueue,
) -> Result<()> {
    let parent = path
        .parent()
        .context("PocketComet task action path has no parent")?;
    fs::create_dir_all(parent).context("failed to create PocketComet task action directory")?;
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(queue)?)
        .context("failed to stage PocketComet task actions")?;
    fs::rename(temporary, path).context("failed to save PocketComet task actions")
}

fn enqueue_pocketcomet_task_action(
    source: &PocketCometTaskSourceSnapshot,
    task_id: &str,
    command: PocketCometTaskActionCommand,
) -> Result<()> {
    if !source.tasks.iter().any(|task| task.id == task_id) {
        return Err(anyhow!("PocketComet task is no longer available"));
    }
    let _guard = pocketcomet_task_action_queue_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = pocketcomet_task_action_queue_path()?;
    let mut queue = read_pocketcomet_task_action_queue(&path)?;
    if matches!(command, PocketCometTaskActionCommand::SetStatus { .. }) {
        queue.actions.retain(|action| {
            action.device_id != source.device_id
                || action.workspace_id != source.workspace_id
                || action.pocketcomet_project_id != source.pocketcomet_project_id
                || action.task_id != task_id
                || !matches!(
                    action.command,
                    PocketCometTaskActionCommand::SetStatus { .. }
                )
        });
    }
    if queue.actions.len() >= 1_000 {
        return Err(anyhow!(
            "PocketComet has too many pending task changes; open PocketComet to sync them"
        ));
    }
    queue.actions.push(PocketCometTaskAction {
        action_id: Uuid::new_v4().to_string(),
        device_id: source.device_id.clone(),
        workspace_id: source.workspace_id.clone(),
        pocketcomet_project_id: source.pocketcomet_project_id.clone(),
        task_id: task_id.to_string(),
        created_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        command,
    });
    write_pocketcomet_task_action_queue(&path, &queue)
}

pub fn pending_pocketcomet_task_actions(
    device_id: &str,
    workspace_id: &str,
) -> Result<Vec<PocketCometTaskAction>> {
    let _guard = pocketcomet_task_action_queue_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = pocketcomet_task_action_queue_path()?;
    Ok(read_pocketcomet_task_action_queue(&path)?
        .actions
        .into_iter()
        .filter(|action| action.device_id == device_id && action.workspace_id == workspace_id)
        .take(100)
        .collect())
}

pub fn acknowledge_pocketcomet_task_actions(
    device_id: &str,
    workspace_id: &str,
    action_ids: &[String],
) -> Result<()> {
    let acknowledged = action_ids
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let _guard = pocketcomet_task_action_queue_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let path = pocketcomet_task_action_queue_path()?;
    let mut queue = read_pocketcomet_task_action_queue(&path)?;
    queue.actions.retain(|action| {
        action.device_id != device_id
            || action.workspace_id != workspace_id
            || !acknowledged.contains(action.action_id.as_str())
    });
    write_pocketcomet_task_action_queue(&path, &queue)
}

pub fn is_valid_pocketcomet_task_asset_file(asset_file: &str) -> bool {
    let Some((digest, extension)) = asset_file
        .strip_prefix("pocketcomet-")
        .and_then(|value| value.rsplit_once('.'))
    else {
        return false;
    };
    digest.len() == 64
        && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
        && matches!(
            extension,
            "apng" | "avif" | "gif" | "jpg" | "png" | "svg" | "webp"
        )
}

pub fn pocketcomet_task_asset_path(asset_file: &str) -> Option<PathBuf> {
    if !is_valid_pocketcomet_task_asset_file(asset_file) {
        return None;
    }
    let path = LocalStore::open_default()
        .ok()?
        .app_data_dir()
        .join("task-assets")
        .join("pocketcomet")
        .join(asset_file);
    path.is_file().then_some(path)
}

pub fn store_pocketcomet_task_asset(mime: &str, bytes: &[u8]) -> Result<String> {
    if bytes.is_empty() {
        return Err(anyhow!("PocketComet sent an empty task image"));
    }
    let extension = match mime.trim().to_ascii_lowercase().as_str() {
        "image/apng" => "apng",
        "image/avif" => "avif",
        "image/gif" => "gif",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/svg+xml" => "svg",
        "image/webp" => "webp",
        _ => return Err(anyhow!("PocketComet task attachments must be images")),
    };
    let file_name = format!("pocketcomet-{:x}.{extension}", Sha256::digest(bytes));
    let asset_dir = LocalStore::open_default()?
        .app_data_dir()
        .join("task-assets")
        .join("pocketcomet");
    fs::create_dir_all(&asset_dir).context("failed to create PocketComet task asset directory")?;
    let path = asset_dir.join(&file_name);
    if !path.is_file() {
        fs::write(&path, bytes).context("failed to store PocketComet task image")?;
    }
    Ok(file_name)
}

impl PocketCometTaskSourceSnapshot {
    pub fn from_connection(connection: &TaskTrackerConnection) -> Result<Self> {
        if connection.provider != IssueTrackerProvider::PocketComet {
            return Err(anyhow!("task source is not managed by PocketComet"));
        }
        serde_json::from_str(&connection.provider_config_json)
            .context("PocketComet task snapshot is unreadable")
    }

    pub fn users(&self) -> Vec<TaskTrackerUser> {
        self.assignees
            .iter()
            .map(|assignee| TaskTrackerUser {
                account_id: assignee.id.clone(),
                display_name: assignee.name.clone(),
                email: assignee.email.clone(),
                avatar_url: None,
                active: true,
            })
            .collect()
    }

    pub fn board(&self, connection: &TaskTrackerConnection) -> TaskBoard {
        let mut columns = self
            .statuses
            .iter()
            .map(|status| TaskBoardColumn {
                name: status.name.clone(),
                status_ids: vec![status.id.clone()],
            })
            .collect::<Vec<_>>();
        for task in &self.tasks {
            if let Some(column) = columns.iter_mut().find(|column| column.name == task.status) {
                if !column.status_ids.contains(&task.status_id) {
                    column.status_ids.push(task.status_id.clone());
                }
            } else {
                columns.push(TaskBoardColumn {
                    name: task.status.clone(),
                    status_ids: vec![task.status_id.clone()],
                });
            }
        }

        let issues = self
            .tasks
            .iter()
            .filter(|task| {
                connection
                    .assignee_account_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(|assignee| task.assignee_id.as_deref() == Some(assignee))
                    .or_else(|| {
                        connection
                            .assignee_display_name
                            .as_deref()
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(|assignee| task.assignee_name.as_deref() == Some(assignee))
                    })
                    .unwrap_or(true)
            })
            .map(|task| task.summary(self))
            .collect();

        TaskBoard {
            connection_id: connection.id,
            provider: IssueTrackerProvider::PocketComet,
            connection_name: connection.name.clone(),
            source_id: self.pocketcomet_project_id.clone(),
            source_name: self.project_name.clone(),
            board_id: 0,
            board_name: self.project_name.clone(),
            assignee_filter: connection.assignee_filter.clone(),
            assignee_display_name: connection.assignee_display_name.clone(),
            columns,
            issues,
        }
    }

    pub fn detail(&self, reference: &TaskRef) -> Result<TaskDetail> {
        let task = self
            .tasks
            .iter()
            .find(|task| task.id == reference.issue_id)
            .ok_or_else(|| anyhow!("PocketComet task is no longer available"))?;
        Ok(TaskDetail {
            summary: task.summary(self),
            description: TaskRichText::plain(task.description.clone()),
            comments: task
                .comments
                .iter()
                .map(|comment| TaskComment {
                    author: comment.author_name.clone(),
                    body: TaskRichText::plain(comment.body.clone()),
                    created: Some(comment.created_at.to_string()),
                })
                .collect(),
            attachments: task
                .attachments
                .iter()
                .map(|attachment| TaskAttachment {
                    id: attachment.id.clone(),
                    filename: attachment.file_name.clone(),
                    mime_type: attachment.mime_type.clone(),
                    content_url: None,
                    thumbnail_url: None,
                    local_path: pocketcomet_task_asset_path(&attachment.asset_file),
                    size: Some(attachment.size_bytes),
                })
                .collect(),
        })
    }
}

impl PocketCometTask {
    fn summary(&self, source: &PocketCometTaskSourceSnapshot) -> TaskSummary {
        let suffix = self
            .id
            .chars()
            .rev()
            .take(8)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .to_ascii_uppercase();
        TaskSummary {
            reference: TaskRef {
                provider: IssueTrackerProvider::PocketComet,
                site_url: format!(
                    "pocketcomet://workspace/{}/project/{}",
                    source.workspace_id, source.pocketcomet_project_id
                ),
                issue_id: self.id.clone(),
                issue_key: format!("PC-{suffix}"),
                issue_url: format!("pocketcomet://task/{}", self.id),
                title: self.title.clone(),
            },
            status_id: self.status_id.clone(),
            status: self.status.clone(),
            status_category: Some(self.status_category.clone()),
            column: self.status.clone(),
            assignee: self.assignee_name.clone(),
            priority: (self.priority != "none").then(|| self.priority.clone()),
            issue_type: Some(format!("{} task", self.list_name)),
            labels: self.labels.clone(),
            updated: Some(self.updated_at.to_string()),
            created: Some(self.created_at.to_string()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDetail {
    pub summary: TaskSummary,
    pub description: TaskRichText,
    pub comments: Vec<TaskComment>,
    pub attachments: Vec<TaskAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskComment {
    pub author: String,
    pub body: TaskRichText,
    pub created: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRichText {
    pub text: String,
    pub blocks: Vec<TaskContentBlock>,
}

impl TaskRichText {
    pub fn plain(text: impl Into<String>) -> Self {
        let text = text.into();
        let blocks = (!text.trim().is_empty()).then(|| TaskContentBlock::Text(text.clone()));
        Self {
            text,
            blocks: blocks.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskContentBlock {
    Text(String),
    Image(TaskInlineImage),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskInlineImage {
    pub attachment_id: Option<String>,
    pub media_id: Option<String>,
    pub filename: Option<String>,
    pub alt: Option<String>,
    pub mime_type: Option<String>,
    pub local_path: Option<PathBuf>,
    pub content_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAttachment {
    pub id: String,
    pub filename: String,
    pub mime_type: Option<String>,
    pub content_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub local_path: Option<PathBuf>,
    pub size: Option<u64>,
}

impl TaskAttachment {
    pub fn is_image(&self) -> bool {
        self.mime_type
            .as_deref()
            .is_some_and(|mime| mime.to_ascii_lowercase().starts_with("image/"))
            || image_extension(Path::new(&self.filename)).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JiraBoard {
    pub id: i64,
    pub name: String,
    pub board_type: String,
}

impl From<JiraBoard> for TaskTrackerSource {
    fn from(value: JiraBoard) -> Self {
        Self {
            id: value.id.to_string(),
            name: value.name,
            source_type: value.board_type,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTrackerUser {
    pub account_id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub active: bool,
}

pub type JiraUser = TaskTrackerUser;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonalTaskStatus {
    Todo,
    InProgress,
    Done,
}

impl PersonalTaskStatus {
    pub const ALL: [Self; 3] = [Self::Todo, Self::InProgress, Self::Done];

    pub fn label(self) -> &'static str {
        match self {
            Self::Todo => "To Do",
            Self::InProgress => "In Progress",
            Self::Done => "Done",
        }
    }

    pub fn status_id(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::InProgress => "in_progress",
            Self::Done => "done",
        }
    }

    pub fn from_status_id(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.status_id() == value)
    }

    /// The available statuses as generic options, for the PR-done suggestion UI.
    pub fn options() -> Vec<TaskStatusOption> {
        Self::ALL
            .into_iter()
            .map(|status| TaskStatusOption {
                apply_id: status.status_id().to_string(),
                name: status.label().to_string(),
                category: Some(
                    match status {
                        Self::Done => "done",
                        Self::InProgress => "in progress",
                        Self::Todo => "new",
                    }
                    .to_string(),
                ),
            })
            .collect()
    }
}

impl Default for PersonalTaskStatus {
    fn default() -> Self {
        Self::Todo
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonalTaskPriority {
    Low,
    Medium,
    High,
    Urgent,
}

impl PersonalTaskPriority {
    pub const ALL: [Self; 4] = [Self::Low, Self::Medium, Self::High, Self::Urgent];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Urgent => "Urgent",
        }
    }
}

impl Default for PersonalTaskPriority {
    fn default() -> Self {
        Self::Medium
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalTaskRecord {
    pub id: Uuid,
    pub project_id: ProjectId,
    pub key_number: i64,
    pub title: String,
    pub description_markdown: String,
    pub status: PersonalTaskStatus,
    pub priority: PersonalTaskPriority,
    #[serde(default)]
    pub labels: Vec<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub archived: bool,
}

impl PersonalTaskRecord {
    pub fn issue_key(&self) -> String {
        format!("TASK-{}", self.key_number)
    }

    pub fn source_site_url(project_id: ProjectId) -> String {
        format!("personal://{}", project_id.0)
    }

    pub fn task_ref(&self) -> TaskRef {
        let issue_key = self.issue_key();
        TaskRef {
            provider: IssueTrackerProvider::Personal,
            site_url: Self::source_site_url(self.project_id),
            issue_id: self.id.to_string(),
            issue_key: issue_key.clone(),
            issue_url: format!("personal://{}/{}", self.project_id.0, issue_key),
            title: self.title.clone(),
        }
    }

    pub fn summary(&self) -> TaskSummary {
        let status = self.status.label().to_string();
        TaskSummary {
            reference: self.task_ref(),
            status_id: self.status.status_id().to_string(),
            status: status.clone(),
            status_category: match self.status {
                PersonalTaskStatus::Done => Some("done".to_string()),
                PersonalTaskStatus::InProgress => Some("in progress".to_string()),
                PersonalTaskStatus::Todo => Some("new".to_string()),
            },
            column: status,
            assignee: None,
            priority: Some(self.priority.label().to_string()),
            issue_type: Some("Personal task".to_string()),
            labels: self.labels.clone(),
            updated: Some(self.updated_at.to_string()),
            created: Some(self.created_at.to_string()),
        }
    }

    pub fn detail(&self) -> TaskDetail {
        self.detail_with_comments(Vec::new())
    }

    pub fn detail_with_comments(&self, comments: Vec<TaskComment>) -> TaskDetail {
        TaskDetail {
            summary: self.summary(),
            description: TaskRichText::plain(self.description_markdown.clone()),
            comments,
            attachments: Vec::new(),
        }
    }

    /// Returns a copy of this record with a different status. Immutable.
    pub fn with_status(&self, status: PersonalTaskStatus) -> Self {
        Self {
            status,
            ..self.clone()
        }
    }
}

/// A locally-stored activity note on a personal-board task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersonalTaskComment {
    pub id: Uuid,
    pub task_id: Uuid,
    pub author: String,
    pub body: String,
    pub created_at: u64,
}

mod client;
mod jira;
mod support;

pub use crate::env::expand_env_vars;
pub use client::TaskTrackerClient;
pub use jira::JiraClient;
pub use support::{
    jira_adf_to_rich_text, jira_adf_to_text, jira_assignee_filter_to_jql, normalize_site_url,
};

use support::*;
