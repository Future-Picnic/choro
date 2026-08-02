//! Local-first application storage backed by embedded Turso.
//!
//! This module intentionally uses `turso::Builder::new_local` only. The sync
//! feature is disabled in Cargo.toml, so the store never talks to Turso Cloud.

use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::runtime::Runtime;
use turso::{params, Builder, Connection, Value};
use uuid::Uuid;
use zip::{write::FileOptions, ZipArchive, ZipWriter};

use crate::agents::{AgentChangedFile, AgentRecord, AgentStoreFile, LaneProfile};
use crate::config::AppConfig;
use crate::git::{FileDiff, LineOrigin};
use crate::project::{
    DbConnection, Project, ProjectId, ProjectReference, ProjectReferenceKind, ProjectSection,
    ProjectSectionId, ScriptPreset,
};
use crate::task_tracker::{
    PersonalTaskComment, PersonalTaskPriority, PersonalTaskRecord, PersonalTaskStatus, TaskRef,
    TaskTrackerConnection,
};

const STORE_SCHEMA_VERSION: u32 = 22;
const EXPORT_FORMAT_VERSION: u32 = 4;
const DIFF_SNAPSHOT_MAX_LINES_PER_FILE: usize = 2_000;
const PROJECT_REFERENCE_PREVIEW_MAX_SIZE: u32 = 1200;

#[derive(Clone)]
pub struct LocalStore {
    root: PathBuf,
    db_path: PathBuf,
    rt: Arc<Runtime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredChatMessage {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub role: String,
    pub text: String,
    pub sequence: i64,
    pub created_at: u64,
    pub backend_message_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTimelineEvent {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub kind: String,
    #[serde(default)]
    pub event_key: Option<String>,
    pub payload_json: String,
    pub sequence: i64,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTimelinePage {
    /// Events are always returned in display order, oldest to newest.
    pub events: Vec<StoredTimelineEvent>,
    /// Sequence cursor to pass when loading the page immediately before this one.
    pub oldest_sequence: Option<i64>,
    pub has_more: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAttachment {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub message_id: Option<Uuid>,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
    pub sha256: String,
    pub relative_path: PathBuf,
    pub created_at: u64,
    pub state: String,
}

/// A session-transient web Preview request discoverable by every agent chat in
/// one project. Rows bridge the MCP helper process to the desktop and are
/// cleared whenever a new Choro desktop session starts.
///
/// Normal-agent sources remain project-wide. A Solo source id lets the UI
/// scope that preview to its owning worktree and agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProjectPreview {
    pub id: Uuid,
    pub project_id: ProjectId,
    pub url: String,
    pub title: String,
    pub source_agent_id: Option<Uuid>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// One remembered fact in Choro's cross-agent memory. `scope` is `"global"`
/// (about the user, all projects) or `"project"` (about one repo, with
/// `project_id` set). Rendered into a budgeted block at session start;
/// written by the `memory_save` MCP tool or by hand in Settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredMemory {
    pub id: Uuid,
    pub scope: String,
    pub project_id: Option<ProjectId>,
    pub text: String,
    pub enabled: bool,
    pub pinned: bool,
    pub source_agent_id: Option<Uuid>,
    pub created_at: u64,
    pub updated_at: u64,
    pub last_used_at: Option<u64>,
}

impl StoredMemory {
    pub fn is_global(&self) -> bool {
        self.scope == "global"
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentDiffSnapshot {
    pub id: Uuid,
    pub agent_id: Uuid,
    pub project_id: ProjectId,
    pub repo_path: PathBuf,
    pub source: String,
    pub base_sha: Option<String>,
    pub head_sha: Option<String>,
    pub commit_sha: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub state: String,
    pub files: Vec<StoredAgentDiffFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAgentDiffFile {
    pub snapshot_id: Uuid,
    pub path: PathBuf,
    pub additions: usize,
    pub deletions: usize,
    pub is_binary: bool,
    pub diff: FileDiff,
    pub truncated: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredAgentDiffSnapshotRow {
    id: Uuid,
    agent_id: Uuid,
    project_id: ProjectId,
    repo_path: PathBuf,
    source: String,
    base_sha: Option<String>,
    head_sha: Option<String>,
    commit_sha: Option<String>,
    created_at: u64,
    updated_at: u64,
    state: String,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DiffSnapshotBackfillCounts {
    pub scanned_events: usize,
    pub backfilled_events: usize,
    pub worktree_matches: usize,
    pub commit_matches: usize,
}

#[derive(Debug, Deserialize)]
struct BackfillChangedFilesPayload {
    #[serde(default)]
    files: Vec<BackfillFileChange>,
    #[serde(default)]
    snapshot_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize)]
struct BackfillFileChange {
    path: PathBuf,
    #[serde(default)]
    additions: usize,
    #[serde(default)]
    deletions: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportManifest {
    format_version: u32,
    app: String,
    exported_at: u64,
    counts: ExportCounts,
    checksums: Vec<ExportChecksum>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct ExportCounts {
    projects: usize,
    #[serde(default)]
    project_references: usize,
    #[serde(default)]
    personal_tasks: usize,
    agents: usize,
    messages: usize,
    timeline_events: usize,
    #[serde(default)]
    memories: usize,
    attachments: usize,
    #[serde(default)]
    diff_snapshots: usize,
    #[serde(default)]
    diff_files: usize,
    #[serde(default)]
    penpot_connections: usize,
    #[serde(default)]
    penpot_bindings: usize,
    #[serde(default)]
    penpot_designs: usize,
    #[serde(default)]
    penpot_conversations: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportChecksum {
    path: String,
    sha256: String,
}

mod agents;
mod api;
mod archive;
mod chat;
mod diffs;
mod maintenance;
mod memories;
mod penpot;
mod project_preview;
mod references;
mod schema;
mod support;
mod tasks;
mod workspace;

use agents::*;
use archive::*;
use chat::*;
use diffs::*;
pub use memories::MAX_MEMORY_TEXT_CHARS;
use memories::*;
use penpot::*;
pub use penpot::{
    StoredPenpotConnection, StoredPenpotDesign, StoredPenpotDesignConversation,
    StoredProjectPenpotBinding,
};
use project_preview::*;
use references::*;
use schema::*;
use support::*;
use tasks::*;
use workspace::*;

#[cfg(test)]
mod tests;
