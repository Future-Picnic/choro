use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{Context, EventEmitter, PathPromptOptions};
use ide_core::config::{
    AppConfig, CompanionMusicSettings, ConversationLayout, GenerationAgent, GitStatusGroupMode,
    GitStatusViewMode, NavStyle, NewAgentDefaults, NotificationSettings, PanelSizes,
    ReviewChecklistMode, ThemeMode, VerificationMode, VoiceSettings,
};
use ide_core::local_store::LocalStore;
use ide_core::{GitWorkflow, GitWorkflowRun, Project, ProjectId, ProjectSection, ProjectSectionId};
use uuid::Uuid;

const SAVE_DEBOUNCE: Duration = Duration::from_millis(500);
const LEGACY_CODE_REVIEW_OUTPUT_PREFIX: &str =
    "\n\nPut your entire review inside a single <code_review>";

fn user_code_review_prompt(prompt: String) -> String {
    if let Some((instructions, _)) = prompt.split_once(LEGACY_CODE_REVIEW_OUTPUT_PREFIX) {
        instructions.trim_end().to_string()
    } else {
        prompt
    }
}

pub enum WorkspaceEvent {
    ProjectsChanged,
    ActiveChanged,
}

/// Root application state: the list of managed projects and which one is active.
pub struct Workspace {
    pub projects: Vec<Project>,
    pub project_sections: Vec<ProjectSection>,
    pub active: Option<ProjectId>,
    pub panels: PanelSizes,
    pub theme: ThemeMode,
    pub theme_name: Option<String>,
    pub git_status_view: GitStatusViewMode,
    pub git_status_group: GitStatusGroupMode,
    pub conversation_layout: ConversationLayout,
    pub notifications: NotificationSettings,
    pub companion_enabled: bool,
    pub companion_music: CompanionMusicSettings,
    pub voice: VoiceSettings,
    pub generation_agent: GenerationAgent,
    /// `None` = config predates the composer/generation split; readers fall
    /// back to the generation agent via [`Self::new_agent_defaults`].
    stored_new_agent_defaults: Option<NewAgentDefaults>,
    pub code_review_prompt: String,
    pub code_review_output_instructions: String,
    pub memory_proposals_enabled: bool,
    pub verification_mode: VerificationMode,
    pub review_checklist_mode: ReviewChecklistMode,
    pub design_browser_open_prompt_dismissed: bool,
    pub keymap: std::collections::HashMap<String, String>,
    pub expanded_projects: HashSet<ProjectId>,
    pub favorites_collapsed: bool,
    pub projects_collapsed: bool,
    pub attention_collapsed: bool,
    save_scheduled: bool,
}

impl EventEmitter<WorkspaceEvent> for Workspace {}

impl Workspace {
    pub fn load() -> Self {
        let legacy_config = AppConfig::load();
        let config = match LocalStore::open_default()
            .and_then(|store| store.load_workspace_config(legacy_config.clone()))
        {
            Ok(config) => config,
            Err(error) => {
                eprintln!("failed to load local store workspace; using config.json: {error:#}");
                legacy_config
            }
        };
        let mut theme_name = config.theme_name.clone();
        let migrated_legacy_theme =
            theme_name.as_deref() == Some(crate::theme::LEGACY_SIGNATURE_THEME);
        if migrated_legacy_theme {
            theme_name = Some(crate::theme::SIGNATURE_THEME.to_string());
        }
        let mut projects = config.projects;
        let project_sections = config.project_sections;
        let section_ids: HashSet<ProjectSectionId> =
            project_sections.iter().map(|section| section.id).collect();
        let mut migrated_legacy_icons = false;
        for project in &mut projects {
            migrated_legacy_icons |= project.migrate_legacy_icon_image();
            if project
                .section_id
                .is_some_and(|section_id| !section_ids.contains(&section_id))
            {
                project.section_id = None;
            }
            if project.is_favorite {
                project.section_id = None;
            }
        }
        let active = config
            .active_project
            .filter(|id| projects.iter().any(|p| p.id == *id))
            .or_else(|| projects.first().map(|p| p.id));
        let expanded_projects = config
            .expanded_projects
            .into_iter()
            .filter(|id| projects.iter().any(|project| project.id == *id))
            .collect();
        let mut workspace = Self {
            projects,
            project_sections,
            active,
            panels: config.panels,
            theme: config.theme,
            theme_name,
            git_status_view: config.git_status_view,
            git_status_group: config.git_status_group,
            conversation_layout: config.conversation_layout,
            notifications: config.notifications,
            companion_enabled: config.companion_enabled,
            companion_music: config.companion_music,
            voice: config.voice,
            generation_agent: config.generation_agent.normalized(),
            stored_new_agent_defaults: config.new_agent_defaults.map(NewAgentDefaults::normalized),
            code_review_prompt: user_code_review_prompt(config.code_review_prompt),
            code_review_output_instructions: config.code_review_output_instructions,
            memory_proposals_enabled: config.memory_proposals_enabled,
            verification_mode: config.verification_mode,
            review_checklist_mode: config.review_checklist_mode,
            design_browser_open_prompt_dismissed: config.design_browser_open_prompt_dismissed,
            keymap: config.keymap,
            expanded_projects,
            favorites_collapsed: config.favorites_collapsed,
            projects_collapsed: config.projects_collapsed,
            attention_collapsed: config.attention_collapsed,
            save_scheduled: false,
        };
        if migrated_legacy_icons || migrated_legacy_theme {
            workspace.save_now();
        }
        workspace
    }

    pub fn set_keymap(
        &mut self,
        keymap: std::collections::HashMap<String, String>,
        cx: &mut Context<Self>,
    ) {
        self.keymap = keymap;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_theme(&mut self, theme: ThemeMode, cx: &mut Context<Self>) {
        self.theme = theme;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_theme_name(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.theme_name = name;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_git_status_view(&mut self, view: GitStatusViewMode, cx: &mut Context<Self>) {
        if self.git_status_view == view {
            return;
        }
        self.git_status_view = view;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_git_status_group(&mut self, group: GitStatusGroupMode, cx: &mut Context<Self>) {
        if self.git_status_group == group {
            return;
        }
        self.git_status_group = group;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_conversation_layout(&mut self, layout: ConversationLayout, cx: &mut Context<Self>) {
        if self.conversation_layout == layout {
            return;
        }
        self.conversation_layout = layout;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_notification_settings(
        &mut self,
        notifications: NotificationSettings,
        cx: &mut Context<Self>,
    ) {
        if self.notifications == notifications {
            return;
        }
        self.notifications = notifications;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_companion_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.companion_enabled == enabled {
            return;
        }
        self.companion_enabled = enabled;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_companion_music_settings(
        &mut self,
        companion_music: CompanionMusicSettings,
        cx: &mut Context<Self>,
    ) {
        if self.companion_music == companion_music {
            return;
        }
        self.companion_music = companion_music;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_memory_proposals_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.memory_proposals_enabled == enabled {
            return;
        }
        self.memory_proposals_enabled = enabled;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_verification_mode(&mut self, mode: VerificationMode, cx: &mut Context<Self>) {
        if self.verification_mode == mode {
            return;
        }
        self.verification_mode = mode;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_review_checklist_mode(&mut self, mode: ReviewChecklistMode, cx: &mut Context<Self>) {
        if self.review_checklist_mode == mode {
            return;
        }
        self.review_checklist_mode = mode;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn dismiss_design_browser_open_prompt(&mut self, cx: &mut Context<Self>) {
        if self.design_browser_open_prompt_dismissed {
            return;
        }
        self.design_browser_open_prompt_dismissed = true;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_generation_agent(
        &mut self,
        generation_agent: GenerationAgent,
        cx: &mut Context<Self>,
    ) {
        let generation_agent = generation_agent.normalized();
        if self.generation_agent == generation_agent {
            return;
        }
        self.generation_agent = generation_agent;
        self.schedule_save(cx);
        cx.notify();
    }

    /// What a fresh agent composer opens with. Configs from before the split
    /// fall back to the generation agent, which used to double as this.
    pub fn new_agent_defaults(&self) -> NewAgentDefaults {
        self.stored_new_agent_defaults.clone().unwrap_or_else(|| {
            let generation = &self.generation_agent;
            NewAgentDefaults {
                provider: generation.provider,
                model: generation.model,
                effort: generation.model.default_effort(),
                external_model_id: generation.external_model_id.clone(),
                external_model_label: generation.external_model_label.clone(),
            }
        })
    }

    pub fn set_new_agent_defaults(&mut self, defaults: NewAgentDefaults, cx: &mut Context<Self>) {
        let defaults = defaults.normalized();
        if self.stored_new_agent_defaults.as_ref() == Some(&defaults) {
            return;
        }
        self.stored_new_agent_defaults = Some(defaults);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_code_review_prompt(&mut self, prompt: String, cx: &mut Context<Self>) {
        let prompt = user_code_review_prompt(prompt);
        if self.code_review_prompt == prompt {
            return;
        }
        self.code_review_prompt = prompt;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn effective_code_review_prompt(&self) -> &str {
        if self.code_review_prompt.trim().is_empty() {
            ide_core::config::DEFAULT_CODE_REVIEW_PROMPT
        } else {
            &self.code_review_prompt
        }
    }

    pub fn effective_code_review_output_instructions(&self) -> &str {
        if self.code_review_output_instructions.trim().is_empty() {
            ide_core::config::DEFAULT_CODE_REVIEW_OUTPUT_INSTRUCTIONS
        } else {
            &self.code_review_output_instructions
        }
    }

    pub fn save_now(&mut self) {
        self.save_scheduled = false;
        let config = self.to_config();
        if let Err(error) =
            LocalStore::open_default().and_then(|store| store.save_workspace_config(&config))
        {
            eprintln!("failed to save workspace to local store: {error:#}");
        }
        if let Err(error) = config.save() {
            eprintln!("failed to save config: {error:#}");
        }
    }

    pub fn active_project(&self) -> Option<&Project> {
        self.active
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
    }

    pub fn set_active(&mut self, id: ProjectId, cx: &mut Context<Self>) {
        if self.active == Some(id) {
            return;
        }
        self.active = Some(id);
        cx.emit(WorkspaceEvent::ActiveChanged);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn add_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(existing) = self.projects.iter().find(|p| p.path == path) {
            let id = existing.id;
            self.set_active(id, cx);
            return;
        }
        let project = Project::from_path(path);
        let id = project.id;
        self.projects.push(project);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.set_active(id, cx);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn remove_project(&mut self, id: ProjectId, cx: &mut Context<Self>) {
        self.projects.retain(|p| p.id != id);
        self.expanded_projects.remove(&id);
        if self.active == Some(id) {
            self.active = self.projects.first().map(|p| p.id);
            cx.emit(WorkspaceEvent::ActiveChanged);
        }
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn rename_project(&mut self, id: ProjectId, name: String, cx: &mut Context<Self>) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if let Some(project) = self.projects.iter_mut().find(|p| p.id == id) {
            if project.name == name {
                return;
            }
            project.name = name.to_string();
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn set_project_visual(
        &mut self,
        id: ProjectId,
        icon: String,
        icon_color: String,
        icon_image_path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        if let Some(project) = self.projects.iter_mut().find(|project| project.id == id) {
            if project.icon == icon
                && project.icon_color == icon_color
                && project.icon_image_path == icon_image_path
            {
                return;
            }
            project.icon = icon;
            project.icon_color = icon_color;
            project.icon_image_path = icon_image_path;
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn toggle_project_expanded(&mut self, id: ProjectId, cx: &mut Context<Self>) {
        if !self.expanded_projects.remove(&id) {
            self.expanded_projects.insert(id);
        }
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn add_section(&mut self, name: String, cx: &mut Context<Self>) -> ProjectSectionId {
        let name = self.unique_section_name(&name, None);
        let section = ProjectSection::new(name);
        let id = section.id;
        self.project_sections.push(section);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        id
    }

    pub fn rename_section(&mut self, id: ProjectSectionId, name: String, cx: &mut Context<Self>) {
        let name = self.unique_section_name(&name, Some(id));
        if let Some(section) = self
            .project_sections
            .iter_mut()
            .find(|section| section.id == id)
        {
            if section.name == name {
                return;
            }
            section.name = name;
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn delete_section(&mut self, id: ProjectSectionId, cx: &mut Context<Self>) {
        let before = self.project_sections.len();
        self.project_sections.retain(|section| section.id != id);
        if self.project_sections.len() == before {
            return;
        }
        for project in &mut self.projects {
            if project.section_id == Some(id) {
                project.section_id = None;
            }
        }
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn toggle_section_collapsed(&mut self, id: ProjectSectionId, cx: &mut Context<Self>) {
        if let Some(section) = self
            .project_sections
            .iter_mut()
            .find(|section| section.id == id)
        {
            section.collapsed = !section.collapsed;
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn move_section_up(&mut self, id: ProjectSectionId, cx: &mut Context<Self>) {
        let Some(ix) = self
            .project_sections
            .iter()
            .position(|section| section.id == id)
        else {
            return;
        };
        if ix == 0 {
            return;
        }
        self.project_sections.swap(ix, ix - 1);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn move_section_down(&mut self, id: ProjectSectionId, cx: &mut Context<Self>) {
        let Some(ix) = self
            .project_sections
            .iter()
            .position(|section| section.id == id)
        else {
            return;
        };
        if ix + 1 >= self.project_sections.len() {
            return;
        }
        self.project_sections.swap(ix, ix + 1);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn move_section_before(
        &mut self,
        dragged: ProjectSectionId,
        target: ProjectSectionId,
        cx: &mut Context<Self>,
    ) {
        if dragged == target {
            return;
        }
        let Some(from) = self
            .project_sections
            .iter()
            .position(|section| section.id == dragged)
        else {
            return;
        };
        let Some(mut to) = self
            .project_sections
            .iter()
            .position(|section| section.id == target)
        else {
            return;
        };
        let section = self.project_sections.remove(from);
        if from < to {
            to -= 1;
        }
        self.project_sections.insert(to, section);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn toggle_favorites_collapsed(&mut self, cx: &mut Context<Self>) {
        self.favorites_collapsed = !self.favorites_collapsed;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn toggle_projects_collapsed(&mut self, cx: &mut Context<Self>) {
        self.projects_collapsed = !self.projects_collapsed;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn toggle_attention_collapsed(&mut self, cx: &mut Context<Self>) {
        self.attention_collapsed = !self.attention_collapsed;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_project_favorite(&mut self, id: ProjectId, favorite: bool, cx: &mut Context<Self>) {
        if let Some(project) = self.projects.iter_mut().find(|project| project.id == id) {
            if project.is_favorite == favorite && (!favorite || project.section_id.is_none()) {
                return;
            }
            project.is_favorite = favorite;
            if favorite {
                project.section_id = None;
            }
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn move_project_to_section(
        &mut self,
        id: ProjectId,
        section_id: Option<ProjectSectionId>,
        cx: &mut Context<Self>,
    ) {
        if section_id
            .is_some_and(|id| !self.project_sections.iter().any(|section| section.id == id))
        {
            return;
        }
        if let Some(project) = self.projects.iter_mut().find(|project| project.id == id) {
            if project.section_id == section_id && !project.is_favorite {
                return;
            }
            project.section_id = section_id;
            project.is_favorite = false;
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn open_folder_dialog(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                this.update(cx, |workspace, cx| {
                    for path in paths {
                        workspace.add_project(path, cx);
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn update_presets(
        &mut self,
        id: ProjectId,
        presets: Vec<ide_core::ScriptPreset>,
        cx: &mut Context<Self>,
    ) {
        if let Some(project) = self.projects.iter_mut().find(|p| p.id == id) {
            project.presets = presets;
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    /// Pull only run-script presets from the store after an external Choro
    /// integration may have changed them. Other in-memory workspace settings
    /// remain authoritative and are intentionally left untouched.
    pub fn refresh_project_presets_from_store(
        &mut self,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<bool> {
        let persisted = LocalStore::open_default()?
            .load_workspace_config(AppConfig::default())?
            .projects;
        let mut changed = false;
        for project in &mut self.projects {
            let Some(saved) = persisted.iter().find(|saved| saved.id == project.id) else {
                continue;
            };
            if project.presets != saved.presets {
                project.presets = saved.presets.clone();
                changed = true;
            }
        }
        if changed {
            cx.emit(WorkspaceEvent::ProjectsChanged);
            cx.notify();
        }
        Ok(changed)
    }

    pub fn update_db_connections(
        &mut self,
        id: ProjectId,
        connections: Vec<ide_core::DbConnection>,
        cx: &mut Context<Self>,
    ) {
        if let Some(project) = self.projects.iter_mut().find(|p| p.id == id) {
            project.db_connections = connections;
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn update_task_tracker_connections(
        &mut self,
        id: ProjectId,
        connections: Vec<ide_core::TaskTrackerConnection>,
        cx: &mut Context<Self>,
    ) {
        if let Some(project) = self.projects.iter_mut().find(|p| p.id == id) {
            project.task_tracker_connections = connections;
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn add_git_workflow(
        &mut self,
        project_id: ProjectId,
        workflow: GitWorkflow,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Uuid> {
        let project = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        project.validate_git_workflow(&workflow)?;
        let id = workflow.id;
        project.git_workflows.push(workflow);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        Ok(id)
    }

    pub fn update_git_workflow(
        &mut self,
        project_id: ProjectId,
        workflow: GitWorkflow,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let project = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        project.validate_git_workflow(&workflow)?;
        let existing = project
            .git_workflows
            .iter_mut()
            .find(|existing| existing.id == workflow.id)
            .ok_or_else(|| anyhow::anyhow!("Workflow not found"))?;
        *existing = workflow;
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        Ok(())
    }

    pub fn duplicate_git_workflow(
        &mut self,
        project_id: ProjectId,
        workflow_id: Uuid,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<Uuid> {
        let project = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        let mut workflow = project
            .git_workflows
            .iter()
            .find(|workflow| workflow.id == workflow_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Workflow not found"))?;
        workflow.id = Uuid::new_v4();
        let base = format!("{} copy", workflow.name.trim());
        workflow.name = base.clone();
        for suffix in 2.. {
            if !project.git_workflows.iter().any(|existing| {
                existing.repository_path == workflow.repository_path
                    && existing.name.eq_ignore_ascii_case(&workflow.name)
            }) {
                break;
            }
            workflow.name = format!("{base} {suffix}");
        }
        let id = workflow.id;
        project.validate_git_workflow(&workflow)?;
        project.git_workflows.push(workflow);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        Ok(id)
    }

    pub fn delete_git_workflow(
        &mut self,
        project_id: ProjectId,
        workflow_id: Uuid,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
        else {
            return false;
        };
        let before = project.git_workflows.len();
        project
            .git_workflows
            .retain(|workflow| workflow.id != workflow_id);
        if project.git_workflows.len() == before {
            return false;
        }
        for run in &mut project.git_workflow_runs {
            if run.workflow_id == Some(workflow_id) {
                run.workflow_id = None;
            }
        }
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        true
    }

    pub fn reorder_git_workflows(
        &mut self,
        project_id: ProjectId,
        repository_path: &std::path::Path,
        ordered_ids: &[Uuid],
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        let project = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        let existing_ids: HashSet<Uuid> = project
            .git_workflows
            .iter()
            .filter(|workflow| workflow.repository_path == repository_path)
            .map(|workflow| workflow.id)
            .collect();
        let requested_ids: HashSet<Uuid> = ordered_ids.iter().copied().collect();
        if existing_ids != requested_ids || ordered_ids.len() != requested_ids.len() {
            anyhow::bail!("Workflow order must contain each repository workflow exactly once");
        }
        let selected: std::collections::HashMap<Uuid, GitWorkflow> = project
            .git_workflows
            .iter()
            .filter(|workflow| workflow.repository_path == repository_path)
            .cloned()
            .map(|workflow| (workflow.id, workflow))
            .collect();
        let mut ordered = ordered_ids.iter().map(|id| selected[id].clone());
        for workflow in &mut project.git_workflows {
            if workflow.repository_path == repository_path {
                *workflow = ordered.next().expect("validated workflow order");
            }
        }
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        Ok(())
    }

    pub fn upsert_git_workflow_run(
        &mut self,
        project_id: ProjectId,
        mut run: GitWorkflowRun,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<()> {
        run.validate()?;
        let project = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| anyhow::anyhow!("Project not found"))?;
        if run.workflow_id.is_some_and(|workflow_id| {
            !project
                .git_workflows
                .iter()
                .any(|workflow| workflow.id == workflow_id)
        }) {
            run.workflow_id = None;
        }
        if let Some(existing) = project
            .git_workflow_runs
            .iter_mut()
            .find(|existing| existing.id == run.id)
        {
            *existing = run;
        } else {
            project.git_workflow_runs.push(run);
        }
        project.prune_git_workflow_runs(30);
        cx.emit(WorkspaceEvent::ProjectsChanged);
        self.schedule_save(cx);
        cx.notify();
        Ok(())
    }

    #[allow(
        dead_code,
        reason = "used by maintenance and future explicit pruning UI"
    )]
    pub fn prune_git_workflow_runs(&mut self, project_id: ProjectId, cx: &mut Context<Self>) {
        let Some(project) = self
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
        else {
            return;
        };
        let before = project.git_workflow_runs.len();
        project.prune_git_workflow_runs(30);
        if project.git_workflow_runs.len() != before {
            cx.emit(WorkspaceEvent::ProjectsChanged);
            self.schedule_save(cx);
            cx.notify();
        }
    }

    pub fn set_left_panel_size(&mut self, left: f32, cx: &mut Context<Self>) {
        if (self.panels.left - left).abs() < 0.5 {
            return;
        }
        self.panels.left = left;
        self.schedule_save(cx);
    }

    pub fn set_right_panel_size(&mut self, right: f32, cx: &mut Context<Self>) {
        if (self.panels.right - right).abs() < 0.5 {
            return;
        }
        self.panels.right = right;
        self.schedule_save(cx);
    }

    pub fn set_voice_announcements(
        &mut self,
        announcements: ide_core::VoiceAnnouncements,
        cx: &mut Context<Self>,
    ) {
        if self.voice.announcements == announcements {
            return;
        }
        self.voice.announcements = announcements;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_voice_patient_turn_taking(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.voice.patient_turn_taking == enabled {
            return;
        }
        self.voice.patient_turn_taking = enabled;
        self.schedule_save(cx);
        cx.notify();
    }

    pub fn set_voice_input_device(&mut self, input_device: Option<String>, cx: &mut Context<Self>) {
        if self.voice.input_device == input_device {
            return;
        }
        self.voice.input_device = input_device;
        self.schedule_save(cx);
        cx.notify();
    }

    fn to_config(&self) -> AppConfig {
        AppConfig {
            projects: self.projects.clone(),
            project_sections: self.project_sections.clone(),
            active_project: self.active,
            panels: self.panels.clone(),
            theme: self.theme,
            theme_name: self.theme_name.clone(),
            nav_style: NavStyle::RailRight,
            git_status_view: self.git_status_view,
            git_status_group: self.git_status_group,
            conversation_layout: self.conversation_layout,
            notifications: self.notifications,
            companion_enabled: self.companion_enabled,
            companion_music: self.companion_music.clone(),
            voice: self.voice.clone(),
            generation_agent: self.generation_agent.clone(),
            new_agent_defaults: self.stored_new_agent_defaults.clone(),
            code_review_prompt: self.code_review_prompt.clone(),
            code_review_output_instructions: self.code_review_output_instructions.clone(),
            memory_proposals_enabled: self.memory_proposals_enabled,
            verification_mode: self.verification_mode,
            review_checklist_mode: self.review_checklist_mode,
            design_browser_open_prompt_dismissed: self.design_browser_open_prompt_dismissed,
            keymap: self.keymap.clone(),
            expanded_projects: self.expanded_projects.iter().copied().collect(),
            favorites_collapsed: self.favorites_collapsed,
            projects_collapsed: self.projects_collapsed,
            attention_collapsed: self.attention_collapsed,
        }
    }

    fn unique_section_name(&self, name: &str, current: Option<ProjectSectionId>) -> String {
        let base = if name.trim().is_empty() {
            "New Section"
        } else {
            name.trim()
        };
        let exists = |candidate: &str| {
            self.project_sections.iter().any(|section| {
                Some(section.id) != current && section.name.eq_ignore_ascii_case(candidate)
            })
        };
        if !exists(base) {
            return base.to_string();
        }
        for ix in 2.. {
            let candidate = format!("{base} {ix}");
            if !exists(&candidate) {
                return candidate;
            }
        }
        unreachable!()
    }

    fn schedule_save(&mut self, cx: &mut Context<Self>) {
        if self.save_scheduled {
            return;
        }
        self.save_scheduled = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SAVE_DEBOUNCE).await;
            let config = this
                .update(cx, |workspace, _| {
                    workspace.save_scheduled = false;
                    workspace.to_config()
                })
                .ok();
            if let Some(config) = config {
                cx.background_executor()
                    .spawn(async move {
                        if let Err(error) = LocalStore::open_default()
                            .and_then(|store| store.save_workspace_config(&config))
                        {
                            eprintln!("failed to save workspace to local store: {error:#}");
                        }
                        if let Err(error) = config.save() {
                            eprintln!("failed to save config: {error:#}");
                        }
                    })
                    .await;
            }
        })
        .detach();
    }
}
