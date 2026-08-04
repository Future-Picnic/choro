use super::*;

impl CenterArea {
    pub fn open_file(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(item) = self
            .editors
            .iter()
            .find(|e| e.project == project && e.path == path)
        {
            item.input.update(cx, |input, cx| input.focus(window, cx));
            self.selected_file.insert(project, FileSel::Editor(path));
            if self.view_mode.activity() != ProjectActivity::Code {
                self.set_view_mode(CenterMode::Split, cx);
            }
            cx.notify();
            return;
        }

        // Make sure the editor section is actually visible.
        if matches!(
            self.view_mode,
            CenterMode::Terminal
                | CenterMode::Agents
                | CenterMode::Tasks
                | CenterMode::Db
                | CenterMode::Docs
                | CenterMode::Design
        ) {
            self.set_view_mode(CenterMode::Split, cx);
        }
        self.selected_file
            .insert(project, FileSel::Editor(path.clone()));
        cx.notify();

        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let meta = std::fs::metadata(&path)?;
                    if meta.len() > MAX_EDITOR_FILE_BYTES {
                        anyhow::bail!("file too large for editor: {} bytes", meta.len());
                    }
                    let modified_at = meta.modified().ok();
                    let content = std::fs::read_to_string(&path)?;
                    Ok::<_, anyhow::Error>((path, content, modified_at))
                })
                .await;

            this.update(cx, |this, cx| {
                match result {
                    Ok((path, content, modified_at)) => {
                        if let Some(item) = this
                            .editors
                            .iter()
                            .find(|e| e.project == project && e.path == path)
                        {
                            window_handle
                                .update(cx, |_, window, cx| {
                                    item.input.update(cx, |input, cx| input.focus(window, cx));
                                })
                                .ok();
                            this.selected_file.insert(project, FileSel::Editor(path));
                            cx.notify();
                            return;
                        }

                        let language = language_for(&path);
                        let input = match window_handle.update(cx, |_, window, cx| {
                            let input = cx.new(|cx| {
                                InputState::new(window, cx)
                                    .code_editor(language)
                                    .default_value(content)
                            });
                            input.update(cx, |input, cx| input.focus(window, cx));
                            input
                        }) {
                            Ok(input) => input,
                            Err(error) => {
                                eprintln!("cannot focus editor window: {error}");
                                return;
                            }
                        };

                        let editor_path = path.clone();
                        let editor_project = project;
                        let target_position = this
                            .pending_editor_positions
                            .remove(&(project, path.clone()));
                        let target_input = input.clone();
                        cx.subscribe(&input, move |this: &mut Self, _, event: &InputEvent, cx| {
                            match event {
                                InputEvent::Change => {
                                    if let Some(item) = this.editors.iter_mut().find(|e| {
                                        e.project == editor_project && e.path == editor_path
                                    }) {
                                        item.dirty = true;
                                    }
                                    cx.notify();
                                }
                                _ => {}
                            }
                        })
                        .detach();

                        let title: SharedString = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| path.display().to_string())
                            .into();
                        let cursor_status =
                            cx.new(|cx| EditorCursorStatus::new(input.clone(), language, cx));
                        this.editors.push(EditorItem {
                            project,
                            path: path.clone(),
                            title,
                            input,
                            cursor_status,
                            dirty: false,
                            saving: false,
                            modified_at,
                        });
                        this.selected_file.insert(project, FileSel::Editor(path));
                        if let Some((line_number, column)) = target_position {
                            window_handle
                                .update(cx, |_, window, cx| {
                                    target_input.update(cx, |input, cx| {
                                        input.set_cursor_position(
                                            Position::new(
                                                line_number.saturating_sub(1) as u32,
                                                column.saturating_sub(1) as u32,
                                            ),
                                            window,
                                            cx,
                                        );
                                    });
                                })
                                .ok();
                        }
                    }
                    Err(error) => {
                        eprintln!("cannot open file: {error:#}");
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_file_at(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        line_number: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pending_editor_positions
            .insert((project, path.clone()), (line_number, column));
        self.open_file(project, path.clone(), window, cx);
        if self.focus_editor_position(project, &path, line_number, column, window, cx) {
            self.pending_editor_positions.remove(&(project, path));
        }
    }

    pub(super) fn focus_editor_position(
        &mut self,
        project: ProjectId,
        path: &PathBuf,
        line_number: usize,
        column: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(input) = self
            .editors
            .iter()
            .find(|e| e.project == project && &e.path == path)
            .map(|e| e.input.clone())
        else {
            return false;
        };

        let position = Position::new(
            line_number.saturating_sub(1) as u32,
            column.saturating_sub(1) as u32,
        );
        input.update(cx, |input, cx| {
            input.set_cursor_position(position, window, cx);
        });
        true
    }

    /// Opens a diff selected in the Git sidebar without replacing that sidebar
    /// with Files when the center transitions from Agents to Code.
    pub fn open_diff_from_git(
        &mut self,
        project: ProjectId,
        kind: DiffKind,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        self.git_diff_open_epoch = self.git_diff_open_epoch.wrapping_add(1);
        self.open_diff(project, kind, title, cx);
    }

    /// Opens (or focuses) a diff view tab for the project.
    pub fn open_diff(
        &mut self,
        project: ProjectId,
        kind: DiffKind,
        title: SharedString,
        cx: &mut Context<Self>,
    ) {
        let key = kind.key();
        if self
            .diffs
            .iter()
            .any(|d| d.project == project && d.key == key)
        {
            self.selected_file.insert(project, FileSel::Diff(key));
            if matches!(
                self.view_mode,
                CenterMode::Terminal
                    | CenterMode::Agents
                    | CenterMode::Tasks
                    | CenterMode::Db
                    | CenterMode::Docs
                    | CenterMode::Design
            ) {
                self.set_view_mode(CenterMode::Split, cx);
            }
            cx.notify();
            return;
        }
        let Some(git) = self.git_states.read(cx).get(project) else {
            return;
        };
        let repo = git.read(cx).repo_path.clone();
        let center = cx.weak_entity();
        let view = cx.new(|cx| DiffPane::new(project, repo, kind, Some(git.clone()), center, cx));
        self.diffs.push(DiffItem {
            project,
            key: key.clone(),
            title,
            view,
        });
        self.selected_file.insert(project, FileSel::Diff(key));
        if matches!(
            self.view_mode,
            CenterMode::Terminal
                | CenterMode::Agents
                | CenterMode::Tasks
                | CenterMode::Db
                | CenterMode::Docs
                | CenterMode::Design
        ) {
            self.set_view_mode(CenterMode::Split, cx);
        }
        cx.notify();
    }

    /// Opens (or focuses) a database object in the dedicated DB view.
    pub fn open_db_object(
        &mut self,
        handle: ide_core::DatabaseHandle,
        conn: ide_core::DbConnection,
        namespace: String,
        object: ide_core::DbObject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let key = format!("{}/{namespace}/{}", conn.id, object.name);
        if !self
            .db_views
            .iter()
            .any(|v| v.project == project && v.key == key)
        {
            let title: SharedString = format!("{} · {namespace}.{}", conn.name, object.name).into();
            let prod = conn.looks_like_prod();
            let view = match handle {
                ide_core::DatabaseHandle::Mongo(handle) => {
                    let collection = object.name.clone();
                    let child = cx.new(|cx| {
                        CollectionPane::new(
                            handle,
                            namespace.clone(),
                            collection,
                            prod,
                            conn.read_only,
                            window,
                            cx,
                        )
                    });
                    cx.new(|_| DatabasePane::documents(child))
                }
                ide_core::DatabaseHandle::Relational(handle) => {
                    let table = object.name.clone();
                    let child = cx.new(|cx| {
                        TablePane::new(
                            handle,
                            namespace.clone(),
                            table,
                            prod,
                            conn.read_only,
                            window,
                            cx,
                        )
                    });
                    cx.new(|_| DatabasePane::table(child))
                }
            };
            self.db_views.push(DbItem {
                project,
                key: key.clone(),
                title,
                view,
            });
        }
        self.selected_db.insert(project, key);
        self.set_view_mode(CenterMode::Db, cx);
        cx.notify();
    }

    /// The selected DB tab for a project (validated, with fallback).
    pub(super) fn active_db_key(&self, project: ProjectId) -> Option<String> {
        if let Some(key) = self.selected_db.get(&project) {
            if self
                .db_views
                .iter()
                .any(|v| v.project == project && &v.key == key)
            {
                return Some(key.clone());
            }
        }
        self.db_views
            .iter()
            .find(|v| v.project == project)
            .map(|v| v.key.clone())
    }

    /// The selected file-section tab for a project (validated, with fallback).
    pub(super) fn active_file_sel(&self, project: ProjectId) -> Option<FileSel> {
        if let Some(sel) = self.selected_file.get(&project) {
            let valid = match sel {
                FileSel::Editor(path) => self
                    .editors
                    .iter()
                    .any(|e| e.project == project && &e.path == path),
                FileSel::Diff(key) => self
                    .diffs
                    .iter()
                    .any(|d| d.project == project && &d.key == key),
            };
            if valid {
                return Some(sel.clone());
            }
        }
        self.editors
            .iter()
            .find(|e| e.project == project)
            .map(|e| FileSel::Editor(e.path.clone()))
            .or_else(|| {
                self.diffs
                    .iter()
                    .find(|d| d.project == project)
                    .map(|d| FileSel::Diff(d.key.clone()))
            })
    }

    /// The currently shown editor for a project (only when an editor tab is selected).
    pub(super) fn active_editor(&self, project: ProjectId) -> Option<&EditorItem> {
        match self.active_file_sel(project)? {
            FileSel::Editor(path) => self
                .editors
                .iter()
                .find(|e| e.project == project && e.path == path),
            FileSel::Diff(_) => None,
        }
    }

    pub fn save_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        let Some(path) = self.active_editor(project).map(|e| e.path.clone()) else {
            return;
        };
        let Some(item) = self
            .editors
            .iter_mut()
            .find(|e| e.project == project && e.path == path)
        else {
            return;
        };
        if item.saving {
            return;
        }
        let content = item.input.read(cx).value().to_string();
        let loaded_modified_at = item.modified_at;
        if Self::file_changed_on_disk(&path, loaded_modified_at) {
            self.confirm_overwrite_save(project, path, content, window, cx);
            return;
        }
        self.start_save(project, path, content, cx);
    }

    pub(super) fn file_changed_on_disk(
        path: &PathBuf,
        loaded_modified_at: Option<SystemTime>,
    ) -> bool {
        let Some(loaded_modified_at) = loaded_modified_at else {
            return false;
        };
        match std::fs::metadata(path).and_then(|meta| meta.modified()) {
            Ok(current) => current != loaded_modified_at,
            Err(_) => true,
        }
    }

    pub(super) fn start_save(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        content: String,
        cx: &mut Context<Self>,
    ) {
        let Some(item) = self
            .editors
            .iter_mut()
            .find(|e| e.project == project && e.path == path)
        else {
            return;
        };
        if item.saving {
            return;
        }
        item.saving = true;
        let file = item.path.clone();
        let git_states = self.git_states.clone();
        cx.spawn(async move |this, cx| {
            let write = cx
                .background_executor()
                .spawn(async move {
                    std::fs::write(&file, content.as_bytes())?;
                    let modified_at = std::fs::metadata(&file)
                        .and_then(|meta| meta.modified())
                        .ok();
                    std::io::Result::Ok((file, modified_at))
                })
                .await;
            this.update(cx, |this, cx| {
                match write {
                    Ok((saved_path, modified_at)) => {
                        if let Some(item) = this
                            .editors
                            .iter_mut()
                            .find(|e| e.project == project && e.path == saved_path)
                        {
                            item.saving = false;
                            item.dirty = false;
                            item.modified_at = modified_at;
                        }
                        // The .git watcher doesn't see worktree edits — refresh manually.
                        if let Some(git) = git_states.read(cx).get(project) {
                            git.update(cx, |git, cx| git.refresh(cx));
                        }
                    }
                    Err(error) => {
                        eprintln!("save failed: {error}");
                        for item in this.editors.iter_mut() {
                            item.saving = false;
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub(super) fn confirm_overwrite_save(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        content: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let center = cx.entity().clone();
        let name: SharedString = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        ConfirmDialog::new(
            "File changed on disk",
            "Another process changed this file after it was opened. Overwrite the disk version with your editor contents?",
        )
        .tone(ConfirmTone::Warning)
        .icon(IconName::TriangleAlert)
        .detail(name)
        .confirm_label("Overwrite")
        .confirm_id("confirm-overwrite-save")
        .cancel_label("Keep Editing")
        .width(460.0)
        .on_confirm(move |_, cx| {
            center.update(cx, |center, cx| {
                center.start_save(project, path.clone(), content.clone(), cx);
            });
        })
        .open(window, cx);
    }

    /// cmd-w: closes the active editor if any file is open, else the active terminal.
    pub fn close_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };
        // In the DB view, cmd-w closes the active collection viewer.
        if self.view_mode == CenterMode::Db {
            if let Some(key) = self.active_db_key(project) {
                self.close_db_view(project, &key, cx);
            }
            return;
        }
        if self.view_mode == CenterMode::Agents {
            let selected = self.agents.read(cx).selected_agent_id(project);
            let terminal = selected.and_then(|agent_id| {
                self.terminals
                    .read(cx)
                    .agent_record_terminal(project, agent_id)
            });
            if let Some(id) = terminal {
                self.terminals
                    .update(cx, |manager, cx| manager.close(id, cx));
            }
            return;
        }
        match self.active_file_sel(project) {
            Some(FileSel::Editor(_)) => self.request_close_editor(project, window, cx),
            Some(FileSel::Diff(key)) => self.close_diff(project, &key, cx),
            None => {
                let active = self
                    .terminals
                    .read(cx)
                    .active_session(project)
                    .map(|s| s.id);
                if let Some(id) = active {
                    self.terminals
                        .update(cx, |manager, cx| manager.close(id, cx));
                }
            }
        }
    }

    pub fn cycle_open_item(
        &mut self,
        direction: isize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.activity() != ProjectActivity::Code {
            return;
        }
        let Some((project, _)) = self.active_project(cx) else {
            return;
        };

        if self.view_mode == CenterMode::Terminal {
            let (ids, active) = {
                let terminals = self.terminals.read(cx);
                (
                    terminals
                        .sessions_for(project)
                        .into_iter()
                        .map(|session| session.id)
                        .collect::<Vec<_>>(),
                    terminals.active_session(project).map(|session| session.id),
                )
            };
            if ids.is_empty() {
                return;
            }
            let index = active
                .and_then(|active| ids.iter().position(|id| *id == active))
                .unwrap_or(0);
            let next = (index as isize + direction).rem_euclid(ids.len() as isize) as usize;
            self.focus_terminal(project, ids[next], window, cx);
            return;
        }

        let mut items = self
            .editors
            .iter()
            .filter(|editor| editor.project == project)
            .map(|editor| FileSel::Editor(editor.path.clone()))
            .collect::<Vec<_>>();
        items.extend(
            self.diffs
                .iter()
                .filter(|diff| diff.project == project)
                .map(|diff| FileSel::Diff(diff.key.clone())),
        );
        if items.is_empty() {
            return;
        }
        let index = self
            .active_file_sel(project)
            .and_then(|selected| items.iter().position(|item| item == &selected))
            .unwrap_or(0);
        let next = (index as isize + direction).rem_euclid(items.len() as isize) as usize;
        let selected = items[next].clone();
        self.selected_file.insert(project, selected.clone());
        if let FileSel::Editor(path) = selected {
            if let Some(input) = self
                .editors
                .iter()
                .find(|editor| editor.project == project && editor.path == path)
                .map(|editor| editor.input.clone())
            {
                input.update(cx, |input, cx| input.focus(window, cx));
            }
        }
        cx.notify();
    }

    pub(super) fn close_diff(&mut self, project: ProjectId, key: &str, cx: &mut Context<Self>) {
        self.diffs
            .retain(|d| !(d.project == project && d.key == key));
        self.selected_file.remove(&project);
        cx.notify();
    }

    pub(super) fn close_db_view(&mut self, project: ProjectId, key: &str, cx: &mut Context<Self>) {
        self.db_views
            .retain(|v| !(v.project == project && v.key == key));
        self.selected_db.remove(&project);
        cx.notify();
    }

    pub(super) fn request_close_editor(
        &mut self,
        project: ProjectId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((path, dirty)) = self
            .active_editor(project)
            .map(|e| (e.path.clone(), e.dirty))
        else {
            return;
        };
        if dirty {
            self.confirm_close_dirty(project, path, window, cx);
        } else {
            self.close_editor(project, &path, cx);
        }
    }

    pub(super) fn close_editor(
        &mut self,
        project: ProjectId,
        path: &PathBuf,
        cx: &mut Context<Self>,
    ) {
        self.editors
            .retain(|e| !(e.project == project && &e.path == path));
        self.selected_file.remove(&project);
        cx.notify();
    }

    pub(super) fn confirm_close_dirty(
        &mut self,
        project: ProjectId,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let center = cx.entity().clone();
        let name: SharedString = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
            .into();
        ConfirmDialog::new(
            "Unsaved changes",
            "Close this file and discard your local edits?",
        )
        .detail(name)
        .confirm_label("Discard")
        .confirm_id("discard-close")
        .cancel_label("Keep Editing")
        .width(420.0)
        .on_confirm(move |_, cx| {
            center.update(cx, |center, cx| {
                center.close_editor(project, &path, cx);
            });
        })
        .open(window, cx);
    }

    // ----- rendering -----

    pub(super) fn render_editor_section(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        // Tabs: editors first, then diff views.
        let mut tabs: Vec<(FileSel, SharedString, bool)> = self
            .editors
            .iter()
            .filter(|e| e.project == project)
            .map(|e| (FileSel::Editor(e.path.clone()), e.title.clone(), e.dirty))
            .collect();
        tabs.extend(self.diffs.iter().filter(|d| d.project == project).map(|d| {
            (
                FileSel::Diff(d.key.clone()),
                SharedString::from(format!("± {}", d.title)),
                false,
            )
        }));
        if tabs.is_empty() {
            return None;
        }

        let selection = self.active_file_sel(project)?;
        let selected_ix = tabs
            .iter()
            .position(|(sel, _, _)| sel == &selection)
            .unwrap_or(0);

        let project_root = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|item| item.id == project)
            .map(|item| item.path.clone());
        let has_terminals = self.has_terminal_content(project, cx);
        let content: gpui::AnyElement = match &selection {
            FileSel::Editor(path) => {
                let editor = self
                    .editors
                    .iter()
                    .find(|e| e.project == project && &e.path == path)?;
                let (input, cursor_status, dirty, saving) = (
                    editor.input.clone(),
                    editor.cursor_status.clone(),
                    editor.dirty,
                    editor.saving,
                );
                let relative_path = relative_editor_path(project_root.as_deref(), path);
                let parent_label = relative_path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(|parent| format!("{}/", parent.to_string_lossy()));
                let file_label = relative_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| relative_path.to_string_lossy().into_owned());
                let (status_label, status_color) = if saving {
                    ("Saving…", crate::ui::design::sky(cx))
                } else if dirty {
                    ("Modified", crate::ui::design::amber(cx))
                } else {
                    ("Saved", crate::ui::design::t4(cx))
                };
                v_flex()
                    .size_full()
                    .bg(crate::ui::design::base(cx))
                    .child(
                        h_flex()
                            .flex_none()
                            .w_full()
                            .h(crate::ui::design::editor_breadcrumb_h())
                            .px_3()
                            .gap_1p5()
                            .items_center()
                            .border_b_1()
                            .border_color(crate::ui::design::line(cx))
                            .bg(crate::ui::design::base(cx))
                            .child(
                                h_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        Icon::new(IconName::FileText)
                                            .size(crate::ui::design::icon_sm())
                                            .text_color(crate::ui::design::t4(cx)),
                                    )
                                    .when_some(parent_label, |row, parent| {
                                        row.child(
                                            div()
                                                .min_w(px(0.))
                                                .truncate()
                                                .font_family(crate::ui::design::FONT_MONO)
                                                .text_size(crate::ui::design::text_file())
                                                .text_color(crate::ui::design::t4(cx))
                                                .child(parent),
                                        )
                                    })
                                    .child(
                                        div()
                                            .flex_none()
                                            .font_family(crate::ui::design::FONT_MONO)
                                            .text_size(crate::ui::design::text_file())
                                            .text_color(crate::ui::design::t2(cx))
                                            .child(file_label),
                                    ),
                            )
                            .when(dirty, |row| {
                                row.child(
                                    style::ghost_button_compact(
                                        "save-file",
                                        if saving { "Saving…" } else { "Save" },
                                    )
                                    .disabled(saving)
                                    .tooltip("Save (⌘S)")
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.save_active(window, cx);
                                        },
                                    )),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h(px(0.))
                            .bg(crate::ui::design::base(cx))
                            .child(
                                Input::new(&input)
                                    .appearance(true)
                                    .bordered(false)
                                    .focus_bordered(false)
                                    .small()
                                    .h_full()
                                    .px(crate::ui::design::editor_pad_x())
                                    .py(crate::ui::design::editor_pad_y()),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_none()
                            .w_full()
                            .h(crate::ui::design::editor_status_h())
                            .px_3()
                            .gap_3()
                            .items_center()
                            .border_t_1()
                            .border_color(crate::ui::design::line(cx))
                            .bg(crate::ui::design::nav(cx))
                            .font_family(crate::ui::design::FONT_MONO)
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .child(
                                        div().w(px(6.)).h(px(6.)).rounded_full().bg(status_color),
                                    )
                                    .child(status_label),
                            )
                            .child(div().flex_1())
                            .child(cursor_status),
                    )
                    .into_any_element()
            }
            FileSel::Diff(key) => {
                let view = self
                    .diffs
                    .iter()
                    .find(|d| d.project == project && &d.key == key)?
                    .view
                    .clone();
                div().size_full().child(view).into_any_element()
            }
        };
        let code_mode_action = if has_terminals {
            self.render_code_mode_switch(cx)
        } else {
            self.render_code_terminal_action(cx)
        };
        let tab_menu_items = tabs.clone();
        let tab_menu_center = cx.entity().clone();
        let show_tab_menu = tabs.len() > 1;

        Some(
            v_flex()
                .size_full()
                .child(
                    h_flex()
                        .flex_none()
                        .w_full()
                        .h(crate::ui::design::editor_tab_bar_h())
                        .pl_1()
                        .pr_1()
                        .gap_0p5()
                        .items_center()
                        .border_b_1()
                        .border_color(crate::ui::design::line(cx))
                        .bg(crate::ui::design::nav(cx))
                        .child(
                            h_flex()
                                .id("editor-tabs")
                                .flex_1()
                                .min_w(px(0.))
                                .h_full()
                                .items_center()
                                .gap_0p5()
                                .overflow_x_scroll()
                                .children(tabs.into_iter().enumerate().map(
                                    |(ix, (selection, label, modified))| {
                                        style::editor_file_tab(
                                            ("editor-tab", ix),
                                            label,
                                            selected_ix == ix,
                                            modified,
                                            cx,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, window, cx| {
                                                if let FileSel::Editor(path) = &selection {
                                                    this.open_file(
                                                        project,
                                                        path.clone(),
                                                        window,
                                                        cx,
                                                    );
                                                } else {
                                                    this.selected_file
                                                        .insert(project, selection.clone());
                                                    cx.notify();
                                                }
                                            }),
                                        )
                                    },
                                )),
                        )
                        .child(style::toolbar_divider(cx))
                        .when(show_tab_menu, |bar| {
                            bar.child(
                                style::header_icon_button(
                                    "editor-tab-list",
                                    IconName::ChevronDown,
                                    cx,
                                )
                                .tooltip("Open files")
                                .dropdown_menu(
                                    move |mut menu, window, _| {
                                        for (ix, (selection, label, modified)) in
                                            tab_menu_items.iter().cloned().enumerate()
                                        {
                                            let center = tab_menu_center.clone();
                                            let menu_label: SharedString = if modified {
                                                format!("● {label}").into()
                                            } else {
                                                label
                                            };
                                            menu = menu.item(
                                                PopupMenuItem::new(menu_label)
                                                    .checked(selected_ix == ix)
                                                    .on_click(window.listener_for(
                                                        &center,
                                                        move |this: &mut Self, _, window, cx| {
                                                            if let FileSel::Editor(path) =
                                                                &selection
                                                            {
                                                                this.open_file(
                                                                    project,
                                                                    path.clone(),
                                                                    window,
                                                                    cx,
                                                                );
                                                            } else {
                                                                this.selected_file.insert(
                                                                    project,
                                                                    selection.clone(),
                                                                );
                                                                cx.notify();
                                                            }
                                                        },
                                                    )),
                                            );
                                        }
                                        menu
                                    },
                                ),
                            )
                        })
                        .child(code_mode_action)
                        .child(
                            style::header_icon_button("close-editor", IconName::Close, cx)
                                .tooltip("Close tab")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    match this.active_file_sel(project) {
                                        Some(FileSel::Editor(_)) => {
                                            this.request_close_editor(project, window, cx)
                                        }
                                        Some(FileSel::Diff(key)) => {
                                            this.close_diff(project, &key, cx)
                                        }
                                        None => {}
                                    }
                                })),
                        ),
                )
                .child(div().flex_1().min_h(px(0.)).child(content))
                .into_any_element(),
        )
    }

    pub(super) fn render_terminal_section(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let has_files = self.has_file_content(project);
        let manager = self.terminals.read(cx);
        let sessions: Vec<(SessionId, SharedString, bool)> = manager
            .sessions_for(project)
            .iter()
            .map(|s| (s.id, s.title.clone(), s.exited))
            .collect();

        if sessions.is_empty() {
            return v_flex()
                .size_full()
                .min_h(px(280.))
                .items_center()
                .justify_center()
                .gap_4()
                .on_action(cx.listener(|this, _: &NewTerminal, window, cx| {
                    this.spawn_shell(window, cx);
                }))
                .child(
                    div()
                        .w(px(160.))
                        .h(px(110.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::ui::illustrations::illustration(
                                crate::ui::illustrations::Illustration::CodeTerminal,
                                cx,
                            )
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_title())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child("Open a file or start a terminal"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child(
                                    "Choose a file from the Files sidebar, or create a terminal to begin.",
                                ),
                        ),
                )
                .child(
                    style::secondary_button("empty-new-terminal", "New Terminal")
                        .icon(IconName::Plus)
                        .tooltip("New terminal (⌘N)")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.spawn_shell(window, cx);
                        })),
                )
                .into_any_element();
        }

        let active = manager.active_session(project).map(|s| s.id);
        let selected_ix = sessions
            .iter()
            .position(|(id, _, _)| Some(*id) == active)
            .unwrap_or(0);
        let active_session = manager
            .sessions_for(project)
            .into_iter()
            .find(|s| Some(s.id) == active)
            .or_else(|| manager.sessions_for(project).into_iter().nth(selected_ix));
        let view = active_session.map(|s| s.view.clone());
        let exited_id = active_session.filter(|s| s.exited).map(|s| s.id);
        let close_id = active_session.map(|s| s.id);
        let ids: Vec<SessionId> = sessions.iter().map(|(id, _, _)| *id).collect();
        let code_mode_switch = (has_files && self.view_mode == CenterMode::Terminal)
            .then(|| self.render_code_mode_switch(cx));

        v_flex()
            .size_full()
            .on_action(cx.listener(|this, _: &NewTerminal, window, cx| {
                this.spawn_shell(window, cx);
            }))
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            TabBar::new("terminal-tabs")
                                .menu(true)
                                .w_full()
                                .selected_index(selected_ix)
                                .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                                    if let Some(id) = ids.get(*ix).copied() {
                                        this.terminals.update(cx, |manager, cx| {
                                            manager.set_active(project, id, cx);
                                        });
                                    }
                                }))
                                .children(sessions.iter().map(|(_, title, exited)| {
                                    let label: SharedString = if *exited {
                                        format!("{title} (exited)").into()
                                    } else {
                                        format!("⌁ {title}").into()
                                    };
                                    Tab::new().label(label)
                                })),
                        ),
                    )
                    .when_some(exited_id, |bar, id| {
                        bar.child(
                            style::header_icon_button("restart-terminal", IconName::Redo2, cx)
                                .tooltip("Restart")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.terminals.update(cx, |manager, cx| {
                                        if let Err(error) = manager.restart(id, cx) {
                                            eprintln!("restart failed: {error:#}");
                                        }
                                    });
                                })),
                        )
                    })
                    .when_some(close_id, |bar, id| {
                        bar.child(
                            style::header_icon_button("close-terminal", IconName::Close, cx)
                                .tooltip("Close terminal")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.terminals
                                        .update(cx, |manager, cx| manager.close(id, cx));
                                })),
                        )
                    })
                    .child(
                        style::header_icon_button("new-terminal", IconName::Plus, cx)
                            .tooltip("New terminal (⌘N)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.spawn_shell(window, cx);
                            })),
                    )
                    .children(code_mode_switch),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .when_some(view, |area, view| area.child(view)),
            )
            .into_any_element()
    }

    pub(super) fn render_db_section(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let tabs: Vec<(String, SharedString)> = self
            .db_views
            .iter()
            .filter(|v| v.project == project)
            .map(|v| (v.key.clone(), v.title.clone()))
            .collect();

        if tabs.is_empty() {
            let has_connections = self
                .workspace
                .read(cx)
                .projects
                .iter()
                .find(|item| item.id == project)
                .is_some_and(|item| !item.db_connections.is_empty());
            let workspace = self.workspace.clone();

            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_4()
                .child(
                    div()
                        .w(px(160.))
                        .h(px(110.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            crate::ui::illustrations::illustration(
                                crate::ui::illustrations::Illustration::Database,
                                cx,
                            )
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                        ),
                )
                .child(
                    v_flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_title())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t1(cx))
                                .child(if has_connections {
                                    "Choose a database object"
                                } else {
                                    "No database connections"
                                }),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .text_color(crate::ui::design::t3(cx))
                                .child(if has_connections {
                                    "Select a table, view, or collection from the Database sidebar."
                                } else {
                                    "Add a database connection to start exploring your data."
                                }),
                        ),
                )
                .when(!has_connections, |empty| {
                    empty.child(
                        style::secondary_button(
                            "database-empty-add-connection",
                            "Add DB connection",
                        )
                        .icon(IconName::Plus)
                        .on_click(move |_, window, cx| {
                            crate::ui::db::db_panel::DbConnectionsEditor::open(
                                workspace.clone(),
                                window,
                                cx,
                            );
                        }),
                    )
                })
                .into_any_element();
        }

        let active_key = self.active_db_key(project);
        let selected_ix = tabs
            .iter()
            .position(|(key, _)| Some(key) == active_key.as_ref())
            .unwrap_or(0);
        let view = active_key.as_ref().and_then(|key| {
            self.db_views
                .iter()
                .find(|v| v.project == project && &v.key == key)
                .map(|v| v.view.clone())
        });
        let keys: Vec<String> = tabs.iter().map(|(key, _)| key.clone()).collect();
        let close_key = active_key.clone();

        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .px_2()
                    .gap_1()
                    .items_center()
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        div().flex_1().min_w(px(0.)).child(
                            TabBar::new("db-tabs")
                                .menu(true)
                                .w_full()
                                .selected_index(selected_ix)
                                .on_click(cx.listener(move |this, ix: &usize, _, cx| {
                                    if let Some(key) = keys.get(*ix).cloned() {
                                        this.selected_db.insert(project, key);
                                        cx.notify();
                                    }
                                }))
                                .children(
                                    tabs.iter()
                                        .map(|(_, title)| Tab::new().label(title.clone())),
                                ),
                        ),
                    )
                    .when_some(close_key, |bar, key| {
                        bar.child(
                            Button::new("close-db-view")
                                .ghost()
                                .small()
                                .icon(IconName::Close)
                                .tooltip("Close collection")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.close_db_view(project, &key, cx);
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.))
                    .when_some(view, |area, view| area.child(view)),
            )
            .into_any_element()
    }
}
