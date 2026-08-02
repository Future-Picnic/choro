use std::path::PathBuf;

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
        }
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
}
