use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{AppContext, Context, Entity};
use ide_core::{detect_project_services, read_sub_app_env, EnvFile, ProjectId, SubAppServices};

use super::{Workspace, WorkspaceEvent};

/// How long a scan stays fresh before we re-detect (the on-disk stack rarely
/// changes, so this is generous — a project edit invalidates it immediately).
const SCAN_TTL: Duration = Duration::from_secs(60);

/// Which face of the Services panel is showing.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum ServicesMode {
    #[default]
    Services,
    Env,
}

/// Per-project state for the Services panel: the detected inventory, plus the
/// sidebar's navigation (mode + selected source) and an on-demand env cache.
/// Detection is a read-only file scan (no secrets) run on a background thread;
/// env values are read lazily and only kept in memory, never sent anywhere.
pub struct ServicesState {
    services: HashMap<ProjectId, Vec<SubAppServices>>,
    scanning: HashSet<ProjectId>,
    scanned_at: HashMap<ProjectId, Instant>,
    mode: HashMap<ProjectId, ServicesMode>,
    selected_source: HashMap<ProjectId, String>,
    /// Chosen env file within a source, keyed by `"project:rel"`.
    selected_env_file: HashMap<String, String>,
    env_cache: HashMap<String, Vec<EnvFile>>,
}

impl ServicesState {
    pub fn view(workspace: Entity<Workspace>, cx: &mut gpui::App) -> Entity<Self> {
        cx.new(|cx| {
            cx.subscribe(
                &workspace,
                |this: &mut Self, _, event: &WorkspaceEvent, cx| {
                    if matches!(event, WorkspaceEvent::ProjectsChanged) {
                        this.scanned_at.clear();
                        this.env_cache.clear();
                        cx.notify();
                    }
                },
            )
            .detach();
            Self {
                services: HashMap::new(),
                scanning: HashSet::new(),
                scanned_at: HashMap::new(),
                mode: HashMap::new(),
                selected_source: HashMap::new(),
                selected_env_file: HashMap::new(),
                env_cache: HashMap::new(),
            }
        })
    }

    pub fn services_for(&self, project: ProjectId) -> Option<&Vec<SubAppServices>> {
        self.services.get(&project)
    }

    pub fn is_scanning(&self, project: ProjectId) -> bool {
        self.scanning.contains(&project)
    }

    pub fn mode(&self, project: ProjectId) -> ServicesMode {
        self.mode.get(&project).copied().unwrap_or_default()
    }

    pub fn set_mode(&mut self, project: ProjectId, mode: ServicesMode, cx: &mut Context<Self>) {
        self.mode.insert(project, mode);
        cx.notify();
    }

    /// The chosen source's `rel_path`, if the user has picked one.
    pub fn selected_source(&self, project: ProjectId) -> Option<&String> {
        self.selected_source.get(&project)
    }

    pub fn set_selected_source(
        &mut self,
        project: ProjectId,
        rel_path: String,
        cx: &mut Context<Self>,
    ) {
        self.selected_source.insert(project, rel_path);
        cx.notify();
    }

    pub fn selected_env_file(&self, project: ProjectId, rel_path: &str) -> Option<&String> {
        self.selected_env_file
            .get(&format!("{}:{rel_path}", project.0))
    }

    pub fn set_selected_env_file(
        &mut self,
        project: ProjectId,
        rel_path: &str,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.selected_env_file
            .insert(format!("{}:{rel_path}", project.0), name);
        cx.notify();
    }

    /// Drop cached env values so the next read re-parses from disk (after a save).
    pub fn clear_env_cache(&mut self, cx: &mut Context<Self>) {
        self.env_cache.clear();
        cx.notify();
    }

    /// Read (and cache) a source's env files. Files are tiny and read only when
    /// the Env view needs them; the cache is cleared on rescan.
    pub fn env_files(
        &mut self,
        project: ProjectId,
        rel_path: &str,
        dir: PathBuf,
        names: &[String],
    ) -> Vec<EnvFile> {
        let key = format!("{}:{}", project.0, rel_path);
        if let Some(cached) = self.env_cache.get(&key) {
            return cached.clone();
        }
        let files = read_sub_app_env(&dir, names);
        self.env_cache.insert(key, files.clone());
        files
    }

    /// Kick a background scan when results are missing or stale. Safe to call
    /// from render — it throttles itself and never blocks the UI thread.
    pub fn ensure_scanned(&mut self, project: ProjectId, root: PathBuf, cx: &mut Context<Self>) {
        let fresh = self
            .scanned_at
            .get(&project)
            .is_some_and(|at| at.elapsed() < SCAN_TTL);
        if fresh || self.scanning.contains(&project) {
            return;
        }
        self.scanning.insert(project);
        self.scanned_at.insert(project, Instant::now());
        cx.spawn(async move |this, cx| {
            let detected = cx
                .background_executor()
                .spawn(async move { detect_project_services(&root) })
                .await;
            this.update(cx, |this, cx| {
                this.scanning.remove(&project);
                this.services.insert(project, detected);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Force a re-scan and drop cached env values on the next access.
    pub fn invalidate(&mut self, project: ProjectId, cx: &mut Context<Self>) {
        self.scanned_at.remove(&project);
        self.env_cache.clear();
        cx.notify();
    }
}
