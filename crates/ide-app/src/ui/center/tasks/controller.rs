use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn refresh_task_board(
        &mut self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) {
        let has_board = self
            .tasks
            .read(cx)
            .configured_connection(project, cx)
            .is_some_and(|connection| {
                connection.provider == ide_core::IssueTrackerProvider::Personal
                    || connection.has_selected_source()
            });
        if !has_board {
            return;
        }

        self.tasks.update(cx, |tasks, cx| {
            let selected = tasks.selected_ref(project);
            tasks.refresh_project(project, cx);
            if let Some(reference) = selected {
                tasks.refresh_detail(project, reference, cx);
            }
        });
    }

    pub(in crate::ui::center) fn refresh_active_task_board(&mut self, cx: &mut Context<Self>) {
        let Some(project) = self
            .workspace
            .read(cx)
            .active_project()
            .map(|project| project.id)
        else {
            return;
        };
        self.refresh_task_board(project, cx);
    }

    pub(in crate::ui::center) fn start_tasks_auto_refresh(&mut self, cx: &mut Context<Self>) {
        self.tasks_refresh_epoch = self.tasks_refresh_epoch.wrapping_add(1);
        let epoch = self.tasks_refresh_epoch;
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(TASK_BOARD_REFRESH_INTERVAL)
                .await;
            let Some(center) = this.upgrade() else {
                break;
            };
            let keep_running = center
                .update(cx, |center, cx| {
                    if center.tasks_refresh_epoch != epoch || center.view_mode != CenterMode::Tasks
                    {
                        return false;
                    }
                    center.refresh_active_task_board(cx);
                    true
                })
                .ok()
                .unwrap_or(false);
            if !keep_running {
                break;
            }
        })
        .detach();
    }

    pub(in crate::ui::center) fn open_task(
        &mut self,
        project: ProjectId,
        task: TaskRef,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        let detail_task = task.clone();
        self.tasks.update(cx, |tasks, cx| {
            tasks.select_task(project, task, cx);
            tasks.refresh_project(project, cx);
            tasks.ensure_detail(project, detail_task, cx);
        });
        self.tasks_detail_collapsed = false;
        self.stash_new_agent_composer();
        self.set_view_mode(CenterMode::Tasks, cx);
        self.start_tasks_auto_refresh(cx);
        cx.notify();
    }

    /// Open a task from the cross-project "My Tasks" sidebar: navigate to its
    /// project and show its detail, but stay in the My Tasks view so the
    /// aggregated list on the right persists.
    pub(crate) fn open_my_task(
        &mut self,
        project: ProjectId,
        task: TaskRef,
        cx: &mut Context<Self>,
    ) {
        self.workspace
            .update(cx, |workspace, cx| workspace.set_active(project, cx));
        let detail_task = task.clone();
        self.tasks.update(cx, |tasks, cx| {
            if let Some(connection_id) = tasks.connection_for_ref(project, &task, cx) {
                tasks.select_connection(project, connection_id, cx);
            }
            tasks.select_task(project, task, cx);
            tasks.refresh_project(project, cx);
            tasks.ensure_detail(project, detail_task, cx);
        });
        self.tasks_detail_collapsed = false;
        self.stash_new_agent_composer();
        self.set_view_mode(CenterMode::MyTasks, cx);
        self.start_tasks_auto_refresh(cx);
        cx.notify();
    }

    pub(in crate::ui::center) fn open_implementation_agent_for_task(
        &mut self,
        project: ProjectId,
        summary: TaskSummary,
        detail: Option<TaskDetail>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let reference = summary.reference.clone();
        let prompt = task_implementation_prompt(&summary, detail.as_ref());

        self.open_new_agent_composer_for_project(project, window, cx);

        let Some(composer) = self.new_agent_composer.as_mut() else {
            return;
        };
        composer.reset_source_implementation();
        composer.prompt.update(cx, |input, cx| {
            input.set_value(prompt.clone(), window, cx);
            input.set_cursor_position(
                input_position_for_byte_offset(&prompt, prompt.len()),
                window,
                cx,
            );
            input.focus(window, cx);
        });
        composer.linked_tasks.clear();
        composer.linked_tasks.push(reference.clone());
        composer.suggested_title = Some(super::agent_naming::implementation_agent_title(
            &summary.reference.title,
            "task",
        ));
        composer.source_task = Some(reference);
        composer.error = None;
        composer.doc_mention_selected = 0;
        composer.doc_mention_dismissed_query = None;
        composer.file_mention_selected = 0;
        composer.file_mention_dismissed_query = None;
        self.attach_studio_source_designs(project, None, Some(&summary.reference), cx);
        crate::ui::onboarding::emit_for_project(
            project,
            crate::ui::onboarding::OnboardingEvent::TaskImplementOpened,
            cx,
        );
        cx.notify();
    }

    pub(crate) fn open_personal_task_editor(
        &mut self,
        project: ProjectId,
        task: Option<ide_core::PersonalTaskRecord>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = PersonalTaskEditorDialog::new(task, window, cx);
        let title = if editor.read(cx).task.is_some() {
            "Edit personal task"
        } else {
            "New personal task"
        };
        let tasks_entity = self.tasks.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let editor_for_child = editor.clone();
            let editor_for_save = editor.clone();
            let tasks_entity = tasks_entity.clone();
            dialog
                .title(SharedString::from(title))
                .w(px(560.))
                .child(editor_for_child)
                .footer(move |_, _, _, cx| {
                    let editor = editor_for_save.clone();
                    let tasks_entity = tasks_entity.clone();
                    vec![
                        crate::ui::style::dialog_neutral_button(
                            "cancel-personal-task",
                            "Cancel",
                            cx,
                        )
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                        crate::ui::style::primary_button_compact("save-personal-task", "Save", cx)
                            .on_click(move |_, window, cx| {
                                let (task, name, description) = editor.update(cx, |editor, cx| {
                                    (
                                        editor.task.clone(),
                                        editor.name.read(cx).value().trim().to_string(),
                                        editor.description.read(cx).value().to_string(),
                                    )
                                });
                                if let Some(mut task) = task {
                                    task.title = if name.is_empty() {
                                        task.issue_key()
                                    } else {
                                        name
                                    };
                                    task.description_markdown = description;
                                    tasks_entity.update(cx, |tasks, cx| {
                                        tasks.save_personal_task(task, cx);
                                    });
                                } else {
                                    tasks_entity.update(cx, |tasks, cx| {
                                        tasks.create_personal_task(project, name, description, cx);
                                    });
                                }
                                window.close_dialog(cx);
                            }),
                    ]
                })
        });
    }
}
