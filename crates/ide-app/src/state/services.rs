use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{AppContext, Context, Entity};
use ide_core::{
    detect_project_environment_files, detect_project_services, local_store::OrbitBuiltin,
    read_sub_app_env, EnvFile, ProjectId, SubAppServices,
};

use super::{Workspace, WorkspaceEvent};

/// How long a scan stays fresh before we re-detect (the on-disk stack rarely
/// changes, so this is generous — a project edit invalidates it immediately).
const SCAN_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ServicesScanKind {
    Environment,
    Integrations,
}

impl ServicesScanKind {
    pub const fn from_builtin(builtin: OrbitBuiltin) -> Self {
        match builtin {
            OrbitBuiltin::Environment => Self::Environment,
            OrbitBuiltin::Integrations => Self::Integrations,
        }
    }

    pub const fn builtin(self) -> OrbitBuiltin {
        match self {
            Self::Environment => OrbitBuiltin::Environment,
            Self::Integrations => OrbitBuiltin::Integrations,
        }
    }

    pub const fn scanning_message(self) -> &'static str {
        match self {
            Self::Environment => "Finding environment files…",
            Self::Integrations => "Detecting integrations…",
        }
    }

    pub const fn empty_title(self) -> &'static str {
        match self {
            Self::Environment => "No environment files found",
            Self::Integrations => "No integrations detected",
        }
    }

    pub const fn empty_body(self) -> &'static str {
        match self {
            Self::Environment => "Orbit could not find a supported .env file in this project.",
            Self::Integrations => {
                "Nothing recognizable in this project's dependencies or platform configuration yet."
            }
        }
    }

    pub const fn item_label(self, count: usize) -> &'static str {
        match (self, count) {
            (Self::Environment, 1) => "env file",
            (Self::Environment, _) => "env files",
            (Self::Integrations, 1) => "integration",
            (Self::Integrations, _) => "integrations",
        }
    }
}

/// Internal state for Orbit's two code-owned views. Environment and
/// Integrations have separate inventories and freshness keys so opening or
/// hiding one cannot accidentally run the other's detector.
pub struct ServicesState {
    services: HashMap<(ProjectId, ServicesScanKind), Vec<SubAppServices>>,
    scanning: HashSet<(ProjectId, ServicesScanKind)>,
    scanned_at: HashMap<(ProjectId, ServicesScanKind), Instant>,
    env_cache: HashMap<String, Vec<EnvFile>>,
    selected_tabs: HashMap<(ProjectId, ServicesScanKind), String>,
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
                env_cache: HashMap::new(),
                selected_tabs: HashMap::new(),
            }
        })
    }

    pub fn services_for(
        &self,
        project: ProjectId,
        kind: ServicesScanKind,
    ) -> Option<&Vec<SubAppServices>> {
        self.services.get(&(project, kind))
    }

    pub fn is_scanning(&self, project: ProjectId, kind: ServicesScanKind) -> bool {
        self.scanning.contains(&(project, kind))
    }

    /// Resolve the selected middle-pane tab, falling back to the first tab when
    /// a rescan removes the previous source or file.
    pub fn selected_tab(
        &self,
        project: ProjectId,
        kind: ServicesScanKind,
        available: &[String],
    ) -> Option<String> {
        super::preferred_selection(self.selected_tabs.get(&(project, kind)), available)
    }

    pub fn select_tab(
        &mut self,
        project: ProjectId,
        kind: ServicesScanKind,
        key: String,
        cx: &mut Context<Self>,
    ) {
        self.selected_tabs.insert((project, kind), key);
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

    /// Kick off the explicitly selected built-in's background scan when its
    /// results are missing or stale.
    pub fn ensure_scanned(
        &mut self,
        project: ProjectId,
        root: PathBuf,
        kind: ServicesScanKind,
        cx: &mut Context<Self>,
    ) {
        let fresh = self
            .scanned_at
            .get(&(project, kind))
            .is_some_and(|at| at.elapsed() < SCAN_TTL);
        if fresh || self.scanning.contains(&(project, kind)) {
            return;
        }
        self.scanning.insert((project, kind));
        self.scanned_at.insert((project, kind), Instant::now());
        cx.spawn(async move |this, cx| {
            let detected = cx
                .background_executor()
                .spawn(async move {
                    match kind {
                        ServicesScanKind::Environment => detect_project_environment_files(&root),
                        ServicesScanKind::Integrations => detect_project_services(&root),
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.scanning.remove(&(project, kind));
                this.services.insert((project, kind), detected);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Force a re-scan and drop cached env values on the next access.
    pub fn invalidate(
        &mut self,
        project: ProjectId,
        kind: ServicesScanKind,
        cx: &mut Context<Self>,
    ) {
        self.scanned_at.remove(&(project, kind));
        if kind == ServicesScanKind::Environment {
            self.env_cache.clear();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_tabs_keep_a_valid_selection_and_fall_back_after_rescan() {
        let available = vec!["project".to_string(), "web".to_string()];
        assert_eq!(
            super::super::preferred_selection(Some(&"web".to_string()), &available),
            Some("web".to_string())
        );
        assert_eq!(
            super::super::preferred_selection(Some(&"removed".to_string()), &available),
            Some("project".to_string())
        );
        assert_eq!(super::super::preferred_selection::<String>(None, &[]), None);
    }
}
