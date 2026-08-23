use std::collections::HashMap;
use std::sync::Arc;

use gpui::{AppContext, Context, Entity, EventEmitter};
use ide_core::{
    local_store::{
        LocalStore, OrbitBuiltin, OrbitModuleDefinition, OrbitModuleId, OrbitProjectBinding,
        OrbitProjectModuleSnapshot, OrbitRecord, OrbitRecordInput, OrbitSnapshot,
    },
    ProjectId,
};
use uuid::Uuid;

use super::{Workspace, WorkspaceEvent};

pub struct OrbitState {
    workspace: Entity<Workspace>,
    modules: Vec<OrbitModuleDefinition>,
    bindings: HashMap<ProjectId, Vec<OrbitProjectBinding>>,
    records: HashMap<(ProjectId, Uuid), Arc<Vec<OrbitRecord>>>,
    selected: HashMap<ProjectId, OrbitModuleId>,
    pending_enabled: HashMap<(ProjectId, OrbitModuleId), bool>,
    loading: bool,
    saving: bool,
    error: Option<String>,
    generation: u64,
    module_refresh_generations: HashMap<(ProjectId, Uuid), u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbitEvent {
    ModuleSaved {
        module_id: Uuid,
        request_id: Uuid,
    },
    RecordSaved {
        project: ProjectId,
        module_id: Uuid,
        record_id: Uuid,
        request_id: Uuid,
    },
}

impl EventEmitter<OrbitEvent> for OrbitState {}

impl OrbitState {
    pub fn view(workspace: Entity<Workspace>, cx: &mut gpui::App) -> Entity<Self> {
        let state = cx.new(|cx| {
            cx.subscribe(
                &workspace,
                |this: &mut Self, _workspace, event: &WorkspaceEvent, cx| {
                    if matches!(event, WorkspaceEvent::ProjectsChanged) {
                        this.refresh(cx);
                    }
                },
            )
            .detach();
            Self {
                workspace: workspace.clone(),
                modules: Vec::new(),
                bindings: HashMap::new(),
                records: HashMap::new(),
                selected: HashMap::new(),
                pending_enabled: HashMap::new(),
                loading: false,
                saving: false,
                error: None,
                generation: 0,
                module_refresh_generations: HashMap::new(),
            }
        });
        state.update(cx, |state, cx| state.refresh(cx));
        state
    }

    pub fn modules(&self) -> &[OrbitModuleDefinition] {
        &self.modules
    }

    pub fn module(&self, id: Uuid) -> Option<&OrbitModuleDefinition> {
        self.modules.iter().find(|module| module.id == id)
    }

    pub fn loading(&self) -> bool {
        self.loading
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn bindings(&self, project: ProjectId) -> &[OrbitProjectBinding] {
        self.bindings
            .get(&project)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn records(&self, project: ProjectId, module: Uuid) -> &[OrbitRecord] {
        self.records
            .get(&(project, module))
            .map(|records| records.as_slice())
            .unwrap_or_default()
    }

    pub fn records_shared(&self, project: ProjectId, module: Uuid) -> Arc<Vec<OrbitRecord>> {
        self.records
            .get(&(project, module))
            .cloned()
            .unwrap_or_else(|| Arc::new(Vec::new()))
    }

    pub fn enabled(&self, project: ProjectId, module: OrbitModuleId) -> bool {
        if let Some(enabled) = self.pending_enabled.get(&(project, module)) {
            return *enabled;
        }
        self.bindings(project)
            .iter()
            .find(|binding| binding.module == module)
            .map(|binding| binding.enabled)
            .unwrap_or_else(|| matches!(module, OrbitModuleId::Builtin(_)))
    }

    pub fn visible_modules(&self, project: ProjectId) -> Vec<OrbitModuleId> {
        let mut result = Vec::new();
        for builtin in OrbitBuiltin::ALL {
            let module = OrbitModuleId::Builtin(builtin);
            if self.enabled(project, module) {
                result.push(module);
            }
        }
        let mut custom = self
            .bindings(project)
            .iter()
            .filter_map(|binding| match binding.module {
                OrbitModuleId::Custom(id)
                    if self.enabled(project, binding.module)
                        && self
                            .module(id)
                            .is_some_and(|definition| !definition.archived) =>
                {
                    Some((binding.sort_order, OrbitModuleId::Custom(id)))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        custom.sort_by_key(|(order, _)| *order);
        result.extend(custom.into_iter().map(|(_, module)| module));
        result
    }

    pub fn active_custom_modules(&self, project: ProjectId) -> Vec<&OrbitModuleDefinition> {
        self.visible_modules(project)
            .into_iter()
            .filter_map(|module| match module {
                OrbitModuleId::Custom(id) => self.module(id),
                OrbitModuleId::Builtin(_) => None,
            })
            .collect()
    }

    pub fn selected(&self, project: ProjectId) -> Option<OrbitModuleId> {
        let visible = self.visible_modules(project);
        super::preferred_selection(self.selected.get(&project), &visible)
    }

    pub fn select(&mut self, project: ProjectId, module: OrbitModuleId, cx: &mut Context<Self>) {
        if self.visible_modules(project).contains(&module) {
            self.selected.insert(project, module);
            cx.notify();
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let projects = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .map(|project| project.id)
            .collect::<Vec<_>>();
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        self.loading = true;
        self.error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { LocalStore::open_default()?.load_orbit_snapshot(&projects) })
                .await;
            this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                this.loading = false;
                match loaded {
                    Ok(OrbitSnapshot {
                        modules,
                        bindings,
                        records,
                    }) => {
                        this.modules = modules;
                        this.bindings = bindings;
                        this.records = records
                            .into_iter()
                            .map(|(scope, records)| (scope, Arc::new(records)))
                            .collect();
                        this.pending_enabled.clear();
                        this.error = None;
                    }
                    Err(error) => this.error = Some(format!("Could not load Orbit: {error:#}")),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn begin_save(&mut self, cx: &mut Context<Self>) {
        // Invalidate any older full snapshot before a targeted mutation can
        // commit, so a late refresh cannot overwrite the newer local result.
        self.generation = self.generation.wrapping_add(1);
        self.saving = true;
        self.error = None;
        cx.notify();
    }

    fn finish_save(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.saving = false;
        self.error = None;
        cx.notify();
    }

    fn upsert_module(&mut self, module: OrbitModuleDefinition) {
        if let Some(existing) = self
            .modules
            .iter_mut()
            .find(|existing| existing.id == module.id)
        {
            *existing = module;
        } else {
            self.modules.push(module);
        }
        self.modules.sort_by(|left, right| {
            left.archived
                .cmp(&right.archived)
                .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
        });
    }

    fn upsert_binding(&mut self, binding: OrbitProjectBinding) {
        let bindings = self.bindings.entry(binding.project_id).or_default();
        if let Some(existing) = bindings
            .iter_mut()
            .find(|existing| existing.module == binding.module)
        {
            *existing = binding;
        } else {
            bindings.push(binding);
        }
        bindings.sort_by_key(|binding| binding.sort_order);
    }

    fn apply_project_module_snapshot(&mut self, snapshot: OrbitProjectModuleSnapshot) {
        let project = snapshot.binding.project_id;
        let module = match snapshot.binding.module {
            OrbitModuleId::Custom(module) => module,
            OrbitModuleId::Builtin(_) => return,
        };
        let enabled = snapshot.binding.enabled;
        self.upsert_binding(snapshot.binding);
        if enabled {
            self.records
                .insert((project, module), Arc::new(snapshot.records));
        } else {
            self.records.remove(&(project, module));
        }
    }

    fn invalidate_module_refresh(&mut self, project: ProjectId, module_id: Uuid) -> u64 {
        let generation = self
            .module_refresh_generations
            .entry((project, module_id))
            .or_default();
        *generation = generation.wrapping_add(1);
        *generation
    }

    pub fn refresh_module(&mut self, project: ProjectId, module_id: Uuid, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.invalidate_module_refresh(project, module_id);
        cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default()?
                        .load_orbit_project_module_snapshot(project, module_id)
                })
                .await;
            this.update(cx, |this, cx| {
                if this
                    .module_refresh_generations
                    .get(&(project, module_id))
                    .copied()
                    != Some(generation)
                {
                    return;
                }
                match loaded {
                    Ok(snapshot) => {
                        this.apply_project_module_snapshot(snapshot);
                        this.error = None;
                    }
                    Err(error) => {
                        this.error = Some(format!("Could not refresh Orbit module: {error:#}"));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn set_enabled(
        &mut self,
        project: ProjectId,
        module: OrbitModuleId,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        let previous_selection = self.selected.get(&project).copied();
        if enabled {
            // Adding a view is an explicit navigation action: once the binding
            // refresh lands, open the newly added module instead of leaving the
            // user on the previously selected built-in.
            self.selected.insert(project, module);
        }
        self.pending_enabled.insert((project, module), enabled);
        if let OrbitModuleId::Custom(module_id) = module {
            self.invalidate_module_refresh(project, module_id);
        }
        self.begin_save(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = LocalStore::open_default()?;
                    let binding =
                        store.set_project_orbit_module_enabled(project, module, enabled)?;
                    let records = match module {
                        OrbitModuleId::Custom(module_id) if enabled => {
                            store.load_orbit_records(project, module_id)?
                        }
                        _ => Vec::new(),
                    };
                    Ok::<_, anyhow::Error>((binding, records))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((binding, records)) => {
                    this.pending_enabled.remove(&(project, module));
                    this.upsert_binding(binding);
                    if let OrbitModuleId::Custom(module_id) = module {
                        this.invalidate_module_refresh(project, module_id);
                        if enabled {
                            this.records.insert((project, module_id), Arc::new(records));
                        } else {
                            this.records.remove(&(project, module_id));
                        }
                    }
                    this.finish_save(cx);
                }
                Err(error) => {
                    this.saving = false;
                    this.pending_enabled.remove(&(project, module));
                    if enabled && this.selected.get(&project) == Some(&module) {
                        match previous_selection {
                            Some(previous) => {
                                this.selected.insert(project, previous);
                            }
                            None => {
                                this.selected.remove(&project);
                            }
                        }
                    }
                    this.error = Some(format!("Could not update Orbit: {error:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn save_module(
        &mut self,
        module: OrbitModuleDefinition,
        request_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let module_id = module.id;
        let affected_projects = self
            .bindings
            .iter()
            .filter_map(|(project, bindings)| {
                bindings
                    .iter()
                    .any(|binding| {
                        binding.module == OrbitModuleId::Custom(module_id) && binding.enabled
                    })
                    .then_some(*project)
            })
            .collect::<Vec<_>>();
        for project in &affected_projects {
            self.invalidate_module_refresh(*project, module_id);
        }
        self.begin_save(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = LocalStore::open_default()?;
                    let saved = store.save_orbit_module(&module)?;
                    let snapshots =
                        store.load_orbit_module_project_snapshots(&affected_projects, module_id)?;
                    Ok::<_, anyhow::Error>((saved, snapshots))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((saved, snapshots)) => {
                    let module_id = saved.id;
                    this.upsert_module(saved);
                    for snapshot in snapshots {
                        this.invalidate_module_refresh(snapshot.binding.project_id, module_id);
                        this.apply_project_module_snapshot(snapshot);
                    }
                    cx.emit(OrbitEvent::ModuleSaved {
                        module_id,
                        request_id,
                    });
                    this.finish_save(cx);
                }
                Err(error) => {
                    this.saving = false;
                    this.error = Some(format!("Could not save module: {error:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn set_archived(&mut self, module_id: Uuid, archived: bool, cx: &mut Context<Self>) {
        self.begin_save(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default()?.set_orbit_module_archived(module_id, archived)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(module) => {
                    this.upsert_module(module);
                    this.finish_save(cx);
                }
                Err(error) => {
                    this.saving = false;
                    this.error = Some(format!("Could not update module: {error:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn save_record(
        &mut self,
        project: ProjectId,
        module_id: Uuid,
        input: OrbitRecordInput,
        request_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_module_refresh(project, module_id);
        self.begin_save(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = LocalStore::open_default()?;
                    let saved = store.save_orbit_record(project, module_id, input)?;
                    let snapshot = store.load_orbit_project_module_snapshot(project, module_id)?;
                    Ok::<_, anyhow::Error>((saved, snapshot))
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok((saved, snapshot)) => {
                    this.invalidate_module_refresh(project, module_id);
                    this.apply_project_module_snapshot(snapshot);
                    cx.emit(OrbitEvent::RecordSaved {
                        project,
                        module_id,
                        record_id: saved.id,
                        request_id,
                    });
                    this.finish_save(cx);
                }
                Err(error) => {
                    this.saving = false;
                    this.error = Some(format!("Could not save record: {error:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn delete_record(
        &mut self,
        project: ProjectId,
        module_id: Uuid,
        record_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        self.invalidate_module_refresh(project, module_id);
        self.begin_save(cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let store = LocalStore::open_default()?;
                    store.delete_orbit_record(project, module_id, record_id)?;
                    store.load_orbit_project_module_snapshot(project, module_id)
                })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(snapshot) => {
                    this.invalidate_module_refresh(project, module_id);
                    this.apply_project_module_snapshot(snapshot);
                    this.finish_save(cx);
                }
                Err(error) => {
                    this.saving = false;
                    this.error = Some(format!("Could not delete record: {error:#}"));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_selection_has_a_real_empty_state_and_uses_visible_order() {
        assert_eq!(
            super::super::preferred_selection::<OrbitModuleId>(None, &[]),
            None
        );

        let custom = OrbitModuleId::Custom(Uuid::new_v4());
        let visible = [OrbitModuleId::Builtin(OrbitBuiltin::Integrations), custom];
        assert_eq!(
            super::super::preferred_selection(None, &visible),
            Some(visible[0])
        );
        assert_eq!(
            super::super::preferred_selection(Some(&custom), &visible),
            Some(custom)
        );
        assert_eq!(
            super::super::preferred_selection(
                Some(&OrbitModuleId::Builtin(OrbitBuiltin::Environment)),
                &visible,
            ),
            Some(visible[0]),
        );
    }
}
