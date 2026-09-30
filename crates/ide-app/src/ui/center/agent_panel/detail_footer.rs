use super::*;

impl CenterArea {
    pub(in crate::ui::center) fn start_agent_title_edit(
        &mut self,
        agent_id: Uuid,
        current_title: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Agent name")
                .default_value(current_title)
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.agent_title_edit = Some(AgentTitleEdit { agent_id, input });
        self.hovered_agent_title = None;
        cx.notify();
    }

    pub(in crate::ui::center) fn cancel_agent_title_edit(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        if self
            .agent_title_edit
            .as_ref()
            .is_some_and(|edit| edit.agent_id == agent_id)
        {
            self.agent_title_edit = None;
            cx.notify();
        }
    }

    pub(in crate::ui::center) fn save_agent_title_edit(
        &mut self,
        agent_id: Uuid,
        cx: &mut Context<Self>,
    ) {
        let Some(title) = self
            .agent_title_edit
            .as_ref()
            .filter(|edit| edit.agent_id == agent_id)
            .map(|edit| edit.input.read(cx).value().trim().to_string())
        else {
            return;
        };

        if !title.is_empty() {
            self.agents.update(cx, |agents, cx| {
                agents.update_title(agent_id, title.clone(), cx);
            });
            self.agent_chats.update(cx, |chats, cx| {
                chats.update_title(agent_id, title, cx);
            });
        }
        self.agent_title_edit = None;
        cx.notify();
    }

    pub(in crate::ui::center) fn render_agent_detail_switch_footer(
        &self,
        agent: &AgentRecord,
        detail_tab: AgentDetailTab,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let session = self.agent_chats.read(cx).session(agent.id).cloned();
        let tasks = session.as_ref().map(agent_footer_tasks).unwrap_or_default();
        let tasks_drawer = (tasks.len() > 1
            && self.agent_chat_footer_tasks_expanded.contains(&agent.id))
        .then(|| self.render_agent_footer_tasks_drawer(agent.id, &tasks, cx));

        v_flex()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .when_some(tasks_drawer, |footer, drawer| footer.child(drawer))
            .child(
                div()
                    .h(crate::ui::design::header_h())
                    .flex()
                    .items_center()
                    .px(crate::ui::design::agent_detail_bar_pad_x())
                    .child(self.render_agent_footer_tasks(agent, &tasks, cx))
                    .child(div().flex_1())
                    .child(self.render_agent_detail_switch(agent, detail_tab, cx)),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_agent_footer_tasks(
        &self,
        agent: &AgentRecord,
        tasks: &[AgentFooterTask],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        if agent.runtime != AgentRuntimeKind::Chat || tasks.is_empty() {
            return div().into_any_element();
        }
        let Some(current_index) = current_footer_task_index(tasks) else {
            return div().into_any_element();
        };
        let Some(current) = tasks.get(current_index) else {
            return div().into_any_element();
        };
        let done = tasks
            .iter()
            .filter(|task| task.status == WorkLogStatus::Completed)
            .count();
        let status = current.status;
        let expanded = self.agent_chat_footer_tasks_expanded.contains(&agent.id);

        h_flex()
            .min_w(px(0.))
            .max_w(px(760.))
            .gap_2()
            .items_center()
            .text_size(crate::ui::design::text_ui())
            .text_color(crate::ui::design::t3(cx))
            .child(self.render_agent_footer_task_icon(agent.id, current_index, status, cx))
            .child(
                div()
                    .flex_shrink_0()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(crate::ui::design::t1(cx).opacity(0.86))
                    .child("Current task:"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_color(if status == WorkLogStatus::Completed {
                        crate::ui::design::t3(cx).opacity(0.7)
                    } else {
                        crate::ui::design::t1(cx).opacity(0.9)
                    })
                    .child(current.text.clone()),
            )
            .when(!tasks.is_empty(), |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_color(crate::ui::design::t3(cx).opacity(0.78))
                        .child(format!("{done}/{}", tasks.len())),
                )
            })
            .when(tasks.len() > 1, |row| {
                row.child(
                    Button::new(("agent-footer-tasks-toggle", agent.id.as_u128() as u64))
                        .ghost()
                        .xsmall()
                        .compact()
                        .h(crate::ui::design::control_h_xs())
                        .icon(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronUp
                        })
                        .tooltip(if expanded { "Hide tasks" } else { "Show tasks" })
                        .on_click(cx.listener({
                            let agent_id = agent.id;
                            move |this, _, _, cx| {
                                if !this.agent_chat_footer_tasks_expanded.remove(&agent_id) {
                                    this.agent_chat_footer_tasks_expanded.insert(agent_id);
                                }
                                cx.notify();
                            }
                        })),
                )
            })
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_agent_footer_tasks_drawer(
        &self,
        agent_id: Uuid,
        tasks: &[AgentFooterTask],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let done = tasks
            .iter()
            .filter(|task| task.status == WorkLogStatus::Completed)
            .count();
        v_flex()
            .max_h(px(280.))
            .border_t_1()
            .border_b_1()
            .border_color(crate::ui::design::line(cx).opacity(0.24))
            .bg(crate::ui::design::base(cx))
            .overflow_hidden()
            .child(
                h_flex()
                    .h(crate::ui::design::subhead_h())
                    .px_4()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx).opacity(0.16))
                    .bg(crate::ui::design::nav(cx))
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t1(cx))
                            .child("Current tasks"),
                    )
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!("{done}/{}", tasks.len())),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new(("close-agent-footer-tasks", agent_id.as_u128() as u64))
                            .ghost()
                            .xsmall()
                            .compact()
                            .h(crate::ui::design::control_h_xs())
                            .icon(IconName::ChevronDown)
                            .tooltip("Hide tasks")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.agent_chat_footer_tasks_expanded.remove(&agent_id);
                                cx.notify();
                            })),
                    ),
            )
            .child(
                v_flex()
                    .max_h(px(246.))
                    .overflow_y_scrollbar()
                    .p_2()
                    .gap_1()
                    .children(tasks.iter().enumerate().map(|(index, task)| {
                        let text_color = match task.status {
                            WorkLogStatus::Completed => crate::ui::design::t3(cx).opacity(0.72),
                            WorkLogStatus::Failed => crate::ui::design::rose(cx),
                            WorkLogStatus::InProgress => crate::ui::design::t1(cx),
                            WorkLogStatus::Pending => crate::ui::design::t3(cx),
                        };
                        h_flex()
                            .gap_2()
                            .items_start()
                            .rounded(crate::ui::design::r_sm())
                            .px_2()
                            .py_1()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(text_color)
                            .when(task.status == WorkLogStatus::InProgress, |row| {
                                row.bg(crate::ui::design::accent(cx).opacity(0.08))
                            })
                            .when(task.status == WorkLogStatus::Failed, |row| {
                                row.bg(crate::ui::design::rose(cx).opacity(0.08))
                            })
                            .child(div().mt(px(1.)).child(self.render_agent_footer_task_icon(
                                agent_id,
                                index,
                                task.status,
                                cx,
                            )))
                            .child(div().flex_1().min_w(px(0.)).child(task.text.clone()))
                            .into_any_element()
                    })),
            )
            .into_any_element()
    }

    pub(in crate::ui::center) fn render_agent_footer_task_icon(
        &self,
        agent_id: Uuid,
        index: usize,
        status: WorkLogStatus,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        match status {
            WorkLogStatus::InProgress => logo_spinner(
                14.,
                "agent-footer-task-logo",
                (agent_id.as_u128() as usize).wrapping_add(index),
                crate::ui::design::sage(cx),
            )
            .into_any_element(),
            WorkLogStatus::Failed => gpui_component::Icon::new(IconName::TriangleAlert)
                .size(crate::ui::design::icon_md())
                .text_color(crate::ui::design::rose(cx))
                .into_any_element(),
            WorkLogStatus::Completed => gpui_component::Icon::new(IconName::Check)
                .size(crate::ui::design::icon_md())
                .text_color(crate::ui::design::sage(cx))
                .into_any_element(),
            WorkLogStatus::Pending => gpui_component::Icon::new(IconName::Dash)
                .size(crate::ui::design::icon_md())
                .text_color(crate::ui::design::t3(cx).opacity(0.62))
                .into_any_element(),
        }
    }
}
