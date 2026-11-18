//! One source of model choices for every Choro surface. Live catalogs are
//! scoped to the project used by the provider and shared across views.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Instant,
};

use gpui::Context;
use ide_core::{
    config::GenerationAgent, model_favorites::ModelFavorite, AgentEffort, AgentKind, AgentModel,
};

use super::{
    codex::{CodexCatalog, CodexModel},
    open_code::{OpenCodeCatalog, OpenCodeCatalogState, OpenCodeModel},
    Workspace,
};

#[derive(Clone, Debug)]
pub(crate) enum ModelChoice {
    BuiltIn(AgentKind, AgentModel),
    Codex(CodexModel),
    OpenCode(OpenCodeModel),
}

impl ModelChoice {
    pub fn provider(&self) -> AgentKind {
        match self {
            Self::BuiltIn(provider, _) => *provider,
            Self::Codex(_) => AgentKind::Codex,
            Self::OpenCode(_) => AgentKind::OpenCode,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::BuiltIn(_, model) => model.short_label(),
            Self::Codex(model) => &model.name,
            Self::OpenCode(model) => &model.name,
        }
    }

    pub fn menu_label(&self) -> &str {
        match self {
            Self::BuiltIn(_, model) => model.menu_label(),
            _ => self.label(),
        }
    }

    pub fn provider_label(&self) -> String {
        match self {
            Self::OpenCode(model) => model.provider_label(),
            _ => self.provider().label().to_string(),
        }
    }

    pub fn selection(&self) -> GenerationAgent {
        let (model, external_model_id, external_model_label) = match self {
            Self::BuiltIn(_, model) => (*model, None, None),
            Self::Codex(model) => (
                AgentModel::CodexDefault,
                Some(model.id.clone()),
                Some(model.name.clone()),
            ),
            Self::OpenCode(model) => (
                AgentModel::OpenCode,
                Some(model.id.clone()),
                Some(model.name.clone()),
            ),
        };
        GenerationAgent {
            provider: self.provider(),
            model,
            external_model_id,
            external_model_label,
        }
    }

    pub fn matches(
        &self,
        provider: AgentKind,
        model: AgentModel,
        external_id: Option<&str>,
    ) -> bool {
        let selected = self.selection();
        provider == selected.provider
            && ModelFavorite::new(model, external_id) == self.favorite_key()
    }

    pub fn favorite_key(&self) -> ModelFavorite {
        let selected = self.selection();
        ModelFavorite::new(selected.model, selected.external_model_id.as_deref())
    }

    pub fn efforts(&self) -> Vec<AgentEffort> {
        match self {
            Self::BuiltIn(AgentKind::Codex, AgentModel::CodexDefault) => Vec::new(),
            Self::BuiltIn(_, model) => model.efforts(),
            Self::Codex(model) => model.efforts.clone(),
            Self::OpenCode(model) => AgentEffort::supported_variants(&model.variants),
        }
    }

    pub fn default_effort(&self) -> AgentEffort {
        match self {
            Self::BuiltIn(_, model) => model.default_effort(),
            Self::Codex(model) => model
                .default_effort
                .filter(|effort| model.efforts.contains(effort))
                .or_else(|| model.efforts.first().copied())
                .unwrap_or_default(),
            Self::OpenCode(_) => self.efforts().first().copied().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProjectModels {
    pub codex: CodexCatalog,
    pub open_code: OpenCodeCatalog,
}

#[derive(Default)]
pub(crate) struct ModelCatalog {
    projects: HashMap<PathBuf, ProjectModels>,
}

impl ModelCatalog {
    pub fn project(&self, path: &Path) -> ProjectModels {
        self.projects.get(path).cloned().unwrap_or_default()
    }

    pub fn choices(&self, provider: AgentKind, path: &Path) -> Vec<ModelChoice> {
        if !provider.is_visible_in_picker() {
            return Vec::new();
        }
        match provider {
            AgentKind::Codex => {
                let mut choices = vec![ModelChoice::BuiltIn(provider, AgentModel::CodexDefault)];
                if let Some(project) = self.projects.get(path) {
                    choices.extend(project.codex.models.iter().cloned().map(ModelChoice::Codex));
                }
                choices
            }
            AgentKind::OpenCode => self
                .projects
                .get(path)
                .into_iter()
                .flat_map(|project| project.open_code.models.iter().cloned())
                .map(ModelChoice::OpenCode)
                .collect(),
            _ => AgentModel::models_for(provider)
                .iter()
                .copied()
                .map(|model| ModelChoice::BuiltIn(provider, model))
                .collect(),
        }
    }

    fn visible_choices(
        &self,
        provider: AgentKind,
        path: &Path,
        hidden: &[ModelFavorite],
    ) -> Vec<ModelChoice> {
        self.choices(provider, path)
            .into_iter()
            .filter(|choice| {
                matches!(
                    choice,
                    ModelChoice::BuiltIn(AgentKind::Codex, AgentModel::CodexDefault)
                ) || !hidden.contains(&choice.favorite_key())
            })
            .collect()
    }

    pub fn status(&self, provider: AgentKind, path: &Path) -> Option<String> {
        let project = self.projects.get(path)?;
        match provider {
            AgentKind::Codex if project.codex.loading => Some("Refreshing Codex models…".into()),
            AgentKind::Codex => project.codex.error.clone(),
            AgentKind::OpenCode if project.open_code.state == OpenCodeCatalogState::Loading => {
                Some("Refreshing OpenCode models…".into())
            }
            AgentKind::OpenCode => project.open_code.error.clone(),
            _ => None,
        }
    }

    pub fn reject_codex_model(&mut self, path: &Path, id: &str) {
        let project = self.projects.entry(path.to_path_buf()).or_default();
        project.codex.models.retain(|model| model.id != id);
        project.codex.error = Some(format!("Codex rejected {id} for this connection. Choose another model, or update Codex and refresh."));
    }
}

impl Workspace {
    pub(crate) fn model_catalog_path(&self) -> PathBuf {
        self.active_project()
            .map(|project| project.path.clone())
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub(crate) fn model_choices(&self, provider: AgentKind) -> Vec<ModelChoice> {
        self.model_choices_for(provider, &self.model_catalog_path())
    }

    pub(crate) fn model_choices_for(&self, provider: AgentKind, path: &Path) -> Vec<ModelChoice> {
        self.model_catalog
            .visible_choices(provider, path, &self.hidden_models)
    }

    pub(crate) fn refresh_model_catalog(&mut self, force: bool, cx: &mut Context<Self>) {
        let current = self.model_catalog_path();
        let mut paths = if force { self.model_catalog.projects.keys().cloned().collect::<Vec<_>>() } else { Vec::new() };
        if !paths.contains(&current) { paths.push(current); }
        for path in paths { self.refresh_model_catalog_for(path, force, cx); }
    }

    pub(crate) fn refresh_model_catalog_for(
        &mut self,
        path: PathBuf,
        force: bool,
        cx: &mut Context<Self>,
    ) {
        let mut started = false;
        let project = self.model_catalog.projects.entry(path.clone()).or_default();
        if !project.codex.loading && (force || project.codex.stale_for(&path)) {
            started = true;
            project.codex.loading = true;
            project.codex.project_path = Some(path.clone());
            project.codex.error = None;
            let requested = path.clone();
            let catalog_path = path.clone();
            cx.spawn(async move |this, cx| {
                let discovery = cx.background_executor().spawn(async move { super::codex::discover(&requested) }).await;
                let _ = this.update(cx, |this, cx| {
                    let catalog = &mut this.model_catalog.projects.entry(catalog_path).or_default().codex;
                    catalog.loading = false;
                    catalog.refreshed_at = Some(Instant::now());
                    match discovery {
                        Ok(Some(result)) => {
                            catalog.models = result.models;
                            catalog.executable = Some(result.executable);
                            catalog.version = Some(result.version);
                        }
                        Ok(None) => {
                            catalog.models.clear();
                            catalog.error = Some("Codex is not installed. Install Codex and sign in, then refresh models.".into());
                        }
                        Err(error) => catalog.error = Some(ide_core::redact_sensitive_text(&format!("{error:#}"))),
                    }
                    cx.notify();
                });
            }).detach();
        }
        // Each provider refreshes independently. Keep the last successful list
        // during refresh or a transient failure; never borrow another project's.
        if project.open_code.state != OpenCodeCatalogState::Loading
            && (force || project.open_code.is_stale_for(&path))
        {
            started = true;
            project.open_code.state = OpenCodeCatalogState::Loading;
            project.open_code.project_path = Some(path.clone());
            project.open_code.error = None;
            let requested = path.clone();
            cx.spawn(async move |this, cx| {
                let discovery = cx
                    .background_executor()
                    .spawn(async move { super::open_code::discover_open_code_models(&requested) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    let catalog = &mut this
                        .model_catalog
                        .projects
                        .entry(path)
                        .or_default()
                        .open_code;
                    catalog.refreshed_at = Some(Instant::now());
                    match discovery {
                        Ok(Some(result)) => {
                            catalog.state = OpenCodeCatalogState::Ready;
                            catalog.models = result.models;
                            catalog.executable = Some(result.executable);
                            catalog.error = None;
                        }
                        Ok(None) => {
                            catalog.state = OpenCodeCatalogState::NotInstalled;
                            catalog.models.clear();
                            catalog.error = Some(
                                "OpenCode is not installed. Install it, then refresh models."
                                    .into(),
                            );
                        }
                        Err(error) => {
                            catalog.state = OpenCodeCatalogState::Failed;
                            catalog.error =
                                Some(ide_core::redact_sensitive_text(&format!("{error:#}")));
                        }
                    }
                    cx.notify();
                });
            })
            .detach();
        }
        if started {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn future_model() -> CodexModel {
        CodexModel {
            id: "future-codex".into(),
            name: "Future Codex".into(),
            efforts: vec![AgentEffort::Low],
            default_effort: Some(AgentEffort::Low),
            is_default: true,
        }
    }

    #[test]
    fn live_choices_are_project_scoped_and_keep_exact_identity_and_capabilities() {
        let path = Path::new("/project-a");
        let mut catalog = ModelCatalog::default();
        catalog
            .projects
            .entry(path.into())
            .or_default()
            .codex
            .models = vec![future_model()];
        let choices = catalog.choices(AgentKind::Codex, path);
        assert_eq!(choices.len(), 2);
        let selection = choices[1].selection();
        let restored: GenerationAgent =
            serde_json::from_slice(&serde_json::to_vec(&selection).unwrap()).unwrap();
        assert_eq!(
            restored.normalized().model_cli_value(),
            Some("future-codex")
        );
        assert_eq!(choices[1].efforts(), [AgentEffort::Low]);
        assert_eq!(choices[1].default_effort(), AgentEffort::Low);
        assert!(choices[1].matches(
            selection.provider,
            selection.model,
            selection.external_model_id.as_deref()
        ));
        assert_eq!(
            catalog
                .choices(AgentKind::Codex, Path::new("/project-b"))
                .len(),
            1
        );
        assert!(catalog.choices(AgentKind::Gemini, path).is_empty());
        assert!(choices[0].efforts().is_empty());
    }

    #[test]
    fn visibility_does_not_remove_catalog_entries_or_existing_selections() {
        let path = Path::new("/project");
        let mut catalog = ModelCatalog::default();
        catalog
            .projects
            .entry(path.into())
            .or_default()
            .codex
            .models = vec![future_model()];
        let choices = catalog.choices(AgentKind::Codex, path);
        let saved = choices[1].selection();
        let hidden = choices
            .iter()
            .map(ModelChoice::favorite_key)
            .collect::<Vec<_>>();
        let visible = catalog.visible_choices(AgentKind::Codex, path, &hidden);
        assert_eq!(visible.len(), 1); // Automatic always remains available.
        assert!(visible[0].selection().external_model_id.is_none());
        assert_eq!(catalog.choices(AgentKind::Codex, path).len(), 2);
        assert_eq!(saved.model_cli_value(), Some("future-codex"));
        assert_eq!(
            catalog.visible_choices(AgentKind::Codex, path, &[]).len(),
            2
        );
    }

    #[test]
    fn refreshing_or_failed_catalog_keeps_last_successful_models_and_rejections_are_scoped() {
        let path = Path::new("/project");
        let other = Path::new("/other");
        let mut catalog = ModelCatalog::default();
        for path in [path, other] {
            catalog
                .projects
                .entry(path.into())
                .or_default()
                .codex
                .models = vec![future_model()];
        }
        let codex = &mut catalog.projects.get_mut(path).unwrap().codex;
        codex.loading = true;
        assert_eq!(catalog.choices(AgentKind::Codex, path).len(), 2);
        let codex = &mut catalog.projects.get_mut(path).unwrap().codex;
        codex.loading = false;
        codex.error = Some("Connection unavailable".into());
        assert_eq!(catalog.choices(AgentKind::Codex, path).len(), 2);
        catalog.reject_codex_model(path, "future-codex");
        assert_eq!(catalog.choices(AgentKind::Codex, path).len(), 1);
        assert_eq!(catalog.choices(AgentKind::Codex, other).len(), 2);
    }
}
