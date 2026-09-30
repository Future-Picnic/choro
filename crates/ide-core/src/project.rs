use std::path::{Component, Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::task_tracker::TaskTrackerConnection;

pub const DEFAULT_PROJECT_ICON: &str = "folder";
pub const DEFAULT_PROJECT_ICON_COLOR: &str = "default";
pub const CUSTOM_PROJECT_SVG_ICON: &str = "custom-svg";

pub fn default_project_icon() -> String {
    DEFAULT_PROJECT_ICON.to_string()
}

pub fn default_project_icon_color() -> String {
    DEFAULT_PROJECT_ICON_COLOR.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectId(pub Uuid);

impl ProjectId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProjectId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectSectionId(pub Uuid);

impl ProjectSectionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ProjectSectionId {
    fn default() -> Self {
        Self::new()
    }
}

/// A user-defined sidebar grouping for projects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectSection {
    pub id: ProjectSectionId,
    pub name: String,
    #[serde(default)]
    pub collapsed: bool,
}

impl ProjectSection {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: ProjectSectionId::new(),
            name: name.into(),
            collapsed: false,
        }
    }
}

/// A user-defined run command, shown as a one-click button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptPreset {
    pub id: Uuid,
    pub name: String,
    pub command: String,
}

/// What Choro should do after GitHub reports that a workflow pull request is
/// eligible to merge. Confirmation remains the safe default for newly-created
/// and legacy values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GitWorkflowCompletionPolicy {
    #[default]
    ConfirmBeforeMerge,
    AutoMergeWhenReady,
}

impl GitWorkflowCompletionPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfirmBeforeMerge => "confirm_before_merge",
            Self::AutoMergeWhenReady => "auto_merge_when_ready",
        }
    }
}

impl std::str::FromStr for GitWorkflowCompletionPolicy {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "confirm_before_merge" => Ok(Self::ConfirmBeforeMerge),
            "auto_merge_when_ready" => Ok(Self::AutoMergeWhenReady),
            _ => anyhow::bail!("unknown Git workflow completion policy: {value}"),
        }
    }
}

/// A reusable remote source-to-destination route. `repository_path` is always
/// relative to the owning project; `.` identifies the project root repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorkflow {
    pub id: Uuid,
    pub repository_path: PathBuf,
    pub name: String,
    pub source_branch: String,
    pub destination_branch: String,
    #[serde(default)]
    pub completion_policy: GitWorkflowCompletionPolicy,
}

impl GitWorkflow {
    pub fn new(
        repository_path: PathBuf,
        name: impl Into<String>,
        source_branch: impl Into<String>,
        destination_branch: impl Into<String>,
        completion_policy: GitWorkflowCompletionPolicy,
    ) -> Result<Self> {
        let workflow = Self {
            id: Uuid::new_v4(),
            repository_path,
            name: name.into().trim().to_string(),
            source_branch: source_branch.into(),
            destination_branch: destination_branch.into(),
            completion_policy,
        };
        workflow.validate()?;
        Ok(workflow)
    }

    pub fn validate(&self) -> Result<()> {
        validate_repository_relative_path(&self.repository_path)?;
        if self.name.trim().is_empty() {
            anyhow::bail!("Workflow name is required");
        }
        if self.source_branch.trim().is_empty() || self.destination_branch.trim().is_empty() {
            anyhow::bail!("Choose both a source and destination branch");
        }
        if self.source_branch.trim().eq(self.destination_branch.trim()) {
            anyhow::bail!("Source and destination branches must be different");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitWorkflowRunState {
    CreatingPullRequest,
    WaitingForRequirements,
    AwaitingConfirmation,
    AutoMergeEnabled,
    Merged,
    Blocked,
    Closed,
    NeedsAttention,
    Failed,
}

impl GitWorkflowRunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreatingPullRequest => "creating_pull_request",
            Self::WaitingForRequirements => "waiting_for_requirements",
            Self::AwaitingConfirmation => "awaiting_confirmation",
            Self::AutoMergeEnabled => "auto_merge_enabled",
            Self::Merged => "merged",
            Self::Blocked => "blocked",
            Self::Closed => "closed",
            Self::NeedsAttention => "needs_attention",
            Self::Failed => "failed",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Merged | Self::Closed | Self::Failed)
    }
}

impl std::str::FromStr for GitWorkflowRunState {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "creating_pull_request" => Ok(Self::CreatingPullRequest),
            "waiting_for_requirements" => Ok(Self::WaitingForRequirements),
            "awaiting_confirmation" => Ok(Self::AwaitingConfirmation),
            "auto_merge_enabled" => Ok(Self::AutoMergeEnabled),
            "merged" => Ok(Self::Merged),
            "blocked" => Ok(Self::Blocked),
            "closed" => Ok(Self::Closed),
            "needs_attention" => Ok(Self::NeedsAttention),
            "failed" => Ok(Self::Failed),
            _ => anyhow::bail!("unknown Git workflow run state: {value}"),
        }
    }
}

/// Durable local activity for either a saved workflow or a one-time route.
/// GitHub remains authoritative; these records let Choro resume reconciliation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitWorkflowRun {
    pub id: Uuid,
    pub workflow_id: Option<Uuid>,
    pub repository_path: PathBuf,
    pub source_branch: String,
    pub destination_branch: String,
    pub pull_request_number: Option<u64>,
    pub expected_head_sha: Option<String>,
    pub state: GitWorkflowRunState,
    pub error: Option<String>,
    pub started_at: u64,
    pub updated_at: u64,
}

impl GitWorkflowRun {
    pub fn validate(&self) -> Result<()> {
        validate_repository_relative_path(&self.repository_path)?;
        if self.source_branch.trim().is_empty() || self.destination_branch.trim().is_empty() {
            anyhow::bail!("Workflow run requires both source and destination branches");
        }
        if self.source_branch.trim().eq(self.destination_branch.trim()) {
            anyhow::bail!("Workflow run source and destination must be different");
        }
        Ok(())
    }
}

/// Reject paths that could escape the project boundary before they reach a Git
/// or GitHub helper. The root repository has the explicit portable value `.`.
pub fn validate_repository_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        anyhow::bail!("Repository path must be relative to the project root");
    }
    for component in path.components() {
        if matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        ) {
            anyhow::bail!("Repository path cannot leave the project root");
        }
    }
    Ok(())
}

impl ScriptPreset {
    pub fn new(name: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            command: command.into(),
        }
    }
}

/// The database/provider selected by the user. Supabase deliberately remains
/// distinct in the product even though it uses the PostgreSQL wire protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DbProvider {
    #[default]
    MongoDb,
    PostgreSql,
    Supabase,
    SQLite,
    MySql,
    MariaDb,
}

impl DbProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MongoDb => "mongodb",
            Self::PostgreSql => "postgresql",
            Self::Supabase => "supabase",
            Self::SQLite => "sqlite",
            Self::MySql => "mysql",
            Self::MariaDb => "mariadb",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::MongoDb => "MongoDB",
            Self::PostgreSql => "PostgreSQL",
            Self::Supabase => "Supabase",
            Self::SQLite => "SQLite",
            Self::MySql => "MySQL",
            Self::MariaDb => "MariaDB",
        }
    }

    pub fn is_postgres(self) -> bool {
        matches!(self, Self::PostgreSql | Self::Supabase)
    }

    pub fn is_mysql(self) -> bool {
        matches!(self, Self::MySql | Self::MariaDb)
    }
}

impl std::str::FromStr for DbProvider {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "mongodb" => Ok(Self::MongoDb),
            "postgresql" | "postgres" => Ok(Self::PostgreSql),
            "supabase" => Ok(Self::Supabase),
            "sqlite" => Ok(Self::SQLite),
            "mysql" => Ok(Self::MySql),
            "mariadb" => Ok(Self::MariaDb),
            _ => anyhow::bail!("unsupported database provider: {value}"),
        }
    }
}

/// A saved database connection. The URI may
/// reference env vars as `${VAR}` so secrets stay out of the config file.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbConnection {
    pub id: Uuid,
    #[serde(default)]
    pub provider: DbProvider,
    /// Blocks all editor writes while still allowing browsing and filtering.
    #[serde(default)]
    pub read_only: bool,
    pub name: String,
    pub uri: String,
}

impl std::fmt::Debug for DbConnection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DbConnection")
            .field("id", &self.id)
            .field("provider", &self.provider)
            .field("read_only", &self.read_only)
            .field("name", &self.name)
            .field("uri", &crate::redaction::redact_sensitive_text(&self.uri))
            .finish()
    }
}

impl DbConnection {
    pub fn new(name: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            provider: DbProvider::MongoDb,
            read_only: false,
            name: name.into(),
            uri: uri.into(),
        }
    }

    pub fn new_for(provider: DbProvider, name: impl Into<String>, uri: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            provider,
            read_only: provider.is_postgres() || provider.is_mysql(),
            name: name.into(),
            uri: uri.into(),
        }
    }

    /// The URI with `${VAR}` placeholders expanded from the environment.
    pub fn expanded_uri(&self) -> Result<String> {
        crate::env::expand_env_vars(&self.uri)
    }

    /// Heuristic: connections named like production get extra guard rails.
    pub fn looks_like_prod(&self) -> bool {
        let name = self.name.to_lowercase();
        name.contains("prod") || name.contains("live")
    }
}

/// A reusable project-level visual/context reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectReferenceKind {
    Image,
    Figma,
    Pencil,
    Url,
    /// Any local file, copied into the project's assets and opened with the
    /// system default app. Supersedes the Pencil-specific kind, which is kept
    /// only so existing references still deserialize.
    File,
}

impl ProjectReferenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Figma => "figma",
            Self::Pencil => "pencil",
            Self::Url => "url",
            Self::File => "file",
        }
    }

    pub fn from_label(value: &str) -> Option<Self> {
        match value {
            "image" => Some(Self::Image),
            "figma" => Some(Self::Figma),
            "pencil" => Some(Self::Pencil),
            "url" => Some(Self::Url),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

/// Metadata for a project context reference. File paths are relative to the
/// LocalStore root unless they come from the original user source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectReference {
    pub id: Uuid,
    pub project_id: ProjectId,
    pub kind: ProjectReferenceKind,
    pub title: String,
    pub source: String,
    pub preview_relative_path: Option<PathBuf>,
    pub notes: String,
    pub metadata_json: String,
    pub sort_order: i64,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A folder the user manages in the app — usually a git repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    pub name: String,
    pub path: PathBuf,
    #[serde(default = "default_project_icon")]
    pub icon: String,
    #[serde(default = "default_project_icon_color")]
    pub icon_color: String,
    /// App-owned custom SVG. The file is copied into the local project data
    /// directory; arbitrary external images are not supported.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_image_path: Option<PathBuf>,
    #[serde(default)]
    pub section_id: Option<ProjectSectionId>,
    #[serde(default)]
    pub is_favorite: bool,
    #[serde(default)]
    pub presets: Vec<ScriptPreset>,
    #[serde(default)]
    pub db_connections: Vec<DbConnection>,
    #[serde(default)]
    pub task_tracker_connections: Vec<TaskTrackerConnection>,
    #[serde(default)]
    pub git_workflows: Vec<GitWorkflow>,
    #[serde(default)]
    pub git_workflow_runs: Vec<GitWorkflowRun>,
}

impl Project {
    pub fn from_path(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Self {
            id: ProjectId::new(),
            name,
            path,
            icon: default_project_icon(),
            icon_color: default_project_icon_color(),
            icon_image_path: None,
            section_id: None,
            is_favorite: false,
            presets: Vec::new(),
            db_connections: Vec::new(),
            task_tracker_connections: Vec::new(),
            git_workflows: Vec::new(),
            git_workflow_runs: Vec::new(),
        }
    }

    pub fn validate_git_workflow(&self, workflow: &GitWorkflow) -> Result<()> {
        workflow.validate()?;
        if self.git_workflows.iter().any(|existing| {
            existing.id != workflow.id
                && existing.repository_path == workflow.repository_path
                && existing
                    .name
                    .trim()
                    .eq_ignore_ascii_case(workflow.name.trim())
        }) {
            anyhow::bail!(
                "A workflow named '{}' already exists for this repository",
                workflow.name.trim()
            );
        }
        Ok(())
    }

    /// Retains every actionable run and only the newest terminal history.
    pub fn prune_git_workflow_runs(&mut self, terminal_limit: usize) {
        let mut terminal: Vec<(usize, u64)> = self
            .git_workflow_runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.state.is_terminal())
            .map(|(index, run)| (index, run.updated_at))
            .collect();
        terminal.sort_by_key(|(_, updated_at)| std::cmp::Reverse(*updated_at));
        let retained: std::collections::HashSet<Uuid> = terminal
            .into_iter()
            .take(terminal_limit)
            .map(|(index, _)| self.git_workflow_runs[index].id)
            .collect();
        self.git_workflow_runs.retain(|run| {
            if !run.state.is_terminal() {
                return true;
            }
            retained.contains(&run.id)
        });
    }

    /// Replaces images saved by the retired arbitrary-image picker with the
    /// standard folder visual. New app-owned SVGs are explicitly marked in the
    /// `icon` field and survive this compatibility migration.
    pub fn migrate_legacy_icon_image(&mut self) -> bool {
        if self.icon_image_path.is_none() || self.icon == CUSTOM_PROJECT_SVG_ICON {
            return false;
        }
        self.icon_image_path = None;
        self.icon = default_project_icon();
        self.icon_color = default_project_icon_color();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_name_defaults_to_folder_name() {
        let project = Project::from_path(PathBuf::from("/tmp/app server"));
        assert_eq!(project.name, "app server");
        assert_eq!(project.icon, DEFAULT_PROJECT_ICON);
        assert_eq!(project.icon_color, DEFAULT_PROJECT_ICON_COLOR);
        assert_eq!(project.icon_image_path, None);
        assert_eq!(project.section_id, None);
        assert!(!project.is_favorite);
        assert!(project.presets.is_empty());
        assert!(project.task_tracker_connections.is_empty());
        assert!(project.git_workflows.is_empty());
        assert!(project.git_workflow_runs.is_empty());
    }

    #[test]
    fn project_visuals_serde_round_trip() {
        let mut project = Project::from_path(PathBuf::from("/tmp/app server"));
        project.icon = "star".to_string();
        project.icon_color = "green".to_string();
        project.icon_image_path = Some(PathBuf::from("/tmp/icon.png"));
        project.section_id = Some(ProjectSectionId::new());
        project.is_favorite = true;
        let json = serde_json::to_string(&project).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.icon, "star");
        assert_eq!(back.icon_color, "green");
        assert_eq!(back.icon_image_path, Some(PathBuf::from("/tmp/icon.png")));
        assert_eq!(back.section_id, project.section_id);
        assert!(back.is_favorite);
    }

    #[test]
    fn project_visuals_default_for_older_json() {
        let id = Uuid::new_v4();
        let json = format!(
            r#"{{
                "id": "{id}",
                "name": "old app",
                "path": "/tmp/old-app"
            }}"#
        );
        let project: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(project.icon, DEFAULT_PROJECT_ICON);
        assert_eq!(project.icon_color, DEFAULT_PROJECT_ICON_COLOR);
        assert_eq!(project.icon_image_path, None);
        assert_eq!(project.section_id, None);
        assert!(!project.is_favorite);
    }

    #[test]
    fn legacy_image_visual_migrates_to_default_folder() {
        let mut project = Project::from_path(PathBuf::from("/tmp/app server"));
        project.icon = "star".to_string();
        project.icon_color = "green".to_string();
        project.icon_image_path = Some(PathBuf::from("/tmp/icon.png"));

        assert!(project.migrate_legacy_icon_image());
        assert_eq!(project.icon, DEFAULT_PROJECT_ICON);
        assert_eq!(project.icon_color, DEFAULT_PROJECT_ICON_COLOR);
        assert_eq!(project.icon_image_path, None);
        assert!(!project.migrate_legacy_icon_image());
    }

    #[test]
    fn custom_svg_visual_survives_legacy_image_migration() {
        let mut project = Project::from_path(PathBuf::from("/tmp/app server"));
        project.icon = CUSTOM_PROJECT_SVG_ICON.to_string();
        project.icon_color = "green".to_string();
        project.icon_image_path = Some(PathBuf::from("/tmp/icon.svg"));

        assert!(!project.migrate_legacy_icon_image());
        assert_eq!(project.icon, CUSTOM_PROJECT_SVG_ICON);
        assert_eq!(project.icon_color, "green");
        assert_eq!(
            project.icon_image_path,
            Some(PathBuf::from("/tmp/icon.svg"))
        );
    }

    #[test]
    fn preset_serde_round_trip() {
        let preset = ScriptPreset::new("run server", "npm run dev");
        let json = serde_json::to_string(&preset).unwrap();
        let back: ScriptPreset = serde_json::from_str(&json).unwrap();
        assert_eq!(preset, back);
    }

    #[test]
    fn project_section_serde_round_trip() {
        let mut section = ProjectSection::new("Client Work");
        section.collapsed = true;
        let json = serde_json::to_string(&section).unwrap();
        let back: ProjectSection = serde_json::from_str(&json).unwrap();
        assert_eq!(section, back);
    }

    #[test]
    fn database_connection_debug_hides_uri_password() {
        let connection = DbConnection::new(
            "Production",
            "mongodb+srv://operator:literal-super-secret@cluster.example/app",
        );

        let debug = format!("{connection:?}");
        assert!(debug.contains("cluster.example"));
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("literal-super-secret"));
    }

    #[test]
    fn legacy_database_connection_defaults_to_mongodb() {
        let json = format!(
            r#"{{"id":"{}","name":"local","uri":"mongodb://localhost"}}"#,
            Uuid::new_v4()
        );
        let connection: DbConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(connection.provider, DbProvider::MongoDb);
        assert!(!connection.read_only);
    }

    #[test]
    fn supabase_remains_a_distinct_provider() {
        let connection = DbConnection::new_for(
            DbProvider::Supabase,
            "Supabase",
            "postgres://postgres.ref:${PASSWORD}@pooler.supabase.com:5432/postgres",
        );
        let json = serde_json::to_string(&connection).unwrap();
        let back: DbConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(back.provider, DbProvider::Supabase);
        assert!(back.provider.is_postgres());
        assert!(back.read_only);
    }

    #[test]
    fn git_workflow_enums_use_stable_snake_case_values() {
        assert_eq!(
            serde_json::to_string(&GitWorkflowCompletionPolicy::AutoMergeWhenReady).unwrap(),
            "\"auto_merge_when_ready\""
        );
        assert_eq!(
            serde_json::to_string(&GitWorkflowRunState::AwaitingConfirmation).unwrap(),
            "\"awaiting_confirmation\""
        );
        assert_eq!(
            serde_json::from_str::<GitWorkflowRunState>("\"needs_attention\"").unwrap(),
            GitWorkflowRunState::NeedsAttention
        );
    }

    #[test]
    fn legacy_project_json_defaults_git_workflow_collections() {
        let json = format!(
            r#"{{"id":"{}","name":"Legacy","path":"/tmp/legacy"}}"#,
            Uuid::new_v4()
        );
        let project: Project = serde_json::from_str(&json).unwrap();
        assert!(project.git_workflows.is_empty());
        assert!(project.git_workflow_runs.is_empty());
    }

    #[test]
    fn repository_relative_paths_cannot_escape_the_project() {
        assert!(validate_repository_relative_path(Path::new(".")).is_ok());
        assert!(validate_repository_relative_path(Path::new("apps/api")).is_ok());
        assert!(validate_repository_relative_path(Path::new("../api")).is_err());
        assert!(validate_repository_relative_path(Path::new("apps/../../api")).is_err());
        assert!(validate_repository_relative_path(Path::new("/tmp/api")).is_err());
    }

    #[test]
    fn workflow_names_are_unique_per_repository_case_insensitively() {
        let mut project = Project::from_path(PathBuf::from("/tmp/app"));
        let workflow = GitWorkflow::new(
            PathBuf::from("."),
            "  Promote  ",
            "staging",
            "production",
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        )
        .unwrap();
        assert_eq!(workflow.name, "Promote");
        project.git_workflows.push(workflow);
        let duplicate = GitWorkflow::new(
            PathBuf::from("."),
            "promote",
            "preview",
            "production",
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        )
        .unwrap();
        assert!(project.validate_git_workflow(&duplicate).is_err());

        let another_repo = GitWorkflow::new(
            PathBuf::from("apps/api"),
            "promote",
            "preview",
            "production",
            GitWorkflowCompletionPolicy::ConfirmBeforeMerge,
        )
        .unwrap();
        assert!(project.validate_git_workflow(&another_repo).is_ok());
    }

    #[test]
    fn run_pruning_keeps_active_and_newest_terminal_records() {
        let mut project = Project::from_path(PathBuf::from("/tmp/app"));
        for updated_at in 0..35 {
            project.git_workflow_runs.push(GitWorkflowRun {
                id: Uuid::new_v4(),
                workflow_id: None,
                repository_path: PathBuf::from("."),
                source_branch: "staging".into(),
                destination_branch: "production".into(),
                pull_request_number: None,
                expected_head_sha: None,
                state: GitWorkflowRunState::Merged,
                error: None,
                started_at: updated_at,
                updated_at,
            });
        }
        project.git_workflow_runs.push(GitWorkflowRun {
            id: Uuid::new_v4(),
            workflow_id: None,
            repository_path: PathBuf::from("."),
            source_branch: "staging".into(),
            destination_branch: "production".into(),
            pull_request_number: Some(42),
            expected_head_sha: None,
            state: GitWorkflowRunState::AwaitingConfirmation,
            error: None,
            started_at: 0,
            updated_at: 0,
        });

        project.prune_git_workflow_runs(30);
        assert_eq!(project.git_workflow_runs.len(), 31);
        assert!(project
            .git_workflow_runs
            .iter()
            .any(|run| run.state == GitWorkflowRunState::AwaitingConfirmation));
        assert!(project
            .git_workflow_runs
            .iter()
            .filter(|run| run.state == GitWorkflowRunState::Merged)
            .all(|run| run.updated_at >= 5));
    }
}
