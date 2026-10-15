//! Read-only compatibility types for historical isolated assignments.
//! No project capture, cloning, materialization, or integration is available.
use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileKind {
    Regular,
    Symlink,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub hash: String,
    pub kind: FileKind,
    pub executable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub id: Uuid,
    pub source: PathBuf,
    pub storage: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub index_hash: Option<String>,
    pub files: BTreeMap<PathBuf, SnapshotFile>,
    pub excluded: Vec<PathBuf>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrationChange {
    pub path: PathBuf,
    pub before: Option<SnapshotFile>,
    pub after: Option<SnapshotFile>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrationStatus {
    Prepared,
    Conflict,
    Applying,
    Applied,
    NeedsAttention,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntegrationOperation {
    pub id: Uuid,
    pub source: PathBuf,
    pub storage: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub index_hash: Option<String>,
    pub changes: Vec<IntegrationChange>,
    pub conflicts: Vec<PathBuf>,
    pub resolution_dir: PathBuf,
    pub applied_paths: Vec<PathBuf>,
    pub status: IntegrationStatus,
    #[serde(default)]
    pub archive_snapshot: Option<WorkspaceSnapshot>,
}
