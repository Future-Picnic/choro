#![allow(dead_code, reason = "retained document linking API")]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context as _, Result};
use gpui::{App, AppContext, Context, Entity, EventEmitter};
use ide_core::branding::{migrate_legacy_doc_path, LEGACY_DOCS_DIR_NAME};
use ide_core::{AgentStatus, Project, ProjectId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use velotype::Editor as VelotypeEditor;

use crate::state::Workspace;

pub use ide_core::DOCS_DIR_NAME;

const DOCS_METADATA_FILE: &str = ".metadata.json";
const DOC_TEMPLATES_DIR: &str = ".templates";
const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
const MAX_CHORO_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;
const DEFAULT_DOC_LABELS: [&str; 3] = ["feature", "research", "design"];

pub enum DocsEvent {
    Changed,
    SelectionChanged,
    /// A reference embed / `ref:` link inside the doc `doc_path` was activated.
    /// `target` is velotype's opaque routing string (e.g. `ref:file:src/x.rs`).
    OpenReference {
        doc_path: PathBuf,
        target: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocEntry {
    pub project: ProjectId,
    pub project_name: String,
    pub project_path: PathBuf,
    pub path: PathBuf,
    pub relative_path: PathBuf,
    pub title: String,
    pub modified: Option<SystemTime>,
    pub is_template: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocTemplateSource {
    BuiltIn(&'static str),
    Project(PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocTemplateOption {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_label: Option<String>,
    pub source: DocTemplateSource,
}

struct BuiltInDocTemplate {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    default_label: Option<&'static str>,
    document: ChoroDocument,
}

/// Versioned, human-readable document envelope persisted as `*.choro` JSON.
/// BlockNote owns the block payload; Choro owns the file, title, versioning,
/// autosave, asset directory, and external-writer synchronization.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChoroDocument {
    pub version: u32,
    pub format: String,
    pub title: String,
    pub blocks: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<ChoroDocumentOrigin>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChoroDocumentOrigin {
    PocketComet {
        workspace_id: String,
        project_id: String,
        document_id: String,
    },
}

impl ChoroDocumentOrigin {
    pub fn pocketcomet_document_id(&self) -> &str {
        match self {
            Self::PocketComet { document_id, .. } => document_id,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncedChoroDocument {
    pub entry: DocEntry,
    pub document: ChoroDocument,
    pub revision: String,
}

impl ChoroDocument {
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            anyhow::bail!("unsupported Choro document version {}", self.version);
        }
        if self.format != "blocknote" {
            anyhow::bail!("unsupported Choro document format {}", self.format);
        }
        if self.title.len() > 4_096 {
            anyhow::bail!("Choro document title is too long");
        }
        for (index, block) in self.blocks.iter().enumerate() {
            let block_type = block
                .as_object()
                .and_then(|block| block.get("type"))
                .and_then(serde_json::Value::as_str)
                .filter(|block_type| !block_type.trim().is_empty());
            if block_type.is_none() {
                anyhow::bail!("Choro document block {index} has no valid type");
            }
        }
        if let Some(ChoroDocumentOrigin::PocketComet {
            workspace_id,
            project_id,
            document_id,
        }) = self.origin.as_ref()
        {
            for (label, value) in [
                ("workspace", workspace_id),
                ("project", project_id),
                ("document", document_id),
            ] {
                if value.trim().is_empty() || value.len() > 500 {
                    anyhow::bail!("PocketComet {label} identity is invalid");
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocSaveStatus {
    Saved,
    Dirty,
    Saving,
    Error(String),
}

pub struct DocsState {
    workspace: Entity<Workspace>,
    docs: HashMap<ProjectId, Vec<DocEntry>>,
    metadata: HashMap<ProjectId, DocsMetadataFile>,
    selected: HashMap<ProjectId, PathBuf>,
    editors: HashMap<PathBuf, Entity<VelotypeEditor>>,
    web_documents: HashMap<PathBuf, ChoroDocument>,
    web_saved_documents: HashMap<PathBuf, ChoroDocument>,
    web_dirty: HashSet<PathBuf>,
    editor_modified: HashMap<PathBuf, Option<SystemTime>>,
    save_generation: HashMap<PathBuf, u64>,
    save_status: HashMap<PathBuf, DocSaveStatus>,
    refresh_generation: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DocsMetadataFile {
    #[serde(default = "default_doc_labels")]
    labels: Vec<String>,
    #[serde(default)]
    doc_labels: HashMap<String, String>,
    #[serde(default)]
    doc_statuses: HashMap<String, AgentStatus>,
    #[serde(default)]
    doc_implementors: HashMap<String, Uuid>,
}

impl Default for DocsMetadataFile {
    fn default() -> Self {
        Self {
            labels: default_doc_labels(),
            doc_labels: HashMap::new(),
            doc_statuses: HashMap::new(),
            doc_implementors: HashMap::new(),
        }
    }
}

impl EventEmitter<DocsEvent> for DocsState {}

impl DocsState {
    pub fn view(workspace: Entity<Workspace>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |this: &mut Self, _, cx| {
                this.refresh(cx);
            })
            .detach();
            let mut state = Self {
                workspace,
                docs: HashMap::new(),
                metadata: HashMap::new(),
                selected: HashMap::new(),
                editors: HashMap::new(),
                web_documents: HashMap::new(),
                web_saved_documents: HashMap::new(),
                web_dirty: HashSet::new(),
                editor_modified: HashMap::new(),
                save_generation: HashMap::new(),
                save_status: HashMap::new(),
                refresh_generation: 0,
            };
            state.refresh(cx);
            state
        })
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let projects = self.workspace.read(cx).projects.clone();
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        let generation = self.refresh_generation;
        cx.spawn(async move |this, cx| {
            let snapshot = cx
                .background_executor()
                .spawn(async move {
                    projects
                        .into_iter()
                        .map(|project| {
                            let docs = scan_project_docs(&project);
                            let metadata = load_docs_metadata(&project.path);
                            (project.id, docs, metadata)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.refresh_generation != generation {
                    return;
                }
                let project_ids = snapshot
                    .iter()
                    .map(|(project, _, _)| *project)
                    .collect::<Vec<_>>();
                let mut next_docs = HashMap::new();
                let mut next_metadata = HashMap::new();
                for (project, docs, metadata) in snapshot {
                    if this
                        .selected
                        .get(&project)
                        .is_some_and(|selected| !docs.iter().any(|doc| doc.path == *selected))
                    {
                        this.selected.remove(&project);
                    }
                    if !this.selected.contains_key(&project) {
                        if let Some(first) = docs
                            .iter()
                            .find(|doc| !doc.is_template)
                            .or_else(|| docs.first())
                        {
                            this.selected.insert(project, first.path.clone());
                        }
                    }
                    next_docs.insert(project, docs);
                    next_metadata.insert(project, metadata);
                }
                this.selected
                    .retain(|project, _| project_ids.contains(project));
                this.docs = next_docs;
                this.metadata = next_metadata;
                cx.emit(DocsEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }

    pub fn docs_for_project(&self, project: ProjectId) -> Vec<DocEntry> {
        self.docs
            .get(&project)
            .into_iter()
            .flatten()
            .filter(|doc| !doc.is_template)
            .cloned()
            .collect()
    }

    pub fn templates_for_project(&self, project: ProjectId) -> Vec<DocEntry> {
        self.docs
            .get(&project)
            .into_iter()
            .flatten()
            .filter(|doc| doc.is_template)
            .cloned()
            .collect()
    }

    pub fn doc_template_options(&self, _project: ProjectId) -> Vec<DocTemplateOption> {
        built_in_doc_templates()
            .into_iter()
            .map(|template| DocTemplateOption {
                id: template.id.to_string(),
                name: template.name.to_string(),
                description: template.description.to_string(),
                default_label: template.default_label.map(str::to_string),
                source: DocTemplateSource::BuiltIn(template.id),
            })
            .collect()
    }

    pub fn selected_path(&self, project: ProjectId) -> Option<PathBuf> {
        self.selected.get(&project).cloned()
    }

    pub fn selected_doc(&self, project: ProjectId) -> Option<DocEntry> {
        let selected = self.selected.get(&project)?;
        self.docs
            .get(&project)?
            .iter()
            .find(|doc| doc.path == *selected)
            .cloned()
    }

    pub fn labels_for_project(&self, project: ProjectId) -> Vec<String> {
        self.metadata
            .get(&project)
            .map(|metadata| metadata.labels.clone())
            .unwrap_or_else(default_doc_labels)
    }

    pub fn doc_label(&self, project: ProjectId, relative_path: &Path) -> Option<String> {
        let key = doc_metadata_key(relative_path);
        self.metadata
            .get(&project)
            .and_then(|metadata| metadata.doc_labels.get(&key))
            .cloned()
            .filter(|label| !label.trim().is_empty())
    }

    pub fn doc_implementor(&self, project: ProjectId, relative_path: &Path) -> Option<Uuid> {
        let key = doc_metadata_key(relative_path);
        self.metadata
            .get(&project)
            .and_then(|metadata| metadata.doc_implementors.get(&key))
            .copied()
    }

    pub fn doc_status(&self, project: ProjectId, relative_path: &Path) -> AgentStatus {
        let key = doc_metadata_key(relative_path);
        self.metadata
            .get(&project)
            .and_then(|metadata| metadata.doc_statuses.get(&key))
            .copied()
            .unwrap_or(AgentStatus::Todo)
    }

    pub fn set_doc_label(
        &mut self,
        project: ProjectId,
        relative_path: &Path,
        label: Option<String>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let metadata = self
            .metadata
            .entry(project)
            .or_insert_with(|| load_docs_metadata(&project_info.path));
        let key = doc_metadata_key(relative_path);
        match label.and_then(|label| clean_doc_label(&label)) {
            Some(label) => {
                ensure_doc_label(metadata, &label);
                metadata.doc_labels.insert(key, label);
            }
            None => {
                metadata.doc_labels.remove(&key);
            }
        }
        save_docs_metadata(&project_info.path, metadata)?;
        cx.emit(DocsEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn set_doc_implementor(
        &mut self,
        project: ProjectId,
        relative_path: &Path,
        implementor: Option<Uuid>,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let metadata = self
            .metadata
            .entry(project)
            .or_insert_with(|| load_docs_metadata(&project_info.path));
        let key = doc_metadata_key(relative_path);
        match implementor {
            Some(implementor) => {
                metadata.doc_implementors.insert(key, implementor);
            }
            None => {
                metadata.doc_implementors.remove(&key);
            }
        }
        save_docs_metadata(&project_info.path, metadata)?;
        cx.emit(DocsEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn set_doc_status(
        &mut self,
        project: ProjectId,
        relative_path: &Path,
        status: AgentStatus,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let metadata = self
            .metadata
            .entry(project)
            .or_insert_with(|| load_docs_metadata(&project_info.path));
        metadata
            .doc_statuses
            .insert(doc_metadata_key(relative_path), status);
        save_docs_metadata(&project_info.path, metadata)?;
        cx.emit(DocsEvent::Changed);
        cx.notify();
        Ok(())
    }

    pub fn save_status(&self, path: &Path, cx: &App) -> DocSaveStatus {
        if let Some(status) = self.save_status.get(path) {
            return status.clone();
        }
        if self.web_dirty.contains(path) {
            DocSaveStatus::Dirty
        } else if self
            .editors
            .get(path)
            .is_some_and(|editor| editor.read(cx).embedded_is_dirty())
        {
            DocSaveStatus::Dirty
        } else {
            DocSaveStatus::Saved
        }
    }

    pub fn select_doc(&mut self, project: ProjectId, path: PathBuf, cx: &mut Context<Self>) {
        if self.selected.get(&project) == Some(&path) {
            return;
        }
        self.selected.insert(project, path);
        cx.emit(DocsEvent::SelectionChanged);
        cx.notify();
    }

    pub fn create_doc(&mut self, project: ProjectId, cx: &mut Context<Self>) -> Result<PathBuf> {
        self.create_doc_from_template(project, &DocTemplateSource::BuiltIn("feature-prd"), cx)
    }

    pub fn create_doc_from_template(
        &mut self,
        project: ProjectId,
        source: &DocTemplateSource,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf> {
        let project = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let docs_dir = docs_dir(&project.path);
        fs::create_dir_all(&docs_dir).context("failed to create docs directory")?;
        let path = unique_doc_path(&docs_dir, "untitled");
        let (mut document, default_label, asset_source) = match source {
            DocTemplateSource::BuiltIn(id) => {
                let template = built_in_doc_template(id).context("document template not found")?;
                (
                    template.document,
                    template.default_label.map(str::to_string),
                    None,
                )
            }
            DocTemplateSource::Project(template_path) => (
                self.web_documents
                    .get(template_path)
                    .cloned()
                    .map(Ok)
                    .unwrap_or_else(|| read_choro_document(template_path))?,
                None,
                Some(template_path.clone()),
            ),
        };
        // A newly-created document has not been named by the user yet, even
        // when its starter template has a descriptive template title. Keeping
        // the envelope title as Untitled lets the Doc Assistant replace it
        // with a topic-specific title on its first document edit.
        document.title = "Untitled".to_string();
        create_choro_document_with_assets(&path, &document, asset_source.as_deref())
            .context("failed to create doc")?;
        self.web_documents.insert(path.clone(), document.clone());
        self.web_saved_documents.insert(path.clone(), document);
        self.editor_modified
            .insert(path.clone(), file_modified(&path));
        self.refresh(cx);
        self.selected.insert(project.id, path.clone());
        self.save_status.insert(path.clone(), DocSaveStatus::Saved);
        if let Some(label) = default_label {
            let relative = project_relative_doc_path(&project.path, &path)
                .context("created doc is outside the project")?;
            let metadata = self
                .metadata
                .entry(project.id)
                .or_insert_with(|| load_docs_metadata(&project.path));
            ensure_doc_label(metadata, &label);
            metadata
                .doc_labels
                .insert(doc_metadata_key(&relative), label);
            save_docs_metadata(&project.path, metadata)?;
        }
        cx.emit(DocsEvent::SelectionChanged);
        cx.notify();
        Ok(path)
    }

    pub fn create_project_template_from_doc(
        &mut self,
        project: ProjectId,
        path: &Path,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf> {
        let name = name.trim();
        if name.is_empty() {
            anyhow::bail!("template name cannot be empty");
        }
        let document = self.web_document_for_path(path)?;
        self.write_project_template(project, name, document, Some(path), cx)
    }

    fn write_project_template(
        &mut self,
        project: ProjectId,
        name: &str,
        mut document: ChoroDocument,
        asset_source: Option<&Path>,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf> {
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let templates_dir = docs_dir(&project_info.path).join(DOC_TEMPLATES_DIR);
        fs::create_dir_all(&templates_dir).context("failed to create templates directory")?;
        let path = unique_doc_path(&templates_dir, name);
        document.title = name.trim().to_string();
        create_choro_document_with_assets(&path, &document, asset_source)
            .context("failed to create project template")?;
        self.web_documents.insert(path.clone(), document.clone());
        self.web_saved_documents.insert(path.clone(), document);
        self.editor_modified
            .insert(path.clone(), file_modified(&path));
        self.refresh(cx);
        self.save_status.insert(path.clone(), DocSaveStatus::Saved);
        cx.emit(DocsEvent::SelectionChanged);
        cx.notify();
        Ok(path)
    }

    pub fn rename_doc_to_title(
        &mut self,
        project: ProjectId,
        path: &Path,
        title: &str,
        cx: &mut Context<Self>,
    ) -> Result<PathBuf> {
        let clean_title = title.trim();
        if clean_title.is_empty() {
            anyhow::bail!("doc title cannot be empty");
        }
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let docs_dir = docs_dir(&project_info.path);
        if !path.starts_with(&docs_dir) {
            anyhow::bail!("doc is outside docs directory");
        }
        let stem = slug_for_title(clean_title);
        let is_template = project_relative_doc_path(&project_info.path, path)
            .is_some_and(|relative| is_project_template_path(&relative));
        let target_dir = if is_template {
            path.parent().unwrap_or(&docs_dir)
        } else {
            &docs_dir
        };
        let next_path = unique_doc_path_excluding(target_dir, &stem, Some(path));
        if path == next_path {
            return Ok(path.to_path_buf());
        }
        if self.web_dirty.contains(path) {
            self.persist_web_document_now(path)
                .context("failed to save document before rename")?;
        }
        let previous_relative = project_relative_doc_path(&project_info.path, path);
        fs::rename(path, &next_path).context("failed to rename doc")?;
        let previous_assets = path.with_extension("assets");
        let next_assets = next_path.with_extension("assets");
        if previous_assets.is_dir() {
            if let Err(error) = fs::rename(&previous_assets, &next_assets) {
                let _ = fs::rename(&next_path, path);
                return Err(error).context("failed to rename doc assets");
            }
        }
        for (previous, next) in [
            (choro_backup_path(path), choro_backup_path(&next_path)),
            (choro_recovery_path(path), choro_recovery_path(&next_path)),
        ] {
            if previous.is_file() {
                let _ = fs::rename(previous, next);
            }
        }
        if let Some(previous_relative) = previous_relative {
            if let Some(next_relative) = project_relative_doc_path(&project_info.path, &next_path) {
                if let Some(metadata) = self.metadata.get_mut(&project) {
                    if move_doc_metadata(metadata, &previous_relative, &next_relative) {
                        save_docs_metadata(&project_info.path, metadata)?;
                    }
                }
            }
        }
        if let Some(editor) = self.editors.remove(path) {
            editor.update(cx, |editor, cx| {
                editor.embedded_mark_saved(next_path.clone(), cx);
            });
            self.editors.insert(next_path.clone(), editor);
        }
        if let Some(document) = self.web_documents.remove(path) {
            self.web_documents.insert(next_path.clone(), document);
        }
        if let Some(document) = self.web_saved_documents.remove(path) {
            self.web_saved_documents.insert(next_path.clone(), document);
        }
        if self.web_dirty.remove(path) {
            self.web_dirty.insert(next_path.clone());
        }
        if let Some(modified) = self.editor_modified.remove(path) {
            self.editor_modified.insert(next_path.clone(), modified);
        }
        if let Some(status) = self.save_status.remove(path) {
            self.save_status.insert(next_path.clone(), status);
        }
        if let Some(generation) = self.save_generation.remove(path) {
            self.save_generation.insert(next_path.clone(), generation);
        }
        self.refresh(cx);
        self.selected.insert(project, next_path.clone());
        cx.emit(DocsEvent::SelectionChanged);
        cx.notify();
        Ok(next_path)
    }

    /// Rename a generated untitled document from the title written into its
    /// JSON envelope by the Doc Assistant. The assistant updates only the
    /// document contents; Choro owns the filesystem move so open-editor state,
    /// metadata, and selection all move together.
    pub fn rename_untitled_doc_from_document_title(
        &mut self,
        project: ProjectId,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<Option<PathBuf>> {
        if !is_untitled_doc_path(path) {
            return Ok(None);
        }
        let document = read_choro_document(path)?;
        document.validate()?;
        let title = document.title.trim();
        if title.is_empty() || title.eq_ignore_ascii_case("untitled") {
            return Ok(None);
        }
        self.rename_doc_to_title(project, path, title, cx).map(Some)
    }

    pub fn delete_doc(
        &mut self,
        project: ProjectId,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if path.exists() {
            fs::remove_file(path).context("failed to delete doc")?;
        }
        let assets = path.with_extension("assets");
        if assets.is_dir() {
            fs::remove_dir_all(&assets).context("failed to delete doc assets")?;
        }
        for sidecar in [choro_backup_path(path), choro_recovery_path(path)] {
            if sidecar.is_file() {
                fs::remove_file(sidecar).context("failed to delete document recovery file")?;
            }
        }
        if let Some(project_info) = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
        {
            if let Some(relative) = project_relative_doc_path(&project_info.path, path) {
                if let Some(metadata) = self.metadata.get_mut(&project) {
                    if remove_doc_metadata(metadata, &relative) {
                        save_docs_metadata(&project_info.path, metadata)?;
                    }
                }
            }
        }
        self.editors.remove(path);
        self.web_documents.remove(path);
        self.web_saved_documents.remove(path);
        self.web_dirty.remove(path);
        self.editor_modified.remove(path);
        self.save_status.remove(path);
        self.save_generation.remove(path);
        self.refresh(cx);
        if self
            .selected
            .get(&project)
            .is_some_and(|selected| selected == path)
        {
            let fallback = self.docs.get(&project).and_then(|docs| {
                docs.iter()
                    .find(|doc| !doc.is_template)
                    .or_else(|| docs.first())
            });
            match fallback {
                Some(doc) => {
                    self.selected.insert(project, doc.path.clone());
                }
                None => {
                    self.selected.remove(&project);
                }
            }
        }
        cx.emit(DocsEvent::SelectionChanged);
        cx.notify();
        Ok(())
    }

    pub fn editor_for_path(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Result<Entity<VelotypeEditor>> {
        if let Some(editor) = self.editors.get(path).cloned() {
            self.reload_editor_from_disk_if_clean(path, editor.clone(), cx)?;
            return Ok(editor);
        }
        let markdown = fs::read_to_string(path).unwrap_or_default();
        let path = path.to_path_buf();
        let modified = file_modified(&path);
        let editor = cx.new(|cx| {
            let mut editor = VelotypeEditor::from_markdown(cx, markdown, Some(path.clone()));
            // Copy inserted/pasted images into `<doc>/assets` with relative
            // references so docs stay portable within the project.
            editor.set_copy_images_into_assets(true);
            editor.embedded_set_chrome_visible(false, cx);
            editor.embedded_set_host_window_integration(false, cx);
            editor
        });
        let reference_path = path.clone();
        cx.subscribe(
            &editor,
            move |this: &mut Self, _editor, event: &velotype::EditorEvent, cx| match event {
                velotype::EditorEvent::ContentChanged => {
                    this.schedule_save(reference_path.clone(), cx);
                }
                velotype::EditorEvent::OpenReference { target } => {
                    cx.emit(DocsEvent::OpenReference {
                        doc_path: reference_path.clone(),
                        target: target.clone(),
                    });
                }
            },
        )
        .detach();
        self.save_status.insert(path.clone(), DocSaveStatus::Saved);
        self.editor_modified.insert(path.clone(), modified);
        self.editors.insert(path, editor.clone());
        Ok(editor)
    }

    pub fn web_document_for_path(&mut self, path: &Path) -> Result<ChoroDocument> {
        if let Some(document) = self.web_documents.get(path) {
            return Ok(document.clone());
        }
        let document = read_choro_document(path)?;
        document.validate()?;
        self.editor_modified
            .insert(path.to_path_buf(), file_modified(path));
        self.save_status
            .insert(path.to_path_buf(), DocSaveStatus::Saved);
        self.web_documents
            .insert(path.to_path_buf(), document.clone());
        self.web_saved_documents
            .insert(path.to_path_buf(), document.clone());
        Ok(document)
    }

    /// Return the Choro document linked to one PocketComet wiki page. The
    /// source identity lives in the human-readable document envelope, so a
    /// title/file rename never breaks the link.
    pub fn pocketcomet_document(
        &mut self,
        project: ProjectId,
        document_id: &str,
        cx: &mut Context<Self>,
    ) -> Result<Option<SyncedChoroDocument>> {
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let mut matches = scan_project_docs(&project_info)
            .into_iter()
            .filter_map(|entry| {
                let document = read_choro_document(&entry.path).ok()?;
                document
                    .origin
                    .as_ref()
                    .is_some_and(|origin| origin.pocketcomet_document_id() == document_id)
                    .then_some(entry)
            });
        let Some(mut entry) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            anyhow::bail!("more than one Choro document is linked to this PocketComet page");
        }
        if self.web_dirty.contains(&entry.path) {
            self.persist_web_document_now(&entry.path)
                .context("failed to save Choro edits before syncing")?;
        }
        let mut document = read_choro_document(&entry.path)?;
        document.validate()?;
        // Choro's visible title is owned by the file name. Keep the API and
        // PocketComet indicator aligned even for legacy documents whose
        // envelope title predates a local rename.
        entry.title = doc_title_from_path(&entry.path);
        document.title = entry.title.clone();
        let revision = choro_document_revision(&entry.title, &document.blocks)?;
        Ok(Some(SyncedChoroDocument {
            entry,
            document,
            revision,
        }))
    }

    /// Create or replace the local side of a PocketComet document link. This
    /// writes atomically, preserves Choro's backup behavior, refreshes an open
    /// editor, and never removes an existing document.
    pub fn upsert_pocketcomet_document(
        &mut self,
        project: ProjectId,
        origin: ChoroDocumentOrigin,
        title: &str,
        blocks: Vec<serde_json::Value>,
        cx: &mut Context<Self>,
    ) -> Result<SyncedChoroDocument> {
        let project_info = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .cloned()
            .context("project not found")?;
        let document_id = origin.pocketcomet_document_id().to_string();
        let clean_title = title.trim();
        let clean_title = if clean_title.is_empty() {
            "Untitled"
        } else {
            clean_title
        };
        let mut document = ChoroDocument {
            version: 1,
            format: "blocknote".to_string(),
            title: clean_title.to_string(),
            blocks,
            origin: Some(origin),
        };
        sanitize_choro_document(&mut document);
        document.validate()?;

        let existing = self.pocketcomet_document(project, &document_id, cx)?;
        let path = if let Some(existing) = existing {
            let path = existing.entry.path;
            write_choro_document(&path, &document)
                .context("failed to update synced Choro document")?;
            self.web_documents.insert(path.clone(), document.clone());
            self.web_saved_documents
                .insert(path.clone(), document.clone());
            self.web_dirty.remove(&path);
            self.editor_modified
                .insert(path.clone(), file_modified(&path));
            self.save_status.insert(path.clone(), DocSaveStatus::Saved);
            self.rename_doc_to_title(project, &path, clean_title, cx)?
        } else {
            let root = docs_dir(&project_info.path);
            fs::create_dir_all(&root).context("failed to create docs directory")?;
            let path = unique_doc_path(&root, clean_title);
            write_choro_document(&path, &document)
                .context("failed to create synced Choro document")?;
            self.web_documents.insert(path.clone(), document.clone());
            self.web_saved_documents
                .insert(path.clone(), document.clone());
            self.editor_modified
                .insert(path.clone(), file_modified(&path));
            self.save_status.insert(path.clone(), DocSaveStatus::Saved);
            self.refresh(cx);
            path
        };
        let entry = synced_doc_entry(&project_info, path);
        let revision = choro_document_revision(&entry.title, &document.blocks)?;
        cx.emit(DocsEvent::Changed);
        cx.notify();
        Ok(SyncedChoroDocument {
            entry,
            document,
            revision,
        })
    }

    /// Store PocketComet media beside its linked Choro document. Content-
    /// addressed names make retries idempotent and keep authenticated Convex
    /// storage URLs out of the portable document JSON.
    pub fn store_pocketcomet_asset(
        &mut self,
        project: ProjectId,
        document_id: &str,
        mime: &str,
        bytes: &[u8],
        cx: &mut Context<Self>,
    ) -> Result<String> {
        if bytes.is_empty() {
            anyhow::bail!("PocketComet sent an empty document asset");
        }
        let synced = self
            .pocketcomet_document(project, document_id, cx)?
            .context("the linked Choro document is not available")?;
        let extension = pocketcomet_asset_extension(mime);
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let file_name = format!("pocketcomet-{:x}.{extension}", hasher.finalize());
        let asset_dir = synced.entry.path.with_extension("assets");
        fs::create_dir_all(&asset_dir).context("failed to create Choro document assets")?;
        let path = asset_dir.join(&file_name);
        if !path.is_file() {
            write_atomic(&path, bytes).context("failed to store PocketComet document asset")?;
        }
        Ok(format!("choro-asset://localhost/{file_name}"))
    }

    /// Accept a validated document snapshot from the embedded editor and
    /// debounce the disk write. The WebView host updates its own snapshot before
    /// this runs, so the following repaint does not feed the same content back
    /// into BlockNote and disturb its selection.
    pub fn apply_web_document(
        &mut self,
        path: PathBuf,
        mut document: ChoroDocument,
        cx: &mut Context<Self>,
    ) -> Result<()> {
        if !self.web_documents.contains_key(&path) {
            anyhow::bail!("document editor changed a document that is not open");
        }
        sanitize_choro_document(&mut document);
        document.validate()?;
        self.web_documents.insert(path.clone(), document);
        self.web_dirty.insert(path.clone());
        self.schedule_web_save(path, cx);
        Ok(())
    }

    pub fn refresh_open_docs_from_disk(&mut self, cx: &mut Context<Self>) {
        let web_paths = self.web_documents.keys().cloned().collect::<Vec<_>>();
        let mut changed = false;
        for path in web_paths {
            let current_modified = file_modified(&path);
            let known_modified = self.editor_modified.get(&path).cloned().flatten();
            if current_modified == known_modified || self.web_dirty.contains(&path) {
                continue;
            }
            match read_choro_document(&path).and_then(|document| {
                document.validate()?;
                Ok(document)
            }) {
                Ok(document) => {
                    self.web_documents.insert(path.clone(), document.clone());
                    self.web_saved_documents.insert(path.clone(), document);
                    self.editor_modified.insert(path.clone(), current_modified);
                    self.save_status.insert(path, DocSaveStatus::Saved);
                    changed = true;
                }
                Err(error) => {
                    self.save_status
                        .insert(path, DocSaveStatus::Error(error.to_string()));
                    changed = true;
                }
            }
        }
        let paths = self.editors.keys().cloned().collect::<Vec<_>>();
        for path in paths {
            let Some(editor) = self.editors.get(&path).cloned() else {
                continue;
            };
            match self.reload_editor_from_disk_if_clean(&path, editor, cx) {
                Ok(reloaded) => {
                    changed |= reloaded;
                }
                Err(error) => {
                    self.save_status
                        .insert(path.clone(), DocSaveStatus::Error(error.to_string()));
                    changed = true;
                }
            }
        }
        if changed {
            cx.emit(DocsEvent::Changed);
            cx.notify();
        }
    }

    pub fn relative_path_for(&self, project: ProjectId, path: &Path, cx: &App) -> Option<PathBuf> {
        let project = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)?;
        project_relative_doc_path(&project.path, path)
    }

    pub fn linked_path_line(&self, project: ProjectId, path: &Path, cx: &App) -> Option<String> {
        self.relative_path_for(project, path, cx)
            .map(|path| path.to_string_lossy().to_string())
    }

    /// Persist every dirty embedded document immediately during graceful shutdown.
    pub fn save_all_now(&mut self, cx: &mut Context<Self>) -> Result<()> {
        let web_jobs = self.web_dirty.iter().cloned().collect::<Vec<_>>();
        for path in web_jobs {
            self.persist_web_document_now(&path)?;
            self.save_generation.remove(&path);
        }

        let jobs = self
            .editors
            .iter()
            .filter_map(|(path, editor)| {
                editor.read(cx).embedded_is_dirty().then(|| {
                    (
                        path.clone(),
                        editor.clone(),
                        editor.read(cx).embedded_markdown(cx),
                    )
                })
            })
            .collect::<Vec<_>>();

        for (path, editor, markdown) in jobs {
            write_atomic(&path, markdown.as_bytes())?;
            let modified = file_modified(&path);
            editor.update(cx, |editor, cx| {
                editor.embedded_mark_saved(path.clone(), cx);
            });
            self.editor_modified.insert(path.clone(), modified);
            self.save_status.insert(path.clone(), DocSaveStatus::Saved);
            self.save_generation.remove(&path);
        }

        cx.emit(DocsEvent::Changed);
        cx.notify();
        Ok(())
    }

    fn schedule_web_save(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let generation = self
            .save_generation
            .entry(path.clone())
            .and_modify(|generation| *generation = generation.wrapping_add(1))
            .or_insert(1);
        let generation = *generation;
        self.save_status.insert(path.clone(), DocSaveStatus::Dirty);
        cx.emit(DocsEvent::Changed);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let _ = this.update(cx, |this, cx| {
                if this.save_generation.get(&path).copied() != Some(generation) {
                    return;
                }
                this.save_status.insert(path.clone(), DocSaveStatus::Saving);
                cx.emit(DocsEvent::Changed);
                cx.notify();
                match this.persist_web_document_now(&path) {
                    Ok(()) => {}
                    Err(error) => {
                        this.save_status
                            .insert(path.clone(), DocSaveStatus::Error(error.to_string()));
                    }
                }
                cx.emit(DocsEvent::Changed);
                cx.notify();
            });
        })
        .detach();
    }

    /// Save the latest in-memory snapshot without allowing an external writer
    /// (notably the Doc Assistant) to be silently overwritten. Distinct block
    /// edits are merged by stable BlockNote IDs; overlapping edits leave the
    /// external file intact and preserve the local version in a recovery file.
    fn persist_web_document_now(&mut self, path: &Path) -> Result<()> {
        let local = self
            .web_documents
            .get(path)
            .cloned()
            .context("document is not loaded")?;
        let known_modified = self.editor_modified.get(path).cloned().flatten();
        let current_modified = file_modified(path);
        let document = if current_modified != known_modified {
            if !path.is_file() {
                let recovery = write_local_recovery(path, &local)?;
                anyhow::bail!(
                    "document was removed on disk; local edits were preserved at {}",
                    recovery.display()
                );
            }
            let external = match read_choro_document(path).and_then(|document| {
                document.validate()?;
                Ok(document)
            }) {
                Ok(document) => document,
                Err(error) => {
                    let recovery = write_local_recovery(path, &local)?;
                    anyhow::bail!(
                        "external document could not be merged ({error}); local edits were preserved at {}",
                        recovery.display()
                    );
                }
            };
            let Some(base) = self.web_saved_documents.get(path).cloned() else {
                let recovery = write_local_recovery(path, &local)?;
                anyhow::bail!(
                    "document has no saved merge base; local edits were preserved at {}",
                    recovery.display()
                );
            };
            match merge_choro_documents(&base, &local, &external) {
                Ok(document) => document,
                Err(error) => {
                    let recovery = write_local_recovery(path, &local)?;
                    anyhow::bail!(
                        "document changed in both Choro and the Doc Assistant ({error}); local edits were preserved at {}",
                        recovery.display()
                    );
                }
            }
        } else {
            local
        };

        write_choro_document(path, &document)?;
        self.editor_modified
            .insert(path.to_path_buf(), file_modified(path));
        self.web_documents
            .insert(path.to_path_buf(), document.clone());
        self.web_saved_documents
            .insert(path.to_path_buf(), document);
        self.web_dirty.remove(path);
        self.save_status
            .insert(path.to_path_buf(), DocSaveStatus::Saved);
        Ok(())
    }

    fn schedule_save(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let generation = self
            .save_generation
            .entry(path.clone())
            .and_modify(|generation| *generation = generation.wrapping_add(1))
            .or_insert(1);
        let generation = *generation;
        self.save_status.insert(path.clone(), DocSaveStatus::Dirty);
        cx.emit(DocsEvent::Changed);
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let save_job = this
                .update(cx, |this, cx| {
                    if this.save_generation.get(&path).copied() != Some(generation) {
                        return None;
                    }
                    let editor = this.editors.get(&path)?.clone();
                    if !editor.read(cx).embedded_is_dirty() {
                        this.save_status.insert(path.clone(), DocSaveStatus::Saved);
                        cx.notify();
                        return None;
                    }
                    let markdown = editor.read(cx).embedded_markdown(cx);
                    this.save_status.insert(path.clone(), DocSaveStatus::Saving);
                    cx.emit(DocsEvent::Changed);
                    cx.notify();
                    Some((path.clone(), generation, markdown))
                })
                .ok()
                .flatten();
            let Some((path, generation, markdown)) = save_job else {
                return;
            };
            let result = cx
                .background_executor()
                .spawn({
                    let path = path.clone();
                    async move {
                        write_atomic(&path, markdown.as_bytes())?;
                        Ok::<_, anyhow::Error>(file_modified(&path))
                    }
                })
                .await;
            let _ = this.update(cx, move |this, cx| {
                if this.save_generation.get(&path).copied() != Some(generation) {
                    return;
                }
                match result {
                    Ok(modified) => {
                        if let Some(editor) = this.editors.get(&path) {
                            editor.update(cx, |editor, cx| {
                                editor.embedded_mark_saved(path.clone(), cx);
                            });
                        }
                        this.editor_modified.insert(path.clone(), modified);
                        this.save_status.insert(path.clone(), DocSaveStatus::Saved);
                    }
                    Err(error) => {
                        this.save_status
                            .insert(path.clone(), DocSaveStatus::Error(error.to_string()));
                    }
                }
                cx.emit(DocsEvent::Changed);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn reload_editor_from_disk_if_clean(
        &mut self,
        path: &Path,
        editor: Entity<VelotypeEditor>,
        cx: &mut Context<Self>,
    ) -> Result<bool> {
        let current_modified = file_modified(path);
        let known_modified = self
            .editor_modified
            .get(path)
            .cloned()
            .unwrap_or_else(|| current_modified.clone());
        if current_modified == known_modified {
            return Ok(false);
        }
        if editor.read(cx).embedded_is_dirty() {
            return Ok(false);
        }
        let markdown = fs::read_to_string(path).context("failed to reload doc from disk")?;
        let path = path.to_path_buf();
        editor.update(cx, |editor, cx| {
            editor.embedded_reload_markdown(markdown, path.clone(), cx);
        });
        self.editor_modified.insert(path.clone(), current_modified);
        self.save_status.insert(path, DocSaveStatus::Saved);
        Ok(true)
    }
}

pub fn docs_dir(project_path: &Path) -> PathBuf {
    project_path.join(DOCS_DIR_NAME)
}

fn legacy_docs_dir(project_path: &Path) -> PathBuf {
    project_path.join(LEGACY_DOCS_DIR_NAME)
}

fn docs_metadata_path(project_path: &Path) -> PathBuf {
    docs_dir(project_path).join(DOCS_METADATA_FILE)
}

fn default_doc_labels() -> Vec<String> {
    DEFAULT_DOC_LABELS
        .iter()
        .map(|label| (*label).to_string())
        .collect()
}

fn load_docs_metadata(project_path: &Path) -> DocsMetadataFile {
    let current_path = docs_metadata_path(project_path);
    let legacy_path = legacy_docs_dir(project_path).join(DOCS_METADATA_FILE);
    let read_path = if current_path.is_file() {
        current_path
    } else {
        legacy_path
    };
    let mut metadata = fs::read_to_string(read_path)
        .ok()
        .and_then(|text| serde_json::from_str::<DocsMetadataFile>(&text).ok())
        .unwrap_or_default();
    normalize_docs_metadata(&mut metadata);
    normalize_docs_metadata_paths(&mut metadata);
    metadata
}

fn save_docs_metadata(project_path: &Path, metadata: &DocsMetadataFile) -> Result<()> {
    let mut metadata = metadata.clone();
    normalize_docs_metadata(&mut metadata);
    normalize_docs_metadata_paths(&mut metadata);
    let path = docs_metadata_path(project_path);
    let parent = path.parent().context("docs metadata path has no parent")?;
    fs::create_dir_all(parent).context("failed to create docs directory")?;
    let json = serde_json::to_vec_pretty(&metadata).context("failed to serialize docs metadata")?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).context("failed to write docs metadata")?;
    fs::rename(&tmp, path).context("failed to replace docs metadata")?;
    Ok(())
}

fn normalize_docs_metadata(metadata: &mut DocsMetadataFile) {
    let mut labels = default_doc_labels();
    for label in metadata.labels.clone() {
        if let Some(label) = clean_doc_label(&label) {
            ensure_label_value(&mut labels, label);
        }
    }
    metadata.labels = labels;
    metadata.doc_labels.retain(|_, label| {
        clean_doc_label(label)
            .map(|clean| {
                *label = clean;
                true
            })
            .unwrap_or(false)
    });
}

fn normalize_docs_metadata_paths(metadata: &mut DocsMetadataFile) {
    metadata.doc_labels = migrate_metadata_map(std::mem::take(&mut metadata.doc_labels));
    metadata.doc_statuses = migrate_metadata_map(std::mem::take(&mut metadata.doc_statuses));
    metadata.doc_implementors =
        migrate_metadata_map(std::mem::take(&mut metadata.doc_implementors));
}

fn migrate_metadata_map<T>(entries: HashMap<String, T>) -> HashMap<String, T> {
    let mut current = HashMap::new();
    let mut legacy = Vec::new();
    for (path, value) in entries {
        let migrated = migrate_legacy_doc_path(Path::new(&path));
        let migrated_key = doc_metadata_key(&migrated);
        if migrated_key == path {
            current.insert(path, value);
        } else {
            legacy.push((migrated_key, value));
        }
    }
    for (path, value) in legacy {
        current.entry(path).or_insert(value);
    }
    current
}

fn move_doc_metadata(
    metadata: &mut DocsMetadataFile,
    previous_relative: &Path,
    next_relative: &Path,
) -> bool {
    let previous_key = doc_metadata_key(previous_relative);
    let next_key = doc_metadata_key(next_relative);
    let mut changed = false;
    if let Some(label) = metadata.doc_labels.remove(&previous_key) {
        metadata.doc_labels.insert(next_key.clone(), label);
        changed = true;
    }
    if let Some(status) = metadata.doc_statuses.remove(&previous_key) {
        metadata.doc_statuses.insert(next_key.clone(), status);
        changed = true;
    }
    if let Some(agent_id) = metadata.doc_implementors.remove(&previous_key) {
        metadata.doc_implementors.insert(next_key, agent_id);
        changed = true;
    }
    changed
}

fn remove_doc_metadata(metadata: &mut DocsMetadataFile, relative_path: &Path) -> bool {
    let key = doc_metadata_key(relative_path);
    let label_removed = metadata.doc_labels.remove(&key).is_some();
    let status_removed = metadata.doc_statuses.remove(&key).is_some();
    let implementor_removed = metadata.doc_implementors.remove(&key).is_some();
    label_removed || status_removed || implementor_removed
}

fn ensure_doc_label(metadata: &mut DocsMetadataFile, label: &str) {
    ensure_label_value(&mut metadata.labels, label.to_string());
}

fn ensure_label_value(labels: &mut Vec<String>, label: String) {
    if !labels
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(&label))
    {
        labels.push(label);
    }
}

pub fn clean_doc_label(label: &str) -> Option<String> {
    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
    (!label.is_empty()).then_some(label)
}

fn doc_metadata_key(relative_path: &Path) -> String {
    relative_path.to_string_lossy().replace('\\', "/")
}

pub fn starter_doc_template() -> &'static str {
    "# Feature / PRD\n\n## Problem\n\n\n## Goal\n\n\n## Users\n\n\n## Requirements\n\n- \n\n## Out of Scope\n\n- \n\n## Open Questions\n\n- \n\n## Acceptance Criteria\n\n- \n"
}

pub fn starter_choro_document() -> ChoroDocument {
    built_in_doc_template("feature-prd")
        .expect("feature PRD template exists")
        .document
}

fn choro_document(title: &str, blocks: serde_json::Value) -> ChoroDocument {
    ChoroDocument {
        version: 1,
        format: "blocknote".to_string(),
        title: title.to_string(),
        blocks: serde_json::from_value(blocks).expect("built-in BlockNote blocks are valid JSON"),
        origin: None,
    }
}

fn built_in_doc_templates() -> Vec<BuiltInDocTemplate> {
    vec![
        BuiltInDocTemplate {
            id: "blank",
            name: "Blank Doc",
            description: "Start with an empty page.",
            default_label: None,
            document: choro_document(
                "Untitled",
                serde_json::json!([{ "type": "paragraph", "content": "" }]),
            ),
        },
        BuiltInDocTemplate {
            id: "feature-prd",
            name: "Feature / PRD",
            description: "Define the problem, users, requirements, scope, and success criteria.",
            default_label: Some("feature"),
            document: choro_document(
                "Feature / PRD",
                serde_json::json!([
                    { "type": "heading", "props": { "level": 1 }, "content": "Feature / PRD" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Problem" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Goal" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Users" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Requirements" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Out of Scope" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Open Questions" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Acceptance Criteria" },
                    { "type": "checkListItem", "content": "" }
                ]),
            ),
        },
        BuiltInDocTemplate {
            id: "technical-design",
            name: "Technical Design",
            description:
                "Describe architecture, affected systems, trade-offs, rollout, and testing.",
            default_label: Some("design"),
            document: choro_document(
                "Technical Design",
                serde_json::json!([
                    { "type": "heading", "props": { "level": 1 }, "content": "Technical Design" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Context" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Proposed Design" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Affected Systems" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Data and APIs" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Trade-offs" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Rollout and Migration" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Testing" },
                    { "type": "checkListItem", "content": "" }
                ]),
            ),
        },
        BuiltInDocTemplate {
            id: "research-spike",
            name: "Research Spike",
            description: "Investigate an unknown, compare options, and record a recommendation.",
            default_label: Some("research"),
            document: choro_document(
                "Research Spike",
                serde_json::json!([
                    { "type": "heading", "props": { "level": 1 }, "content": "Research Spike" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Question" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Constraints" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Findings" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Options" },
                    { "type": "numberedListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Recommendation" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Remaining Unknowns" },
                    { "type": "bulletListItem", "content": "" }
                ]),
            ),
        },
        BuiltInDocTemplate {
            id: "decision-record",
            name: "Decision Record",
            description: "Capture a decision, the alternatives, reasoning, and consequences.",
            default_label: Some("design"),
            document: choro_document(
                "Decision Record",
                serde_json::json!([
                    { "type": "heading", "props": { "level": 1 }, "content": "Decision Record" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Context" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Decision" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Alternatives Considered" },
                    { "type": "bulletListItem", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Reasoning" },
                    { "type": "paragraph", "content": "" },
                    { "type": "heading", "props": { "level": 2 }, "content": "Consequences" },
                    { "type": "bulletListItem", "content": "" }
                ]),
            ),
        },
    ]
}

fn built_in_doc_template(id: &str) -> Option<BuiltInDocTemplate> {
    built_in_doc_templates()
        .into_iter()
        .find(|template| template.id == id)
}

pub fn scan_project_docs(project: &Project) -> Vec<DocEntry> {
    let root = docs_dir(&project.path);
    let mut docs = Vec::new();
    collect_choro_docs(project, &root, &root, &mut docs);
    let legacy_root = legacy_docs_dir(&project.path);
    let legacy_is_alias = fs::symlink_metadata(&legacy_root)
        .map(|metadata| metadata.file_type().is_symlink())
        .unwrap_or(false);
    if legacy_root.is_dir() && !legacy_is_alias {
        collect_choro_docs(project, &legacy_root, &legacy_root, &mut docs);
    }
    docs.sort_by(|left, right| {
        left.relative_path
            .to_string_lossy()
            .to_lowercase()
            .cmp(&right.relative_path.to_string_lossy().to_lowercase())
    });
    docs
}

fn synced_doc_entry(project: &Project, path: PathBuf) -> DocEntry {
    let relative_path =
        project_relative_doc_path(&project.path, &path).unwrap_or_else(|| path.clone());
    DocEntry {
        project: project.id,
        project_name: project.name.clone(),
        project_path: project.path.clone(),
        title: doc_title_from_path(&path),
        modified: file_modified(&path),
        is_template: false,
        path,
        relative_path,
    }
}

fn choro_document_revision(title: &str, blocks: &[serde_json::Value]) -> Result<String> {
    let payload = serde_json::to_vec(&serde_json::json!({
        "title": title,
        "blocks": blocks,
    }))
    .context("failed to hash Choro document")?;
    let mut hasher = Sha256::new();
    hasher.update(payload);
    Ok(format!("{:x}", hasher.finalize()))
}

fn pocketcomet_asset_extension(mime: &str) -> &'static str {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "video/mp4" => "mp4",
        "audio/mpeg" => "mp3",
        "audio/mp4" => "m4a",
        "application/pdf" => "pdf",
        _ => "bin",
    }
}

fn collect_choro_docs(project: &Project, docs_root: &Path, dir: &Path, docs: &mut Vec<DocEntry>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_choro_docs(project, docs_root, &path, docs);
            continue;
        }
        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("choro") {
            continue;
        }
        let modified = entry
            .metadata()
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let relative_path =
            project_relative_doc_path(&project.path, &path).unwrap_or_else(|| path.clone());
        let title = doc_title_from_path(&path);
        let is_template = is_project_template_path(&relative_path);
        docs.push(DocEntry {
            project: project.id,
            project_name: project.name.clone(),
            project_path: project.path.clone(),
            path,
            relative_path,
            title,
            modified,
            is_template,
        });
    }
}

fn is_project_template_path(relative_path: &Path) -> bool {
    relative_path
        .components()
        .any(|component| component.as_os_str() == DOC_TEMPLATES_DIR)
}

pub fn project_relative_doc_path(project_path: &Path, path: &Path) -> Option<PathBuf> {
    path.strip_prefix(project_path).ok().map(Path::to_path_buf)
}

pub fn doc_title_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| {
            stem.replace(['-', '_'], " ")
                .split_whitespace()
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                        None => String::new(),
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled".to_string())
}

pub fn is_untitled_doc_path(path: &Path) -> bool {
    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    if stem.eq_ignore_ascii_case("untitled") {
        return true;
    }
    let Some(suffix) = stem
        .to_ascii_lowercase()
        .strip_prefix("untitled-")
        .map(str::to_string)
    else {
        return false;
    };
    !suffix.is_empty() && suffix.chars().all(|ch| ch.is_ascii_digit())
}

pub fn slug_for_title(title: &str) -> String {
    let mut slug = String::new();
    let mut previous_dash = false;
    for ch in title.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash && !slug.is_empty() {
            slug.push('-');
            previous_dash = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "untitled".to_string()
    } else {
        slug
    }
}

pub fn unique_doc_path(dir: &Path, stem: &str) -> PathBuf {
    unique_doc_path_excluding(dir, stem, None)
}

pub fn unique_doc_path_excluding(dir: &Path, stem: &str, excluding: Option<&Path>) -> PathBuf {
    let stem = slug_for_title(stem);
    for ix in 1.. {
        let file_name = if ix == 1 {
            format!("{stem}.choro")
        } else {
            format!("{stem}-{ix}.choro")
        };
        let candidate = dir.join(file_name);
        if excluding.is_some_and(|path| path == candidate) || !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

pub fn delete_confirmation_title(path: &Path) -> String {
    format!("Delete {}?", doc_title_from_path(path))
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("doc path has no parent")?;
    fs::create_dir_all(parent).context("failed to create docs directory")?;
    let temp_extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| format!("{extension}.tmp"))
        .unwrap_or_else(|| "tmp".to_string());
    let tmp = path.with_extension(temp_extension);
    fs::write(&tmp, bytes).context("failed to write temp doc")?;
    fs::rename(&tmp, path).context("failed to replace doc")?;
    Ok(())
}

fn read_choro_document(path: &Path) -> Result<ChoroDocument> {
    let size = fs::metadata(path)
        .context("failed to inspect Choro document")?
        .len();
    if size > MAX_CHORO_DOCUMENT_BYTES {
        anyhow::bail!(
            "Choro document is larger than the {} MB limit",
            MAX_CHORO_DOCUMENT_BYTES / (1024 * 1024)
        );
    }
    let source = fs::read_to_string(path).context("failed to read Choro document")?;
    let mut document: ChoroDocument =
        serde_json::from_str(&source).context("failed to parse Choro document JSON")?;
    sanitize_choro_document(&mut document);
    Ok(document)
}

fn sanitize_choro_document(document: &mut ChoroDocument) {
    document
        .title
        .retain(|character| !matches!(character, '\u{001c}'..='\u{001f}'));
    for block in &mut document.blocks {
        remove_navigation_controls(block);
    }
}

fn remove_navigation_controls(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(text) => {
            text.retain(|character| !matches!(character, '\u{001c}'..='\u{001f}'));
        }
        serde_json::Value::Array(values) => {
            for value in values {
                remove_navigation_controls(value);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                remove_navigation_controls(value);
            }
        }
        _ => {}
    }
}

fn write_choro_document(path: &Path, document: &ChoroDocument) -> Result<()> {
    document.validate()?;
    let bytes =
        serde_json::to_vec_pretty(document).context("failed to serialize Choro document")?;
    if bytes.len() as u64 > MAX_CHORO_DOCUMENT_BYTES {
        anyhow::bail!(
            "Choro document is larger than the {} MB limit",
            MAX_CHORO_DOCUMENT_BYTES / (1024 * 1024)
        );
    }
    if path.is_file() {
        fs::copy(path, choro_backup_path(path))
            .context("failed to preserve previous Choro document backup")?;
    }
    write_atomic(path, &bytes)
}

fn create_choro_document_with_assets(
    path: &Path,
    document: &ChoroDocument,
    asset_source: Option<&Path>,
) -> Result<()> {
    write_choro_document(path, document)?;
    let Some(asset_source) = asset_source else {
        return Ok(());
    };
    if let Err(error) = copy_doc_assets(asset_source, path) {
        let cleanup_error = fs::remove_file(path).err();
        if let Some(cleanup_error) = cleanup_error {
            return Err(error).context(format!(
                "failed to roll back document after asset copy failed: {cleanup_error}"
            ));
        }
        return Err(error);
    }
    Ok(())
}

fn copy_doc_assets(source_doc: &Path, target_doc: &Path) -> Result<()> {
    let source_dir = source_doc.with_extension("assets");
    if !source_dir.is_dir() {
        return Ok(());
    }
    let target_dir = target_doc.with_extension("assets");
    let temp_dir = target_doc.with_extension(format!("assets.tmp-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&temp_dir)
        .context("failed to create temporary document assets directory")?;
    let result = (|| -> Result<()> {
        for entry in fs::read_dir(&source_dir).context("failed to read document assets")? {
            let entry = entry.context("failed to inspect document asset")?;
            if !entry
                .file_type()
                .context("failed to inspect document asset type")?
                .is_file()
            {
                continue;
            }
            fs::copy(entry.path(), temp_dir.join(entry.file_name()))
                .context("failed to copy document asset")?;
        }
        fs::rename(&temp_dir, &target_dir).context("failed to move document assets into place")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp_dir);
    }
    result
}

fn choro_backup_path(path: &Path) -> PathBuf {
    path.with_extension("choro.bak")
}

fn choro_recovery_path(path: &Path) -> PathBuf {
    path.with_extension("choro.local-recovery")
}

fn write_local_recovery(path: &Path, document: &ChoroDocument) -> Result<PathBuf> {
    let recovery = choro_recovery_path(path);
    let bytes =
        serde_json::to_vec_pretty(document).context("failed to serialize local recovery")?;
    write_atomic(&recovery, &bytes).context("failed to preserve local document recovery")?;
    Ok(recovery)
}

fn merge_choro_documents(
    base: &ChoroDocument,
    local: &ChoroDocument,
    external: &ChoroDocument,
) -> Result<ChoroDocument> {
    base.validate()?;
    local.validate()?;
    external.validate()?;
    let title = choose_three_way(&base.title, &local.title, &external.title)
        .context("the document title changed on both sides")?;
    let blocks = merge_block_arrays(&base.blocks, &local.blocks, &external.blocks)?;
    let origin = choose_three_way(&base.origin, &local.origin, &external.origin)
        .context("the document origin changed on both sides")?;
    Ok(ChoroDocument {
        version: 1,
        format: "blocknote".to_string(),
        title,
        blocks,
        origin,
    })
}

fn choose_three_way<T: Clone + Eq>(base: &T, local: &T, external: &T) -> Option<T> {
    if local == external {
        Some(local.clone())
    } else if local == base {
        Some(external.clone())
    } else if external == base {
        Some(local.clone())
    } else {
        None
    }
}

fn merge_block_arrays(
    base: &[serde_json::Value],
    local: &[serde_json::Value],
    external: &[serde_json::Value],
) -> Result<Vec<serde_json::Value>> {
    if let Some(blocks) = choose_three_way(&base.to_vec(), &local.to_vec(), &external.to_vec()) {
        return Ok(blocks);
    }

    let (base_order, base_blocks) = keyed_blocks(base)
        .context("concurrent document edits cannot be merged before blocks have stable IDs")?;
    let (local_order, local_blocks) =
        keyed_blocks(local).context("local document contains a block without a stable ID")?;
    let (external_order, external_blocks) =
        keyed_blocks(external).context("external document contains a block without a stable ID")?;

    let mut order = if external_order == base_order {
        local_order.clone()
    } else {
        external_order.clone()
    };
    let mut seen = order.iter().cloned().collect::<HashSet<_>>();
    for id in local_order.iter().chain(external_order.iter()) {
        if seen.insert(id.clone()) {
            order.push(id.clone());
        }
    }

    let mut merged = Vec::with_capacity(order.len());
    for id in order {
        let base = base_blocks.get(&id);
        let local = local_blocks.get(&id);
        let external = external_blocks.get(&id);
        let block = if local == external {
            local.cloned()
        } else if local == base {
            external.cloned()
        } else if external == base {
            local.cloned()
        } else {
            anyhow::bail!("block {id} changed on both sides");
        };
        if let Some(block) = block {
            merged.push(block.clone());
        }
    }
    Ok(merged)
}

fn keyed_blocks(
    blocks: &[serde_json::Value],
) -> Option<(Vec<String>, HashMap<String, serde_json::Value>)> {
    let mut order = Vec::with_capacity(blocks.len());
    let mut keyed = HashMap::with_capacity(blocks.len());
    for block in blocks {
        let id = block.get("id")?.as_str()?.to_string();
        if keyed.insert(id.clone(), block.clone()).is_some() {
            return None;
        }
        order.push(id);
    }
    Some((order, keyed))
}

fn file_modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_at(path: PathBuf) -> Project {
        Project::from_path(path)
    }

    fn document_with_blocks(blocks: Vec<serde_json::Value>) -> ChoroDocument {
        ChoroDocument {
            version: 1,
            format: "blocknote".to_string(),
            title: "Spec".to_string(),
            blocks,
            origin: None,
        }
    }

    #[test]
    fn pocketcomet_asset_extensions_are_derived_only_from_supported_mime_types() {
        assert_eq!(pocketcomet_asset_extension("image/png"), "png");
        assert_eq!(pocketcomet_asset_extension(" IMAGE/JPEG "), "jpg");
        assert_eq!(pocketcomet_asset_extension("image/svg+xml"), "svg");
        assert_eq!(pocketcomet_asset_extension("text/html"), "bin");
    }

    #[test]
    fn merges_concurrent_changes_to_distinct_blocks() {
        let base = document_with_blocks(vec![
            serde_json::json!({ "id": "goal", "type": "paragraph", "content": "Base goal" }),
            serde_json::json!({ "id": "scope", "type": "paragraph", "content": "Base scope" }),
        ]);
        let mut local = base.clone();
        local.blocks[0]["content"] = serde_json::json!("Local goal");
        let mut external = base.clone();
        external.blocks[1]["content"] = serde_json::json!("Assistant scope");

        let merged = merge_choro_documents(&base, &local, &external).unwrap();

        assert_eq!(merged.blocks[0]["content"], "Local goal");
        assert_eq!(merged.blocks[1]["content"], "Assistant scope");
    }

    #[test]
    fn rejects_concurrent_changes_to_the_same_block() {
        let base = document_with_blocks(vec![serde_json::json!({
            "id": "goal",
            "type": "paragraph",
            "content": "Base"
        })]);
        let mut local = base.clone();
        local.blocks[0]["content"] = serde_json::json!("Local");
        let mut external = base.clone();
        external.blocks[0]["content"] = serde_json::json!("Assistant");

        let error = merge_choro_documents(&base, &local, &external).unwrap_err();

        assert!(error
            .to_string()
            .contains("block goal changed on both sides"));
    }

    #[test]
    fn writes_a_recoverable_previous_document_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("spec.choro");
        let first = document_with_blocks(vec![serde_json::json!({
            "id": "goal",
            "type": "paragraph",
            "content": "First"
        })]);
        let mut second = first.clone();
        second.blocks[0]["content"] = serde_json::json!("Second");

        write_choro_document(&path, &first).unwrap();
        write_choro_document(&path, &second).unwrap();

        assert_eq!(read_choro_document(&path).unwrap(), second);
        assert_eq!(
            read_choro_document(&choro_backup_path(&path)).unwrap(),
            first
        );
    }

    #[test]
    fn scans_nested_choro_docs() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_at(dir.path().join("app"));
        fs::create_dir_all(docs_dir(&project.path).join("features")).unwrap();
        fs::write(docs_dir(&project.path).join("overview.choro"), "one").unwrap();
        fs::write(
            docs_dir(&project.path)
                .join("features")
                .join("checkout.choro"),
            "two",
        )
        .unwrap();
        fs::write(docs_dir(&project.path).join("ignore.txt"), "no").unwrap();

        let docs = scan_project_docs(&project);
        let relative = docs
            .iter()
            .map(|doc| doc.relative_path.clone())
            .collect::<Vec<_>>();
        assert_eq!(
            relative,
            vec![
                PathBuf::from("choro_docs/features/checkout.choro"),
                PathBuf::from("choro_docs/overview.choro")
            ]
        );
    }

    #[test]
    fn built_in_templates_are_focused_on_product_work() {
        let templates = built_in_doc_templates();
        let ids = templates
            .iter()
            .map(|template| template.id)
            .collect::<Vec<_>>();

        assert_eq!(
            ids,
            vec![
                "blank",
                "feature-prd",
                "technical-design",
                "research-spike",
                "decision-record"
            ]
        );
        assert!(templates
            .iter()
            .all(|template| template.document.validate().is_ok()));
        assert!(!ids.iter().any(|id| id.contains("bug")));
        assert!(!ids.iter().any(|id| id.contains("meeting")));
    }

    #[test]
    fn scan_marks_reserved_project_templates() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_at(dir.path().join("app"));
        let docs = docs_dir(&project.path);
        let templates = docs.join(DOC_TEMPLATES_DIR);
        fs::create_dir_all(&templates).unwrap();
        fs::write(docs.join("spec.choro"), "doc").unwrap();
        fs::write(templates.join("team-prd.choro"), "template").unwrap();

        let entries = scan_project_docs(&project);

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "Team Prd")
                .map(|entry| entry.is_template),
            Some(true)
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.title == "Spec")
                .map(|entry| entry.is_template),
            Some(false)
        );
    }

    #[test]
    fn copying_a_template_preserves_its_assets() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.choro");
        let target = dir.path().join("target.choro");
        let source_assets = source.with_extension("assets");
        fs::create_dir_all(&source_assets).unwrap();
        fs::write(source_assets.join("diagram.png"), b"image").unwrap();

        copy_doc_assets(&source, &target).unwrap();

        assert_eq!(
            fs::read(target.with_extension("assets").join("diagram.png")).unwrap(),
            b"image"
        );
    }

    #[test]
    fn failed_template_asset_copy_rolls_back_the_new_document() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.choro");
        let target = dir.path().join("target.choro");
        let source_assets = source.with_extension("assets");
        let target_assets = target.with_extension("assets");
        fs::create_dir_all(&source_assets).unwrap();
        fs::write(source_assets.join("diagram.png"), b"new image").unwrap();
        fs::create_dir_all(&target_assets).unwrap();
        fs::write(target_assets.join("existing.png"), b"existing image").unwrap();
        let document = document_with_blocks(vec![serde_json::json!({
            "id": "goal",
            "type": "paragraph",
            "content": "Goal"
        })]);

        let error = create_choro_document_with_assets(&target, &document, Some(&source))
            .expect_err("an existing target assets directory must reject the copy");

        assert!(error
            .to_string()
            .contains("failed to move document assets into place"));
        assert!(!target.exists());
        assert_eq!(
            fs::read(target_assets.join("existing.png")).unwrap(),
            b"existing image"
        );
    }

    #[test]
    fn strips_macos_navigation_controls_from_block_content() {
        let mut value = serde_json::json!({
            "type": "paragraph",
            "content": [{ "type": "text", "text": "left\u{001c}up\u{001e}down\u{001f}right\u{001d}" }]
        });

        remove_navigation_controls(&mut value);

        assert_eq!(value["content"][0]["text"], "leftupdownright");
    }

    #[test]
    fn scan_reads_legacy_docs_without_mutating_the_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_at(dir.path().join("app"));
        let legacy = project.path.join(LEGACY_DOCS_DIR_NAME);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("spec.choro"), "legacy content").unwrap();

        let docs = scan_project_docs(&project);

        assert!(legacy.is_dir());
        assert!(!docs_dir(&project.path).exists());
        assert_eq!(
            fs::read_to_string(legacy.join("spec.choro")).unwrap(),
            "legacy content"
        );
        assert_eq!(
            docs[0].relative_path,
            PathBuf::from("my_ide_docs/spec.choro")
        );
    }

    #[test]
    fn scan_preserves_both_directories_when_names_collide() {
        let dir = tempfile::tempdir().unwrap();
        let project = project_at(dir.path().join("app"));
        let current = docs_dir(&project.path);
        let legacy = legacy_docs_dir(&project.path);
        fs::create_dir_all(&current).unwrap();
        fs::create_dir_all(&legacy).unwrap();
        fs::write(current.join("current.choro"), "current").unwrap();
        fs::write(legacy.join("legacy.choro"), "legacy").unwrap();

        let docs = scan_project_docs(&project);
        let paths = docs
            .into_iter()
            .map(|doc| doc.relative_path)
            .collect::<Vec<_>>();

        assert!(paths.contains(&PathBuf::from("choro_docs/current.choro")));
        assert!(paths.contains(&PathBuf::from("my_ide_docs/legacy.choro")));
        assert!(current.exists());
        assert!(legacy.exists());
    }

    #[test]
    fn unique_paths_increment_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join(DOCS_DIR_NAME);
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("untitled.choro"), "").unwrap();
        fs::write(docs.join("untitled-2.choro"), "").unwrap();

        assert_eq!(
            unique_doc_path(&docs, "untitled"),
            docs.join("untitled-3.choro")
        );
    }

    #[test]
    fn only_generated_untitled_paths_are_auto_name_candidates() {
        assert!(is_untitled_doc_path(Path::new("untitled.choro")));
        assert!(is_untitled_doc_path(Path::new("Untitled-2.choro")));
        assert!(is_untitled_doc_path(Path::new(
            "choro_docs/untitled-104.choro"
        )));
        assert!(!is_untitled_doc_path(Path::new("payments-spec.choro")));
        assert!(!is_untitled_doc_path(Path::new("untitled-draft.choro")));
        assert!(!is_untitled_doc_path(Path::new("untitled-.choro")));
    }

    #[test]
    fn rename_slug_excludes_current_path() {
        let dir = tempfile::tempdir().unwrap();
        let docs = dir.path().join(DOCS_DIR_NAME);
        fs::create_dir_all(&docs).unwrap();
        let current = docs.join("payment-flow.choro");
        fs::write(&current, "").unwrap();

        assert_eq!(
            unique_doc_path_excluding(&docs, "Payment Flow", Some(&current)),
            current
        );
    }

    #[test]
    fn project_relative_doc_paths_are_repo_relative() {
        let project = PathBuf::from("/repo/app");
        let path = PathBuf::from("/repo/app/choro_docs/spec.choro");
        assert_eq!(
            project_relative_doc_path(&project, &path),
            Some(PathBuf::from("choro_docs/spec.choro"))
        );
    }

    #[test]
    fn delete_confirmation_uses_doc_title() {
        assert_eq!(
            delete_confirmation_title(Path::new("choro_docs/payment-flow.choro")),
            "Delete Payment Flow?"
        );
    }

    #[test]
    fn older_docs_metadata_defaults_doc_implementors() {
        let metadata: DocsMetadataFile = serde_json::from_str(
            r#"{
                "labels": ["feature"],
                "doc_labels": {
                    "choro_docs/spec.md": "feature"
                }
            }"#,
        )
        .unwrap();
        assert!(metadata.doc_statuses.is_empty());
        assert!(metadata.doc_implementors.is_empty());
    }

    #[test]
    fn moving_doc_metadata_moves_label_and_implementor() {
        let agent_id = Uuid::new_v4();
        let mut metadata = DocsMetadataFile::default();
        metadata
            .doc_labels
            .insert("choro_docs/old.md".into(), "feature".into());
        metadata
            .doc_statuses
            .insert("choro_docs/old.md".into(), AgentStatus::InProgress);
        metadata
            .doc_implementors
            .insert("choro_docs/old.md".into(), agent_id);

        assert!(move_doc_metadata(
            &mut metadata,
            Path::new("choro_docs/old.md"),
            Path::new("choro_docs/new.md")
        ));
        assert_eq!(metadata.doc_labels.get("choro_docs/old.md"), None);
        assert_eq!(
            metadata.doc_labels.get("choro_docs/new.md"),
            Some(&"feature".to_string())
        );
        assert_eq!(metadata.doc_statuses.get("choro_docs/old.md"), None);
        assert_eq!(
            metadata.doc_statuses.get("choro_docs/new.md"),
            Some(&AgentStatus::InProgress)
        );
        assert_eq!(metadata.doc_implementors.get("choro_docs/old.md"), None);
        assert_eq!(
            metadata.doc_implementors.get("choro_docs/new.md"),
            Some(&agent_id)
        );
    }

    #[test]
    fn removing_doc_metadata_removes_label_and_implementor() {
        let agent_id = Uuid::new_v4();
        let mut metadata = DocsMetadataFile::default();
        metadata
            .doc_labels
            .insert("choro_docs/spec.md".into(), "feature".into());
        metadata
            .doc_statuses
            .insert("choro_docs/spec.md".into(), AgentStatus::Done);
        metadata
            .doc_implementors
            .insert("choro_docs/spec.md".into(), agent_id);

        assert!(remove_doc_metadata(
            &mut metadata,
            Path::new("choro_docs/spec.md")
        ));
        assert!(metadata.doc_labels.is_empty());
        assert!(metadata.doc_statuses.is_empty());
        assert!(metadata.doc_implementors.is_empty());
    }
}
