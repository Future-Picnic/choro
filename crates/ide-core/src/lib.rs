pub mod agents;
pub mod blocking_guard;
pub mod branding;
pub mod config;
pub mod db;
mod db_sql;
pub mod doc_assistant;
pub mod env;
pub mod git;
pub mod lanes;
pub mod local_store;
pub mod memory;
pub mod penpot_assistant;
pub mod preview_control;
pub mod project;
pub mod redaction;
pub mod search;
pub mod services;
pub mod task_tracker;
pub mod visual_review;
pub mod watcher;

pub use agents::{
    AgentAccessMode, AgentChangedFile, AgentChat, AgentDesignContext, AgentEffort, AgentKind,
    AgentModel, AgentRecord, AgentRuntimeKind, AgentStatus, AgentStoreFile, LaneProfile,
};
pub use blocking_guard::mark_ui_thread;
pub use branding::{APP_ID, APP_NAME, DOCS_DIR_NAME};
pub use config::{
    AppConfig, CompletionNotifications, NotificationSettings, VoiceAnnouncements, VoiceSettings,
};
pub use db::{
    DatabaseHandle, DbObject, DbObjectKind, DocEntry, DocPage, MongoHandle, TableColumn,
    TableFilter, TablePage, TableRow,
};
pub use db_sql::SqlHandle;
pub use doc_assistant::{
    DocAssistantMessage, DocAssistantRecord, DocAssistantRole, DocAssistantStoreFile,
    DocAssistantTranscriptMessage,
};
pub use project::{
    validate_repository_relative_path, DbConnection, DbProvider, GitWorkflow,
    GitWorkflowCompletionPolicy, GitWorkflowRun, GitWorkflowRunState, Project, ProjectId,
    ProjectReference, ProjectReferenceKind, ProjectSection, ProjectSectionId, ScriptPreset,
    CUSTOM_PROJECT_SVG_ICON,
};
pub use redaction::{redact_sensitive_text, redact_sensitive_text_with};
pub use services::{
    detect_project_services, read_env_file, read_sub_app_env, set_env_value, DetectedService,
    EnvEntry, EnvFile, ServiceCategory, ServiceFact, SubAppServices,
};
pub use task_tracker::{
    IssueTrackerProvider, JiraBoard, JiraClient, JiraUser, PersonalTaskComment,
    PersonalTaskPriority, PersonalTaskRecord, PersonalTaskStatus, TaskAttachment, TaskBoard,
    TaskBoardColumn, TaskComment, TaskContentBlock, TaskDetail, TaskInlineImage, TaskRef,
    TaskRichText, TaskStatusOption, TaskSummary, TaskTrackerClient, TaskTrackerConnection,
    TaskTrackerSource, TaskTrackerUser,
};
pub use watcher::{GitWatcher, WorktreeWatcher};
