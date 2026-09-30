use super::*;

impl CenterArea {
    /// Selects and focuses an existing terminal session.
    pub fn focus_terminal(
        &mut self,
        project: ProjectId,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminals
            .update(cx, |manager, cx| manager.set_active(project, id, cx));
        if matches!(
            self.view_mode,
            CenterMode::Files
                | CenterMode::Agents
                | CenterMode::Tasks
                | CenterMode::Db
                | CenterMode::Docs
                | CenterMode::Design
                | CenterMode::PocketComet
        ) {
            self.set_view_mode(CenterMode::Split, cx);
        }
        let manager = self.terminals.read(cx);
        if let Some(session) = manager.sessions.iter().find(|s| s.id == id) {
            session.view.read(cx).focus_handle().clone().focus(window);
        }
        cx.notify();
    }

    pub(super) fn active_project(&self, cx: &App) -> Option<(ProjectId, PathBuf)> {
        self.workspace
            .read(cx)
            .active_project()
            .map(|p| (p.id, p.path.clone()))
    }

    pub(super) fn project_by_id(
        &self,
        project: ProjectId,
        cx: &App,
    ) -> Option<(ProjectId, PathBuf)> {
        self.workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .map(|p| (p.id, p.path.clone()))
    }

    // ----- terminals -----

    pub fn spawn_shell(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project, cwd)) = self.active_project(cx) else {
            return;
        };
        let code_mode = if self.active_file_sel(project).is_some() {
            CenterMode::Split
        } else {
            CenterMode::Terminal
        };
        self.set_view_mode(code_mode, cx);
        let spawned = self
            .terminals
            .update(cx, |manager, cx| manager.spawn_shell(project, cwd, cx));
        self.focus_spawned(spawned, window, cx);
    }

    pub fn run_preset(
        &mut self,
        name: &str,
        command: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((project, cwd)) = self.active_project(cx) else {
            return;
        };
        self.set_view_mode(CenterMode::Split, cx);
        let spawned = self.terminals.update(cx, |manager, cx| {
            manager.spawn_preset(project, cwd, name, command, cx)
        });
        let started = spawned.is_ok();
        self.focus_spawned(spawned, window, cx);
        if started {
            crate::ui::onboarding::emit_for_project(
                project,
                crate::ui::onboarding::OnboardingEvent::ScriptStarted,
                cx,
            );
        }
    }

    /// Selects an existing preset terminal without changing the visible
    /// workspace or moving keyboard focus away from the user's current task.
    pub fn select_terminal_in_background(
        &mut self,
        project: ProjectId,
        id: SessionId,
        cx: &mut Context<Self>,
    ) {
        self.terminals
            .update(cx, |manager, cx| manager.set_active(project, id, cx));
        cx.notify();
    }

    /// Starts a top-header script without navigating to Code/Terminal. The
    /// terminal remains available and selected if the user opens Code later.
    pub fn run_preset_in_background(&mut self, name: &str, command: &str, cx: &mut Context<Self>) {
        let Some((project, cwd)) = self.active_project(cx) else {
            return;
        };
        let spawned = self.terminals.update(cx, |manager, cx| {
            manager.spawn_preset(project, cwd, name, command, cx)
        });
        match spawned {
            Ok(_) => {
                crate::ui::onboarding::emit_for_project(
                    project,
                    crate::ui::onboarding::OnboardingEvent::ScriptStarted,
                    cx,
                );
                cx.notify();
            }
            Err(error) => eprintln!("failed to spawn terminal: {error:#}"),
        }
    }

    pub(super) fn focus_spawned(
        &mut self,
        spawned: anyhow::Result<SessionId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match spawned {
            Ok(id) => {
                let manager = self.terminals.read(cx);
                if let Some(session) = manager.sessions.iter().find(|s| s.id == id) {
                    session.view.read(cx).focus_handle().clone().focus(window);
                }
                cx.notify();
            }
            Err(error) => eprintln!("failed to spawn terminal: {error:#}"),
        }
    }

    // ----- app-owned agents -----
}
