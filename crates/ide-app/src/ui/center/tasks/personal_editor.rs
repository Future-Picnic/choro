use super::*;

impl CenterArea {
    /// Return the inline live-doc editor for a personal task, creating it (and
    /// its autosave subscription) if the selection changed. Flushes the previous
    /// task's edits before swapping.
    pub(in crate::ui::center) fn ensure_personal_editor(
        &mut self,
        task: ide_core::PersonalTaskRecord,
        cx: &mut Context<Self>,
    ) -> Entity<velotype::Editor> {
        if let Some(state) = self.personal_editor.as_ref() {
            if state.task.id == task.id {
                return state.editor.clone();
            }
        }
        sync_personal_task_editor_theme(cx);
        self.flush_personal_editor(cx);

        // Give the editor a real, writable path so pasted images land in the
        // app's data dir (velotype resolves the image dir from the file's
        // parent; with no path it falls back to the process cwd = "/").
        let doc_path = ide_core::local_store::LocalStore::open_default()
            .ok()
            .map(|store| {
                store
                    .app_data_dir()
                    .join("projects")
                    .join(task.project_id.0.to_string())
                    .join("personal-tasks")
                    .join(format!("{}.md", task.id))
            });

        let markdown = task.description_markdown.clone();
        let editor = cx.new(|cx| {
            let mut editor = velotype::Editor::from_markdown(cx, markdown.clone(), doc_path);
            editor.embedded_set_chrome_visible(false, cx);
            editor.embedded_set_host_window_integration(false, cx);
            editor.embedded_set_nested_scroll(true, cx);
            editor
        });
        let subscription = cx.subscribe(
            &editor,
            |this: &mut Self, editor, event: &velotype::EditorEvent, cx| {
                if matches!(event, velotype::EditorEvent::ContentChanged) {
                    this.schedule_personal_autosave(editor, cx);
                }
            },
        );
        let handle = editor.clone();
        self.personal_editor = Some(PersonalEditorState {
            task,
            editor,
            last_saved: markdown,
            _subscription: subscription,
        });
        handle
    }

    /// Coalesce rapid edits into a single write after a short pause, so we don't
    /// spin up a Turso runtime + write on every keystroke (which starved reads
    /// and surfaced "database is locked").
    pub(in crate::ui::center) fn schedule_personal_autosave(
        &mut self,
        editor: Entity<velotype::Editor>,
        cx: &mut Context<Self>,
    ) {
        self.personal_editor_save_epoch = self.personal_editor_save_epoch.wrapping_add(1);
        let epoch = self.personal_editor_save_epoch;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(600))
                .await;
            this.update(cx, |this, cx| {
                if this.personal_editor_save_epoch == epoch {
                    this.commit_personal_autosave(editor, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    pub(in crate::ui::center) fn commit_personal_autosave(
        &mut self,
        editor: Entity<velotype::Editor>,
        cx: &mut Context<Self>,
    ) {
        let markdown = editor.read(cx).embedded_markdown(cx);
        let (project, task_id) = {
            let Some(state) = self.personal_editor.as_mut() else {
                return;
            };
            if state.last_saved == markdown {
                return;
            }
            state.last_saved = markdown.clone();
            state.task.description_markdown = markdown.clone();
            (state.task.project_id, state.task.id)
        };
        self.tasks.update(cx, |tasks, cx| {
            tasks.autosave_personal_description(project, task_id, markdown, cx)
        });
        editor.update(cx, |editor, cx| editor.embedded_mark_clean(cx));
    }

    pub(in crate::ui::center) fn flush_personal_editor(&mut self, cx: &mut Context<Self>) {
        // Immediate save (no debounce) when switching away from a task.
        if let Some(editor) = self
            .personal_editor
            .as_ref()
            .map(|state| state.editor.clone())
        {
            self.commit_personal_autosave(editor, cx);
        }
    }

    /// Personal-task priority is stored locally and can be changed directly
    /// from the header's metadata line. External tracker priorities remain
    /// read-only. The caller supplies the colour-carrying flag glyph, so this
    /// label stays neutral — state colour never lands on both.
    pub(in crate::ui::center) fn render_personal_task_priority_control(
        &self,
        task: ide_core::PersonalTaskRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let current = task.priority;
        let project = task.project_id;
        let task_id = task.id;
        let task_element_id = task.id.as_u128() as u64;
        let view = cx.entity().clone();

        style::header_dropdown_button(("personal-task-priority", task_element_id), cx)
            .p_0()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .label(current.label())
            .dropdown_menu(move |mut menu, window, _| {
                for priority in ide_core::PersonalTaskPriority::ALL {
                    menu = menu.item(
                        PopupMenuItem::element(move |_, cx| {
                            personal_priority_menu_row(priority, cx)
                        })
                        .checked(priority == current)
                        .on_click(window.listener_for(
                            &view,
                            move |this: &mut Self, _, _, cx| {
                                if priority == current {
                                    return;
                                }
                                if let Some(editor) = this.personal_editor.as_mut() {
                                    if editor.task.id == task_id {
                                        editor.task.priority = priority;
                                    }
                                }
                                this.tasks.update(cx, |tasks, cx| {
                                    tasks
                                        .set_personal_task_priority(project, task_id, priority, cx);
                                });
                            },
                        )),
                    );
                }
                menu
            })
            .into_any_element()
    }

    /// The status control in the task header. Personal tasks get an editable
    /// dropdown (like the agent/doc status); external tasks are read-only, so a
    /// static pill.
    pub(in crate::ui::center) fn render_task_status_control(
        &self,
        project: ProjectId,
        reference: &TaskRef,
        is_personal: bool,
        personal_task: &Option<ide_core::PersonalTaskRecord>,
        status_label: String,
        status_color: gpui::Hsla,
        task_element_id: u64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if is_personal {
            let status_view = cx.entity().clone();
            let record = personal_task.clone();
            let current = record.as_ref().map(|task| task.status);
            style::header_dropdown_button(("task-status", task_element_id), cx)
                .child(crate::ui::design::indicator::status(
                    SharedString::from(status_label),
                    status_color,
                    cx,
                ))
                .dropdown_menu(move |mut menu, window, _| {
                    for status in ide_core::PersonalTaskStatus::ALL {
                        let record = record.clone();
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                personal_status_menu_row(status, cx)
                            })
                            .checked(current == Some(status))
                            .on_click(window.listener_for(
                                &status_view,
                                move |this: &mut Self, _, _, cx| {
                                    if let Some(mut task) = record.clone() {
                                        task.status = status;
                                        this.tasks.update(cx, |tasks, cx| {
                                            tasks.save_personal_task(task.clone(), cx);
                                        });
                                    }
                                },
                            )),
                        );
                    }
                    menu
                })
                .into_any_element()
        } else {
            // External trackers: a live dropdown of the board's statuses that
            // pushes the change through the provider API — so you can mark a task
            // done in the IDE instead of opening the real tracker.
            let options: Vec<String> = self
                .tasks
                .read(cx)
                .board(project)
                .filter(|board| board.provider == reference.provider)
                .map(|board| {
                    board
                        .columns
                        .iter()
                        .map(|column| column.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let updating = self.task_status_updating.contains(&task_element_id);
            let error = self.task_status_error.get(&task_element_id).cloned();

            if options.is_empty() {
                return task_status_static_badge(&status_label, status_color, error, cx);
            }

            let view = cx.entity().clone();
            let owned_reference = reference.clone();
            let current = status_label.clone();
            let button = style::header_dropdown_button(("task-status", task_element_id), cx)
                .disabled(updating)
                .child(crate::ui::design::indicator::status(
                    SharedString::from(if updating {
                        format!("{current} …")
                    } else {
                        current.clone()
                    }),
                    status_color,
                    cx,
                ))
                .dropdown_menu(move |mut menu, window, _| {
                    for name in options.clone() {
                        let is_current = name.eq_ignore_ascii_case(current.trim());
                        let row_name = name.clone();
                        let click_name = name.clone();
                        let reference = owned_reference.clone();
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                task_status_menu_row(&row_name, is_current, cx)
                            })
                            .checked(is_current)
                            .on_click(window.listener_for(
                                &view,
                                move |this: &mut Self, _, _, cx| {
                                    if is_current {
                                        return;
                                    }
                                    this.apply_external_task_status(
                                        project,
                                        reference.clone(),
                                        click_name.clone(),
                                        task_element_id,
                                        cx,
                                    );
                                },
                            )),
                        );
                    }
                    menu
                });

            match error {
                Some(error) => v_flex()
                    .gap_1()
                    .child(button)
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::rose(cx))
                            .child(SharedString::from(error)),
                    )
                    .into_any_element(),
                None => button.into_any_element(),
            }
        }
    }

    pub(in crate::ui::center) fn apply_external_task_status(
        &mut self,
        project: ProjectId,
        reference: TaskRef,
        status_name: String,
        task_element_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.task_status_updating.contains(&task_element_id) {
            return;
        }
        self.task_status_updating.insert(task_element_id);
        self.task_status_error.remove(&task_element_id);
        cx.notify();

        let connection = self
            .tasks
            .read(cx)
            .connection_object_for_ref(project, &reference, cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn({
                    let reference = reference.clone();
                    let status_name = status_name.clone();
                    async move {
                        let connection = connection
                            .ok_or_else(|| anyhow::anyhow!("no connection found for this task"))?;
                        let client = ide_core::TaskTrackerClient::new(connection)?;
                        client.set_status_by_name(&reference, &status_name)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.task_status_updating.remove(&task_element_id);
                match result {
                    Ok(_) => {
                        this.tasks.update(cx, |tasks, cx| {
                            tasks.refresh_project(project, cx);
                            tasks.refresh_detail(project, reference.clone(), cx);
                        });
                    }
                    Err(error) => {
                        let message = error
                            .to_string()
                            .lines()
                            .next()
                            .unwrap_or("Failed to update status")
                            .trim()
                            .to_string();
                        this.task_status_error.insert(task_element_id, message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}
