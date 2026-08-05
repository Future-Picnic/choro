use super::*;

impl CenterArea {
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
            ProjectActivity::Tasks => self.show_tasks(cx),
            ProjectActivity::Db => self.show_db(cx),
            ProjectActivity::Docs => self.set_context_mode(ContextMode::Docs, cx),
            ProjectActivity::Designs => self.set_context_mode(ContextMode::Designs, cx),
            ProjectActivity::Design => self.show_design(cx),
            ProjectActivity::Services => self.show_services(cx),
        }
    }

    pub fn show_code(&mut self, cx: &mut Context<Self>) {
        self.set_view_mode(self.last_code_mode, cx);
    }

    pub fn show_agents(&mut self, cx: &mut Context<Self>) {
        // Entering Agents is also a request to restore its default side
        // panel (Git), even when Agents is already the active center view.
        self.agents_panel_reset_epoch = self.agents_panel_reset_epoch.wrapping_add(1);
        self.set_view_mode(CenterMode::Agents, cx);
        cx.notify();
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
        let active_project = self.active_project(cx).map(|(project, _)| project);
        let has_open_design = active_project.is_some_and(|project| {
            self.penpot_open_design
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
        self.set_view_mode(CenterMode::Docs, cx);
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
        self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(agent.id, title, cx);
            session.interaction_mode = if session.interaction_mode == AgentInteractionMode::Plan {
                AgentInteractionMode::Default
            } else {
                AgentInteractionMode::Plan
            };
            cx.notify();
        });
        cx.notify();
    }
}
