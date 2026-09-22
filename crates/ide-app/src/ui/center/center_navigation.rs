use super::*;

struct SavedProjectView {
    mode: CenterMode,
    context: ContextMode,
    last_code_mode: CenterMode,
    back: Vec<CenterMode>,
    forward: Vec<CenterMode>,
}

impl Default for SavedProjectView {
    fn default() -> Self {
        Self {
            mode: CenterMode::Agents,
            context: ContextMode::Docs,
            last_code_mode: CenterMode::Split,
            back: Vec::new(),
            forward: Vec::new(),
        }
    }
}

/// Session memory for each project's activity and navigation history.
pub(super) struct ProjectNavigation {
    active: Option<ProjectId>,
    saved: HashMap<ProjectId, SavedProjectView>,
}

impl ProjectNavigation {
    pub(super) fn new(active: Option<ProjectId>) -> Self {
        Self {
            active,
            saved: HashMap::new(),
        }
    }

    fn switch_to(
        &mut self,
        project: Option<ProjectId>,
        current: SavedProjectView,
    ) -> SavedProjectView {
        if let Some(previous) = self.active {
            self.saved.insert(previous, current);
        }
        self.active = project;
        project
            .and_then(|id| self.saved.remove(&id))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod project_navigation_tests {
    use super::*;

    #[test]
    fn project_navigation_restores_docs_and_each_projects_history() {
        let a = ProjectId::new();
        let b = ProjectId::new();
        let mut navigation = ProjectNavigation::new(Some(a));
        let docs = SavedProjectView {
            mode: CenterMode::Docs,
            back: vec![CenterMode::Agents],
            forward: vec![CenterMode::Tasks],
            ..Default::default()
        };
        let first_visit = navigation.switch_to(Some(b), docs);
        assert!(first_visit.mode == CenterMode::Agents);
        assert!(first_visit.back.is_empty());
        let tasks = SavedProjectView {
            mode: CenterMode::Tasks,
            ..Default::default()
        };
        let restored = navigation.switch_to(Some(a), tasks);
        assert!(restored.mode == CenterMode::Docs);
        assert!(restored.context == ContextMode::Docs);
        assert!(restored.back == [CenterMode::Agents]);
        assert!(restored.forward == [CenterMode::Tasks]);
        assert!(navigation.switch_to(Some(b), restored).mode == CenterMode::Tasks);
    }

    #[test]
    fn project_navigation_keeps_assets_and_code_layouts_separate() {
        let a = ProjectId::new();
        let b = ProjectId::new();
        let mut navigation = ProjectNavigation::new(Some(a));
        navigation.switch_to(
            Some(b),
            SavedProjectView {
                mode: CenterMode::Docs,
                context: ContextMode::Designs,
                ..Default::default()
            },
        );
        let assets = navigation.switch_to(
            Some(a),
            SavedProjectView {
                mode: CenterMode::Files,
                last_code_mode: CenterMode::Files,
                ..Default::default()
            },
        );
        assert!(assets.mode == CenterMode::Docs);
        assert!(assets.context == ContextMode::Designs);
        let code = navigation.switch_to(Some(b), assets);
        assert!(code.mode == CenterMode::Files);
        assert!(code.last_code_mode == CenterMode::Files);
    }

    #[test]
    fn project_navigation_remembers_explicit_navigation_after_restoring_a_project() {
        let a = ProjectId::new();
        let b = ProjectId::new();
        let mut navigation = ProjectNavigation::new(Some(a));
        navigation.switch_to(
            Some(b),
            SavedProjectView {
                mode: CenterMode::Docs,
                ..Default::default()
            },
        );
        let a_view = navigation.switch_to(
            Some(a),
            SavedProjectView {
                mode: CenterMode::Tasks,
                ..Default::default()
            },
        );
        let mut b_view = navigation.switch_to(Some(b), a_view);
        assert!(b_view.mode == CenterMode::Tasks);
        // An explicit agent link overrides B's restored Tasks view.
        b_view.mode = CenterMode::Agents;
        let a_view = navigation.switch_to(Some(a), b_view);
        assert!(a_view.mode == CenterMode::Docs);
        assert!(navigation.switch_to(Some(b), a_view).mode == CenterMode::Agents);
    }
}

impl CenterArea {
    /// Synchronize before explicit navigation as well as workspace observation:
    /// opening a doc/agent may switch project and view in the same event.
    pub(super) fn sync_project_navigation(&mut self, cx: &mut Context<Self>) {
        let project = self.workspace.read(cx).active;
        if self.project_navigation.active == project {
            return;
        }
        let current = SavedProjectView {
            mode: self.view_mode,
            context: self.context_mode,
            last_code_mode: self.last_code_mode,
            back: std::mem::take(&mut self.view_history_back),
            forward: std::mem::take(&mut self.view_history_forward),
        };
        let restored = self.project_navigation.switch_to(project, current);
        if self.penpot_compare_open {
            self.close_penpot_compare(cx);
        }
        self.view_mode = restored.mode;
        self.context_mode = restored.context;
        self.last_code_mode = restored.last_code_mode;
        self.view_history_back = restored.back;
        self.view_history_forward = restored.forward;
        if self.view_mode == CenterMode::Tasks {
            self.refresh_active_task_board(cx);
            self.start_tasks_auto_refresh(cx);
        }
        cx.notify();
    }

    /// App-level routes are painted by RootView, above the center panel. Native
    /// WKWebViews sit above GPUI itself, so they must be explicitly hidden while
    /// one of those routes owns the window.
    pub fn set_web_preview_suspended(
        &mut self,
        suspended: bool,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.web_host
            .update(cx, |host, _| host.set_suspended(suspended));
        self.compare_web_host
            .update(cx, |host, _| host.set_suspended(suspended));
        if suspended {
            web_preview::restore_focus(window);
        }
        cx.notify();
    }

    pub fn activity(&self) -> ProjectActivity {
        let activity = self.view_mode.activity();
        // The Docs center mode covers both docs and designs; the active context
        // mode picks which activity it reports as.
        if activity == ProjectActivity::Docs && self.context_mode == ContextMode::Designs {
            ProjectActivity::Designs
        } else {
            activity
        }
    }

    pub fn agents_panel_reset_epoch(&self) -> u64 {
        self.agents_panel_reset_epoch
    }

    pub fn git_diff_open_epoch(&self) -> u64 {
        self.git_diff_open_epoch
    }

    pub fn can_go_back(&self) -> bool {
        !self.view_history_back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.view_history_forward.is_empty()
    }

    pub(super) fn apply_view_mode(&mut self, mode: CenterMode) {
        if mode.activity() == ProjectActivity::Code {
            self.last_code_mode = mode;
        }
        self.view_mode = mode;
    }

    pub fn set_view_mode(&mut self, mode: CenterMode, cx: &mut Context<Self>) {
        if self.view_mode == CenterMode::Design && mode != CenterMode::Design {
            if self.defer_canvas_navigation(move|this,cx|this.set_view_mode(mode,cx),cx){return;}
            if let Some(studio) = self.studio.as_mut().filter(|s| s.dirty || (s.saving&&s.screen.is_some())) {
                studio.pending_mode = Some(mode);
                let reply = serde_json::json!({"session":studio.editor_session,"type":"flush"});
                self.web_host
                    .update(cx, |host, _| host.studio_reply(&reply));
                cx.notify();
                return;
            }
        }
        if self.view_mode==CenterMode::Design&&mode!=CenterMode::Design {self.flush_studio_canvas();}
        self.sync_project_navigation(cx);
        if self.view_mode == mode {
            return;
        }
        if mode != CenterMode::Design && self.penpot_compare_open {
            self.close_penpot_compare(cx);
        }
        self.view_history_back.push(self.view_mode);
        if self.view_history_back.len() > 80 {
            self.view_history_back.remove(0);
        }
        self.view_history_forward.clear();
        self.apply_view_mode(mode);
        cx.notify();
    }

    pub fn go_back(&mut self, cx: &mut Context<Self>) {
        self.sync_project_navigation(cx);
        let Some(previous) = self.view_history_back.pop() else {
            return;
        };
        if previous != CenterMode::Design && self.penpot_compare_open {
            self.close_penpot_compare(cx);
        }
        self.view_history_forward.push(self.view_mode);
        if self.view_history_forward.len() > 80 {
            self.view_history_forward.remove(0);
        }
        self.apply_view_mode(previous);
        if self.view_mode == CenterMode::Tasks {
            self.refresh_active_task_board(cx);
            self.start_tasks_auto_refresh(cx);
        }
        cx.notify();
    }

    pub fn go_forward(&mut self, cx: &mut Context<Self>) {
        self.sync_project_navigation(cx);
        let Some(next) = self.view_history_forward.pop() else {
            return;
        };
        if next != CenterMode::Design && self.penpot_compare_open {
            self.close_penpot_compare(cx);
        }
        self.view_history_back.push(self.view_mode);
        if self.view_history_back.len() > 80 {
            self.view_history_back.remove(0);
        }
        self.apply_view_mode(next);
        if self.view_mode == CenterMode::Tasks {
            self.refresh_active_task_board(cx);
            self.start_tasks_auto_refresh(cx);
        }
        cx.notify();
    }

    /// Switch to a project activity from any of the navigation surfaces (title
    /// tabs, the left rail, or the top-right switcher) so they stay in sync.
    pub fn show_activity(&mut self, activity: ProjectActivity, cx: &mut Context<Self>) {
        match activity {
            ProjectActivity::Code => self.show_code(cx),
            ProjectActivity::Agents => self.show_agents(cx),
            ProjectActivity::PocketComet => self.show_pocketcomet(cx),
            ProjectActivity::Tasks => self.show_tasks(cx),
            ProjectActivity::Db => self.show_db(cx),
            ProjectActivity::Docs => self.set_context_mode(ContextMode::Docs, cx),
            ProjectActivity::Designs => self.set_context_mode(ContextMode::Designs, cx),
            ProjectActivity::Design => self.show_design(cx),
            ProjectActivity::Services => self.show_services(cx),
        }
    }

    pub fn show_code(&mut self, cx: &mut Context<Self>) {
        self.sync_project_navigation(cx);
        self.set_view_mode(self.last_code_mode, cx);
    }

    pub fn show_agents(&mut self, cx: &mut Context<Self>) {
        // Entering Agents is also a request to restore its default side
        // panel (Git), even when Agents is already the active center view.
        self.agents_panel_reset_epoch = self.agents_panel_reset_epoch.wrapping_add(1);
        self.set_view_mode(CenterMode::Agents, cx);
        cx.notify();
    }

    pub fn show_pocketcomet(&mut self, cx: &mut Context<Self>) {
        self.set_view_mode(CenterMode::PocketComet, cx);
    }

    pub fn show_tasks(&mut self, cx: &mut Context<Self>) {
        self.set_view_mode(CenterMode::Tasks, cx);
        self.refresh_active_task_board(cx);
        self.start_tasks_auto_refresh(cx);
    }

    pub fn show_my_tasks(&mut self, cx: &mut Context<Self>) {
        self.set_view_mode(CenterMode::MyTasks, cx);
        self.tasks
            .update(cx, |tasks, cx| tasks.refresh_my_tasks(cx));
    }

    pub fn is_my_tasks_view(&self) -> bool {
        self.view_mode == CenterMode::MyTasks
    }

    /// Open the global Quick Ask archive in the center workspace. Keep the
    /// current selection when it is still present; otherwise land on the
    /// newest conversation so the detail pane is immediately useful.
    pub fn show_quick_ask_history(&mut self, cx: &mut Context<Self>) {
        let history = self.quick_ask.read(cx).history();
        let selected_is_present = self.quick_ask_selected_session.is_some_and(|session_id| {
            history
                .iter()
                .any(|exchange| exchange.session_id == session_id)
        });
        if !selected_is_present {
            self.quick_ask_selected_session = history.first().map(|exchange| exchange.session_id);
        }

        let already_open = self.view_mode == CenterMode::QuickAskHistory;
        self.set_view_mode(CenterMode::QuickAskHistory, cx);
        if already_open {
            cx.notify();
        }
    }

    pub fn is_quick_ask_history_view(&self) -> bool {
        self.view_mode == CenterMode::QuickAskHistory
    }

    /// Tapping a task in the right nav opens its detail (leaving board view).
    pub fn show_task_detail(&mut self, cx: &mut Context<Self>) {
        self.tasks_detail_collapsed = false;
        self.show_tasks(cx);
    }

    /// Tapping a source in the right nav shows the whole board (not a task).
    pub fn show_board_overview(&mut self, cx: &mut Context<Self>) {
        self.tasks_detail_collapsed = true;
        self.show_tasks(cx);
    }

    pub fn show_db(&mut self, cx: &mut Context<Self>) {
        self.set_view_mode(CenterMode::Db, cx);
    }

    pub fn show_services(&mut self, cx: &mut Context<Self>) {
        self.set_view_mode(CenterMode::Services, cx);
    }

    pub fn show_design(&mut self, cx: &mut Context<Self>) {
        if let Some((project, _)) = self.active_project(cx) {
            self.refresh_studio_catalog(project, cx);
        }
        let active_project = self.active_project(cx).map(|(project, _)| project);
        let has_open_design = active_project.is_some_and(|project| {
            self.studio.as_ref().is_some_and(|studio| studio.project == project) || self.penpot_open_design
                .is_some_and(|(open_project, _)| open_project == project)
                || self
                    .figma_open_design
                    .is_some_and(|(open_project, _)| open_project == project)
        });
        if !has_open_design {
            if let Some(project) = active_project {
                self.penpot.update(cx, |penpot, cx| {
                    penpot.refresh_design_thumbnails(project, cx)
                });
            }
        }
        self.set_view_mode(CenterMode::Design, cx);
        self.reconnect_penpot(cx);
        cx.notify();
    }

    pub fn show_docs(&mut self, cx: &mut Context<Self>) {
        self.set_context_mode(ContextMode::Docs, cx);
    }

    pub fn toggle_terminal_area(&mut self, cx: &mut Context<Self>) {
        if self.activity() != ProjectActivity::Code {
            return;
        }
        let mode = match self.view_mode {
            CenterMode::Split => CenterMode::Files,
            CenterMode::Files => CenterMode::Split,
            CenterMode::Terminal => CenterMode::Files,
            _ => return,
        };
        self.set_view_mode(mode, cx);
    }

    pub fn set_context_mode(&mut self, mode: ContextMode, cx: &mut Context<Self>) {
        self.sync_project_navigation(cx);
        // Always switch to the Docs center view — Docs and Designs are now
        // separate activities reachable from anywhere, so selecting one must
        // navigate even when the context mode itself is unchanged.
        self.context_mode = mode;
        self.set_view_mode(CenterMode::Docs, cx);
        cx.notify();
    }

    pub fn show_onboarding_preview(&mut self, url: &str, cx: &mut Context<Self>) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let reference = self
            .designs
            .read(cx)
            .references_for_project(project)
            .into_iter()
            .find(|reference| reference.source == url);

        self.set_context_mode(ContextMode::Designs, cx);
        if let Some(reference) = reference {
            self.designs
                .update(cx, |designs, cx| designs.select(project, reference.id, cx));
        }
    }

    pub fn toggle_selected_agent_plan_mode(&mut self, cx: &mut Context<Self>) {
        if self.view_mode != CenterMode::Agents {
            return;
        }
        // While the new-agent composer is open it owns plan mode: Shift+Tab
        // toggles the draft, just as it toggles a live agent's session. The chip
        // only appears once plan is on, so this is how you turn it on.
        if let Some(composer) = self.new_agent_composer.as_mut() {
            if crate::ui::onboarding::locks_onboarding_plan_mode(cx) {
                return;
            }
            composer.interaction_mode = if composer.interaction_mode == AgentInteractionMode::Plan {
                AgentInteractionMode::Default
            } else {
                AgentInteractionMode::Plan
            };
            composer.error = None;
            cx.notify();
            return;
        }
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let Some(agent) = self.agents.read(cx).selected_agent(project) else {
            return;
        };
        if agent.runtime != AgentRuntimeKind::Chat {
            return;
        }
        if crate::ui::onboarding::locks_onboarding_plan_mode(cx) {
            return;
        }
        let title = agent.title.clone();
        let mode = if self
            .agent_chats
            .read(cx)
            .session(agent.id)
            .is_some_and(|s| s.interaction_mode == AgentInteractionMode::Plan)
        {
            AgentInteractionMode::Default
        } else {
            AgentInteractionMode::Plan
        };
        if !self.set_expert_plan_mode(agent.id, mode, cx) {
            return;
        }
        self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(agent.id, title, cx);
            session.interaction_mode = mode;
            cx.notify();
        });
        cx.notify();
    }
}
