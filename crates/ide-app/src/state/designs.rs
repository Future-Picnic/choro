use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use gpui::{AppContext, Context, Entity, EventEmitter};
use ide_core::local_store::LocalStore;
use ide_core::{ProjectId, ProjectReference, ProjectReferenceKind};
use uuid::Uuid;

use super::{Workspace, WorkspaceEvent};

pub enum DesignsEvent {
    SelectionChanged,
}

pub struct DesignsState {
    references: HashMap<ProjectId, Vec<ProjectReference>>,
    selected: HashMap<ProjectId, Uuid>,
    /// Last time we re-read a project's references from disk, for the throttled
    /// [`poll_refresh`](Self::poll_refresh) that catches externally-added assets
    /// (e.g. saved by an agent via the MCP `save_asset` tool).
    refresh_stamps: HashMap<ProjectId, Instant>,
    refresh_in_flight: HashSet<ProjectId>,
}

impl EventEmitter<DesignsEvent> for DesignsState {}

impl DesignsState {
    pub fn view(workspace: Entity<Workspace>, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx| {
            let mut state = Self {
                references: HashMap::new(),
                selected: HashMap::new(),
                refresh_stamps: HashMap::new(),
                refresh_in_flight: HashSet::new(),
            };
            let project_ids = workspace
                .read(cx)
                .projects
                .iter()
                .map(|project| project.id)
                .collect::<Vec<_>>();
            for project_id in project_ids {
                state.poll_refresh(project_id, cx);
            }
            cx.subscribe(
                &workspace,
                |this: &mut Self, workspace, event: &WorkspaceEvent, cx| {
                    if matches!(event, WorkspaceEvent::ProjectsChanged) {
                        let project_ids = workspace
                            .read(cx)
                            .projects
                            .iter()
                            .map(|project| project.id)
                            .collect::<Vec<_>>();
                        for project_id in project_ids {
                            this.refresh_stamps.remove(&project_id);
                            this.poll_refresh(project_id, cx);
                        }
                        cx.notify();
                    }
                },
            )
            .detach();
            state
        })
    }

    /// Re-read a project's references off the GPUI thread if we haven't in a
    /// couple of seconds. It remains safe to call from render because render
    /// only schedules work and never waits for storage.
    pub fn poll_refresh(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        let now = Instant::now();
        let fresh = self
            .refresh_stamps
            .get(&project)
            .is_none_or(|last| now.duration_since(*last) > Duration::from_secs(2));
        if !fresh || self.refresh_in_flight.contains(&project) {
            return;
        }
        self.refresh_stamps.insert(project, now);
        self.refresh_in_flight.insert(project);
        cx.spawn(async move |this, cx| {
            let references = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default()
                        .and_then(|store| store.load_project_references(project))
                })
                .await;
            this.update(cx, |this, cx| {
                this.refresh_in_flight.remove(&project);
                let Ok(references) = references else {
                    return;
                };
                if this.references.get(&project) == Some(&references) {
                    return;
                }
                this.apply_project_references(project, references);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn references_for_project(&self, project: ProjectId) -> Vec<ProjectReference> {
        self.references.get(&project).cloned().unwrap_or_default()
    }

    pub fn reference(&self, project: ProjectId, reference_id: Uuid) -> Option<ProjectReference> {
        self.references
            .get(&project)?
            .iter()
            .find(|reference| reference.id == reference_id)
            .cloned()
    }

    pub fn design_hub_references(&self, project: ProjectId) -> Vec<ProjectReference> {
        self.references
            .get(&project)
            .into_iter()
            .flatten()
            .filter(|reference| reference_in_design_hub(reference))
            .cloned()
            .collect()
    }

    pub fn selected_reference(&self, project: ProjectId) -> Option<ProjectReference> {
        let selected = self.selected.get(&project)?;
        self.references
            .get(&project)?
            .iter()
            .find(|reference| reference.id == *selected)
            .cloned()
    }

    pub fn select(&mut self, project: ProjectId, reference_id: Uuid, cx: &mut Context<Self>) {
        self.selected.insert(project, reference_id);
        cx.emit(DesignsEvent::SelectionChanged);
        cx.notify();
    }

    pub fn create_image_reference(
        &mut self,
        project: ProjectId,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<ProjectReference> {
        let title = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or("Image reference")
            .to_string();
        let store = LocalStore::open_default()?;
        let reference = store.create_project_reference(
            project,
            ProjectReferenceKind::Image,
            title,
            path.to_string_lossy().to_string(),
            "",
            Some(path),
        )?;
        self.reload_project(project);
        self.select(project, reference.id, cx);
        Ok(reference)
    }

    pub fn create_file_reference(
        &mut self,
        project: ProjectId,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<ProjectReference> {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("File reference")
            .to_string();
        let store = LocalStore::open_default()?;
        // The store copies the source file into the project's assets and
        // rewrites `source` to the copied `data/…` path for File kind.
        let reference = store.create_project_reference(
            project,
            ProjectReferenceKind::File,
            title,
            path.to_string_lossy().to_string(),
            "",
            None,
        )?;
        self.reload_project(project);
        self.select(project, reference.id, cx);
        Ok(reference)
    }

    pub fn create_image_reference_bytes(
        &mut self,
        project: ProjectId,
        title: impl Into<String>,
        extension: &str,
        bytes: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<ProjectReference> {
        let store = LocalStore::open_default()?;
        let reference =
            store.create_project_image_reference_bytes(project, title, "", extension, bytes)?;
        self.reload_project(project);
        self.select(project, reference.id, cx);
        Ok(reference)
    }

    pub fn create_source_reference(
        &mut self,
        project: ProjectId,
        kind: ProjectReferenceKind,
        title: impl Into<String>,
        source: impl Into<String>,
        notes: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Result<ProjectReference> {
        let store = LocalStore::open_default()?;
        let reference =
            store.create_project_reference(project, kind, title, source, notes, None)?;
        self.reload_project(project);
        self.select(project, reference.id, cx);
        Ok(reference)
    }

    pub fn create_figma_design_reference(
        &mut self,
        project: ProjectId,
        title: impl Into<String>,
        source: impl Into<String>,
        cx: &mut Context<Self>,
    ) -> Result<ProjectReference> {
        let mut reference = self.create_source_reference(
            project,
            ProjectReferenceKind::Figma,
            title,
            source,
            "",
            cx,
        )?;
        reference.metadata_json = merge_design_hub(&reference.metadata_json, true);
        reference.updated_at = unix_now();
        LocalStore::open_default()?.upsert_project_reference(&reference)?;
        self.reload_project(project);
        self.select(project, reference.id, cx);
        Ok(reference)
    }

    pub fn delete_reference(
        &mut self,
        project: ProjectId,
        reference_id: Uuid,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let Some(reference) = self
            .references
            .get(&project)
            .and_then(|references| {
                references
                    .iter()
                    .find(|reference| reference.id == reference_id)
            })
            .cloned()
        else {
            return Ok(());
        };
        LocalStore::open_default()?.delete_project_reference(&reference)?;
        self.reload_project(project);
        cx.emit(DesignsEvent::SelectionChanged);
        cx.notify();
        Ok(())
    }

    pub fn update_reference(
        &mut self,
        mut reference: ProjectReference,
        title: impl Into<String>,
        source: impl Into<String>,
        notes: impl Into<String>,
        folder: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<ProjectReference> {
        let project = reference.project_id;
        reference.title = title.into();
        reference.source = source.into();
        reference.notes = notes.into();
        reference.metadata_json = merge_folder(&reference.metadata_json, folder);
        reference.updated_at = unix_now();
        LocalStore::open_default()?.upsert_project_reference(&reference)?;
        self.reload_project(project);
        self.select(project, reference.id, cx);
        Ok(reference)
    }

    /// Move a reference into a folder (or back to the root with `None`). Folder
    /// membership lives in `metadata_json` so it needs no schema change.
    pub fn set_reference_folder(
        &mut self,
        mut reference: ProjectReference,
        folder: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let project = reference.project_id;
        reference.metadata_json = merge_folder(&reference.metadata_json, folder);
        reference.updated_at = unix_now();
        LocalStore::open_default()?.upsert_project_reference(&reference)?;
        self.reload_project(project);
        cx.notify();
        Ok(())
    }

    fn reload_project(&mut self, project: ProjectId) {
        let references = LocalStore::open_default()
            .and_then(|store| store.load_project_references(project))
            .unwrap_or_default();
        self.apply_project_references(project, references);
    }

    fn apply_project_references(&mut self, project: ProjectId, references: Vec<ProjectReference>) {
        let selected_valid = self
            .selected
            .get(&project)
            .is_some_and(|selected| references.iter().any(|reference| reference.id == *selected));
        if !selected_valid {
            if let Some(first) = references.first() {
                self.selected.insert(project, first.id);
            } else {
                self.selected.remove(&project);
            }
        }
        self.references.insert(project, references);
    }
}

/// The folder a reference belongs to (from `metadata_json`), or `None` for the
/// root level.
pub fn reference_folder(reference: &ProjectReference) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(&reference.metadata_json)
        .ok()
        .as_ref()
        .and_then(|value| value.get("folder"))
        .and_then(|folder| folder.as_str())
        .map(str::trim)
        .filter(|folder| !folder.is_empty())
        .map(str::to_string)
}

pub fn reference_in_design_hub(reference: &ProjectReference) -> bool {
    reference.kind == ProjectReferenceKind::Figma
        && serde_json::from_str::<serde_json::Value>(&reference.metadata_json)
            .ok()
            .as_ref()
            .and_then(|value| value.get("design_hub"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
}

/// Merge a folder assignment into a reference's `metadata_json`, preserving any
/// other keys. `None` clears the folder (moves to root).
fn merge_folder(metadata_json: &str, folder: Option<String>) -> String {
    let mut object = serde_json::from_str::<serde_json::Value>(metadata_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    match folder
        .map(|folder| folder.trim().to_string())
        .filter(|f| !f.is_empty())
    {
        Some(folder) => {
            object.insert("folder".to_string(), serde_json::Value::String(folder));
        }
        None => {
            object.remove("folder");
        }
    }
    serde_json::to_string(&serde_json::Value::Object(object)).unwrap_or_else(|_| "{}".to_string())
}

fn merge_design_hub(metadata_json: &str, included: bool) -> String {
    let mut object = serde_json::from_str::<serde_json::Value>(metadata_json)
        .ok()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if included {
        object.insert("design_hub".to_string(), serde_json::Value::Bool(true));
    } else {
        object.remove("design_hub");
    }
    serde_json::to_string(&serde_json::Value::Object(object)).unwrap_or_else(|_| "{}".to_string())
}

pub fn reference_absolute_preview_path(reference: &ProjectReference) -> Option<PathBuf> {
    let relative = reference.preview_relative_path.as_ref()?;
    LocalStore::open_default()
        .ok()
        .map(|store| store.root().join(relative))
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(kind: ProjectReferenceKind, metadata_json: &str) -> ProjectReference {
        ProjectReference {
            id: Uuid::new_v4(),
            project_id: ProjectId::new(),
            kind,
            title: "Example".to_string(),
            source: "https://www.figma.com/design/example".to_string(),
            preview_relative_path: None,
            notes: String::new(),
            metadata_json: metadata_json.to_string(),
            sort_order: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn design_hub_only_includes_explicit_figma_references() {
        assert!(reference_in_design_hub(&reference(
            ProjectReferenceKind::Figma,
            r#"{"design_hub":true}"#,
        )));
        assert!(!reference_in_design_hub(&reference(
            ProjectReferenceKind::Figma,
            "{}",
        )));
        assert!(!reference_in_design_hub(&reference(
            ProjectReferenceKind::Url,
            r#"{"design_hub":true}"#,
        )));
    }

    #[test]
    fn design_hub_metadata_preserves_existing_values() {
        let metadata = merge_design_hub(r#"{"folder":"References"}"#, true);
        let value: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(value["folder"], "References");
        assert_eq!(value["design_hub"], true);
    }
}
