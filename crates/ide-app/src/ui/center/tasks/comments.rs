use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn ensure_task_comment_input(
        &mut self,
        task_element_id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = self.task_comment_inputs.get(&task_element_id) {
            return input.clone();
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(2, 6)
                .placeholder("Add a comment…")
        });
        self.task_comment_inputs
            .insert(task_element_id, input.clone());
        input
    }

    pub(in crate::ui::center) fn render_task_comments(
        &self,
        project: ProjectId,
        reference: &TaskRef,
        task_element_id: u64,
        comment_input: Entity<InputState>,
        detail: Option<&TaskDetail>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let header = |count: Option<usize>, cx: &mut Context<Self>| {
            h_flex()
                .items_center()
                .gap_2()
                .child(section_title("Comments", cx))
                .when_some(count.filter(|count| *count > 0), |header, count| {
                    header.child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .bg(crate::ui::design::surface(cx))
                            .rounded_full()
                            .px_1p5()
                            .child(count.to_string()),
                    )
                })
        };

        let (count, body): (Option<usize>, gpui::AnyElement) = match detail {
            None => (
                None,
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child("Load issue detail to view comments.")
                    .into_any_element(),
            ),
            Some(detail) if detail.comments.is_empty() => (
                Some(0),
                div()
                    .text_size(crate::ui::design::text_body())
                    .text_color(crate::ui::design::t3(cx))
                    .child("No comments yet.")
                    .into_any_element(),
            ),
            Some(detail) => (
                Some(detail.comments.len()),
                self.render_task_comment_list(detail, cx),
            ),
        };

        let composer = detail.is_some().then(|| {
            self.render_task_comment_composer(
                project,
                reference,
                task_element_id,
                comment_input,
                cx,
            )
        });

        v_flex()
            .gap_2()
            .child(header(count, cx))
            .child(body)
            .when_some(composer, |section, composer| section.child(composer))
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_task_comment_composer(
        &self,
        project: ProjectId,
        reference: &TaskRef,
        task_element_id: u64,
        comment_input: Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let posting = self.task_comment_posting.contains(&task_element_id);
        let error = self.task_comment_error.get(&task_element_id).cloned();
        let owned_reference = reference.clone();
        v_flex()
            .w_full()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(crate::ui::design::line(cx).opacity(0.4))
            .child(Input::new(&comment_input))
            .when_some(error, |section, error| {
                section.child(
                    div()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::rose(cx))
                        .child(SharedString::from(error)),
                )
            })
            .child(
                h_flex().items_center().gap_2().child(div().flex_1()).child(
                    style::primary_button_compact(
                        ("task-comment-post", task_element_id),
                        if posting { "Posting…" } else { "Comment" },
                        cx,
                    )
                    .disabled(posting)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.post_task_comment(
                            project,
                            owned_reference.clone(),
                            task_element_id,
                            cx,
                        );
                    })),
                ),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn post_task_comment(
        &mut self,
        project: ProjectId,
        reference: TaskRef,
        task_element_id: u64,
        cx: &mut Context<Self>,
    ) {
        if self.task_comment_posting.contains(&task_element_id) {
            return;
        }
        let body = self
            .task_comment_inputs
            .get(&task_element_id)
            .map(|input| input.read(cx).value().trim().to_string())
            .unwrap_or_default();
        if body.is_empty() {
            return;
        }
        self.task_comment_posting.insert(task_element_id);
        self.task_comment_error.remove(&task_element_id);
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
                    let body = body.clone();
                    async move {
                        let connection = connection
                            .ok_or_else(|| anyhow::anyhow!("no connection found for this task"))?;
                        ide_core::TaskTrackerClient::new(connection)?.add_comment(&reference, &body)
                    }
                })
                .await;
            this.update(cx, |this, cx| {
                this.task_comment_posting.remove(&task_element_id);
                match result {
                    Ok(_) => {
                        // Drop the composer so it re-creates empty next render, then
                        // re-fetch so the new comment shows in the IDE too.
                        this.task_comment_inputs.remove(&task_element_id);
                        this.tasks.update(cx, |tasks, cx| {
                            tasks.refresh_detail(project, reference.clone(), cx);
                        });
                    }
                    Err(error) => {
                        let message = error
                            .to_string()
                            .lines()
                            .next()
                            .unwrap_or("Failed to post comment")
                            .trim()
                            .to_string();
                        this.task_comment_error.insert(task_element_id, message);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub(in crate::ui::center) fn render_task_comment_list(
        &self,
        detail: &TaskDetail,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .children(detail.comments.iter().enumerate().map(|(index, comment)| {
                h_flex()
                    .id(("task-comment", index))
                    .w_full()
                    .gap_3()
                    .items_start()
                    .py_2p5()
                    .when(index > 0, |row| {
                        row.border_t_1()
                            .border_color(crate::ui::design::line(cx).opacity(0.4))
                    })
                    .child(avatar_badge(&comment.author))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w(px(0.))
                            .gap_0p5()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(crate::ui::design::text_ui())
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(style::focus_text(cx))
                                            .child(SharedString::from(comment.author.clone())),
                                    )
                                    .when_some(comment.created.clone(), |row, created| {
                                        let label = format_timestamp(&created);
                                        row.when(!label.is_empty(), |row| {
                                            row.child(
                                                div()
                                                    .text_size(crate::ui::design::text_ui())
                                                    .text_color(
                                                        crate::ui::design::t3(cx).opacity(0.7),
                                                    )
                                                    .child(SharedString::from(label)),
                                            )
                                        })
                                    }),
                            )
                            .child(render_rich_text_block(
                                Some(&comment.body),
                                "No comment.",
                                "task-comment-body",
                                cx,
                            )),
                    )
                    .into_any_element()
            }))
            .into_any_element()
    }
}
