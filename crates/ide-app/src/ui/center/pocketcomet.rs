use super::*;

const POCKETCOMET_PROJECT_LIST_W: f32 = 220.0;

fn clipped_text(value: &str, limit: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= limit {
        return compact;
    }
    format!(
        "{}…",
        compact
            .chars()
            .take(limit.saturating_sub(1))
            .collect::<String>()
    )
}

fn pocketcomet_runtime_label(runtime: AgentRuntime) -> &'static str {
    match runtime {
        AgentRuntime::Working => "Working",
        AgentRuntime::Waiting => "Needs attention",
        AgentRuntime::Open => "Open",
        AgentRuntime::Idle => "Idle",
        AgentRuntime::Ended => "Ended",
        AgentRuntime::NotStarted => "Ready",
    }
}

fn pocketcomet_runtime_tone(runtime: AgentRuntime, cx: &App) -> gpui::Hsla {
    match runtime {
        AgentRuntime::Working | AgentRuntime::Open => crate::ui::design::sky(cx),
        AgentRuntime::Waiting => crate::ui::design::amber(cx),
        _ => crate::ui::design::t3(cx),
    }
}

impl CenterArea {
    pub(super) fn render_pocketcomet_section(
        &mut self,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let mut records = self
            .agents
            .read(cx)
            .all_records()
            .into_iter()
            .filter(|agent| {
                agent
                    .origin
                    .as_ref()
                    .is_some_and(AgentOrigin::is_pocketcomet)
            })
            .collect::<Vec<_>>();
        records.sort_by_key(|agent| std::cmp::Reverse(agent.updated_at));

        if let Some(selected_id) = self.pocketcomet_selected_chat {
            if let Some(chat) = records.iter().find(|agent| {
                agent.id == selected_id
                    && agent
                        .origin
                        .as_ref()
                        .is_some_and(AgentOrigin::is_pocketcomet_chat)
            }) {
                return self.render_pocketcomet_chat_detail(chat, cx);
            }
            self.pocketcomet_selected_chat = None;
        }

        let projects = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .filter_map(|project| {
                let count = records
                    .iter()
                    .filter(|agent| agent.project_id == project.id)
                    .count();
                (count > 0).then(|| (project.id, project.name.clone(), count))
            })
            .collect::<Vec<_>>();
        if self
            .pocketcomet_project_filter
            .is_some_and(|project| !projects.iter().any(|(id, _, _)| *id == project))
        {
            self.pocketcomet_project_filter = None;
        }
        let filter = self.pocketcomet_project_filter;
        let filtered = records
            .iter()
            .filter(|agent| filter.is_none_or(|project| agent.project_id == project))
            .cloned()
            .collect::<Vec<_>>();
        let subtitle = match filter.and_then(|selected| {
            projects
                .iter()
                .find(|(project, _, _)| *project == selected)
                .map(|(_, name, _)| name.clone())
        }) {
            Some(project) => format!("PocketComet activity mapped to {project}"),
            None => "Activity from every mapped PocketComet project".to_string(),
        };
        // Keep the page title, project list, and activity pane in one frame,
        // as in Ask History. Centering only the activity left the sidebar
        // stranded at the window edge on wide displays.
        let frame_max_w =
            px(POCKETCOMET_PROJECT_LIST_W) + crate::ui::design::center_content_frame_max_w();
        let header = crate::ui::design::header::bar(cx).max_w(frame_max_w).child(
            crate::ui::design::header::title_col(cx)
                .child(crate::ui::design::header::title("PocketComet", cx))
                .child(crate::ui::design::header::subtitle(subtitle, cx)),
        );

        if records.is_empty() {
            return v_flex()
                .size_full()
                .bg(crate::ui::design::base(cx))
                .child(header)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .px_6()
                        .gap_3()
                        .child(
                            div()
                                .size(px(48.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(crate::ui::design::r_lg())
                                .bg(crate::ui::design::accent_soft(cx))
                                .child(crate::ui::design::indicator::lucide_icon(
                                    lucide_icons::Icon::Orbit,
                                    crate::ui::design::accent(cx),
                                    crate::ui::design::icon_xl(),
                                )),
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
                                        .child("No PocketComet activity yet"),
                                )
                                .child(
                                    div()
                                        .max_w(px(480.))
                                        .text_center()
                                        .text_size(crate::ui::design::text_body())
                                        .line_height(gpui::relative(1.5))
                                        .text_color(crate::ui::design::t3(cx))
                                        .child("Tag Choro in a mapped PocketComet chat, or send a task to Choro, and it will appear here."),
                                ),
                        ),
                )
                .into_any_element();
        }

        let mut waiting = Vec::new();
        let mut active = Vec::new();
        let mut recent = Vec::new();
        let mut chats = Vec::new();
        for agent in filtered {
            if agent
                .origin
                .as_ref()
                .is_some_and(AgentOrigin::is_pocketcomet_chat)
            {
                chats.push(agent);
                continue;
            }
            match self.agent_runtime(&agent, agent.project_id, cx) {
                AgentRuntime::Waiting => waiting.push(agent),
                AgentRuntime::Working | AgentRuntime::Open => active.push(agent),
                _ => recent.push(agent),
            }
        }

        let content = v_flex()
            .w_full()
            .min_w(px(0.))
            .px_4()
            .py(crate::ui::design::center_column_pad_y())
            .gap_6()
            .children(
                (!waiting.is_empty())
                    .then(|| self.render_pocketcomet_agent_group("Needs attention", &waiting, cx)),
            )
            .children(
                (!active.is_empty())
                    .then(|| self.render_pocketcomet_agent_group("Active now", &active, cx)),
            )
            .children(
                (!recent.is_empty())
                    .then(|| self.render_pocketcomet_agent_group("Recent work", &recent, cx)),
            )
            .child(self.render_pocketcomet_chat_group(&chats, cx));

        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(crate::ui::design::base(cx))
            .child(header)
            .child(div().w_full().h(px(1.)).bg(crate::ui::design::line(cx)))
            .child(
                div().flex_1().min_h(px(0.)).w_full().p_4().child(
                    h_flex()
                        .size_full()
                        .min_w(px(0.))
                        .max_w(frame_max_w)
                        .mx_auto()
                        .items_start()
                        .rounded(crate::ui::design::r_lg())
                        .border_1()
                        .border_color(crate::ui::design::line(cx))
                        .overflow_hidden()
                        .child(self.render_pocketcomet_project_filter(&projects, records.len(), cx))
                        .child(
                            v_flex()
                                .id("pocketcomet-home-scroll")
                                .flex_1()
                                .min_w(px(0.))
                                .h_full()
                                .overflow_y_scrollbar()
                                .child(content),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_pocketcomet_project_filter(
        &self,
        projects: &[(ProjectId, String, usize)],
        total: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.pocketcomet_project_filter;
        let all_row = self.pocketcomet_project_filter_row(
            "pocketcomet-project-all",
            "All projects".into(),
            total,
            selected.is_none(),
            None,
            cx,
        );
        v_flex()
            .id("pocketcomet-project-list")
            .w(px(POCKETCOMET_PROJECT_LIST_W))
            .h_full()
            .min_h(px(0.))
            .flex_none()
            .overflow_y_scrollbar()
            .bg(crate::ui::design::nav(cx))
            .px_2()
            .py_4()
            .gap_1()
            .border_r_1()
            .border_color(crate::ui::design::line(cx))
            .child(
                div()
                    .px_2()
                    .pb_2()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t2(cx))
                    .child("Projects"),
            )
            .child(all_row)
            .children(projects.iter().map(|(project, name, count)| {
                self.pocketcomet_project_filter_row(
                    ("pocketcomet-project", project.0.as_u128() as u64),
                    name.clone().into(),
                    *count,
                    selected == Some(*project),
                    Some(*project),
                    cx,
                )
            }))
            .into_any_element()
    }

    fn pocketcomet_project_filter_row(
        &self,
        id: impl Into<gpui::ElementId>,
        label: SharedString,
        count: usize,
        selected: bool,
        project: Option<ProjectId>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        style::master_list_row(id, selected, cx)
            .min_h(px(34.))
            .px_2()
            .py_1p5()
            .gap_2()
            .items_center()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pocketcomet_project_filter = project;
                this.pocketcomet_selected_chat = None;
                cx.notify();
            }))
            .child(
                Icon::new(if project.is_some() {
                    IconName::Folder
                } else {
                    IconName::LayoutDashboard
                })
                .size(crate::ui::design::icon_sm())
                .text_color(if selected {
                    crate::ui::design::accent(cx)
                } else {
                    crate::ui::design::t3(cx)
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(if selected {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .text_color(if selected {
                        crate::ui::design::t1(cx)
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .child(label),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(crate::ui::design::text_label())
                    .text_color(crate::ui::design::t3(cx))
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn render_pocketcomet_agent_group(
        &self,
        title: &'static str,
        agents: &[AgentRecord],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .child(self.pocketcomet_section_heading(title, agents.len(), cx))
            .child(
                v_flex().w_full().gap_0p5().children(
                    agents
                        .iter()
                        .map(|agent| self.render_pocketcomet_agent_row(agent, cx)),
                ),
            )
            .into_any_element()
    }

    fn render_pocketcomet_agent_row(
        &self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let project_id = agent.project_id;
        let runtime = self.agent_runtime(agent, project_id, cx);
        let status = pocketcomet_runtime_label(runtime);
        let tone = pocketcomet_runtime_tone(runtime, cx);
        let project_name = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.name.clone())
            .unwrap_or_else(|| "Project".into());
        let task_title = match agent.origin.as_ref() {
            Some(AgentOrigin::PocketComet { task_title, .. }) => task_title.clone(),
            _ => agent.title.clone(),
        };
        let outcome = self
            .agent_summaries
            .get(&agent.id)
            .and_then(|summary| {
                summary
                    .outcome_text
                    .as_deref()
                    .or(Some(summary.summary_text.as_str()))
            })
            .map(|text| clipped_text(text, 220));
        let workspace = self.workspace.clone();
        style::master_list_row(("pocketcomet-agent", agent.id.as_u128() as u64), false, cx)
            .px_3()
            .py_2()
            .gap_3()
            .items_start()
            .on_click(cx.listener(move |this, _, window, cx| {
                workspace.update(cx, |workspace, cx| workspace.set_active(project_id, cx));
                this.open_agent(agent_id, window, cx);
            }))
            .child(
                crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::ListTodo,
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_md(),
                )
                .mt(px(3.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(task_title),
                            )
                            .child(
                                crate::ui::design::indicator::status(status, tone, cx).flex_none(),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(format!(
                                "{project_name} · {}",
                                branch_relative_time(agent.updated_at as i64)
                            )),
                    )
                    .children(outcome.map(|outcome| {
                        div()
                            .w_full()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .truncate()
                            .child(outcome)
                    })),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .mt(px(4.))
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .into_any_element()
    }

    fn render_pocketcomet_chat_group(
        &self,
        chats: &[AgentRecord],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .gap_2()
            .child(self.pocketcomet_section_heading("Chats", chats.len(), cx))
            .when(chats.is_empty(), |section| {
                section.child(
                    div()
                        .px_3()
                        .py_4()
                        .text_size(crate::ui::design::text_body())
                        .text_color(crate::ui::design::t3(cx))
                        .child("No Choro conversations in this project yet."),
                )
            })
            .when(!chats.is_empty(), |section| {
                section.child(
                    v_flex().w_full().gap_0p5().children(
                        chats
                            .iter()
                            .map(|chat| self.render_pocketcomet_chat_row(chat, cx)),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_pocketcomet_chat_row(
        &self,
        chat: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let chat_id = chat.id;
        let runtime = self.agent_runtime(chat, chat.project_id, cx);
        let tone = pocketcomet_runtime_tone(runtime, cx);
        let (title, meta) = match chat.origin.as_ref() {
            Some(AgentOrigin::PocketCometChat {
                project_name,
                teammate_name,
                conversation_name,
                thread_title,
                ..
            }) => {
                let teammate = (teammate_name != "Choro")
                    .then(|| format!(" · {teammate_name}"))
                    .unwrap_or_default();
                (
                    thread_title.clone(),
                    format!(
                        "{project_name} · {conversation_name}{teammate} · {}",
                        branch_relative_time(chat.updated_at as i64)
                    ),
                )
            }
            _ => (
                chat.title.clone(),
                branch_relative_time(chat.updated_at as i64),
            ),
        };
        let latest = self.pocketcomet_chat_responses(chat, cx).pop();
        style::master_list_row(("pocketcomet-chat", chat.id.as_u128() as u64), false, cx)
            .px_3()
            .py_2()
            .gap_3()
            .items_start()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.pocketcomet_selected_chat = Some(chat_id);
                cx.notify();
            }))
            .child(
                crate::ui::design::indicator::lucide_icon(
                    lucide_icons::Icon::MessageCircleMore,
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_md(),
                )
                .mt(px(3.)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .gap_1()
                    .child(
                        h_flex()
                            .w_full()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(crate::ui::design::text_body())
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t1(cx))
                                    .child(title),
                            )
                            .child(
                                crate::ui::design::indicator::status(
                                    pocketcomet_runtime_label(runtime),
                                    tone,
                                    cx,
                                )
                                .flex_none(),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t3(cx))
                            .child(meta),
                    )
                    .children(latest.map(|(text, _)| {
                        div()
                            .w_full()
                            .text_size(crate::ui::design::text_ui())
                            .text_color(crate::ui::design::t2(cx))
                            .truncate()
                            .child(clipped_text(&text, 220))
                    })),
            )
            .child(
                Icon::new(IconName::ChevronRight)
                    .mt(px(4.))
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .into_any_element()
    }

    fn pocketcomet_section_heading(
        &self,
        title: &'static str,
        count: usize,
        cx: &App,
    ) -> gpui::AnyElement {
        h_flex()
            .w_full()
            .px_3()
            .items_center()
            .gap_2()
            .child(
                div()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child(title),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .child(count.to_string()),
            )
            .into_any_element()
    }

    fn pocketcomet_chat_responses(&self, chat: &AgentRecord, cx: &App) -> Vec<(String, u64)> {
        let timeline = self
            .agent_chats
            .read(cx)
            .session(chat.id)
            .map(|session| session.timeline.clone())
            .or_else(|| Self::load_chat_session_hydration(chat).map(|value| value.timeline))
            .unwrap_or_default();
        timeline
            .into_iter()
            .filter_map(|item| match item {
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    text,
                    created_at,
                    ..
                }) if !text.trim().is_empty() => Some((text, created_at)),
                _ => None,
            })
            .collect()
    }

    fn render_pocketcomet_chat_detail(
        &self,
        chat: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let (thread_title, project_name, conversation_name, teammate_name) =
            match chat.origin.as_ref() {
                Some(AgentOrigin::PocketCometChat {
                    thread_title,
                    project_name,
                    conversation_name,
                    teammate_name,
                    ..
                }) => (
                    thread_title.clone(),
                    project_name.clone(),
                    conversation_name.clone(),
                    teammate_name.clone(),
                ),
                _ => (
                    chat.title.clone(),
                    "PocketComet".into(),
                    "Chat".into(),
                    "Choro".into(),
                ),
            };
        let teammate = (teammate_name != "Choro")
            .then(|| format!(" · {teammate_name}"))
            .unwrap_or_default();
        let responses = self.pocketcomet_chat_responses(chat, cx);
        let runtime = self.agent_runtime(chat, chat.project_id, cx);
        let session = self.agent_chats.read(cx).session(chat.id);
        let (status, detail) = if let Some(approval) = session.and_then(|s| s.pending_approval.as_ref()) {
            ("Waiting for your approval", approval.title.clone())
        } else if let Some(input) = session.and_then(|s| s.pending_user_input.as_ref()) {
            ("Waiting for your answer", input.questions.get(input.question_index)
                .map(|question| question.question.clone()).unwrap_or_else(|| "Open the work session to respond.".into()))
        } else {
            (pocketcomet_runtime_label(runtime), match runtime {
                AgentRuntime::Working => "The reply will appear in PocketComet when Choro finishes.",
                AgentRuntime::Waiting => "Open the work session to review what Choro needs.",
                AgentRuntime::Ended => "Open the work session to check why Choro stopped.",
                _ => "Open the work session to see activity and results.",
            }.to_string())
        };
        let chat_id = chat.id;
        let project_id = chat.project_id;
        let open_work = style::secondary_button_compact("pocketcomet-chat-open-work", "Open work session")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.workspace.update(cx, |workspace, cx| workspace.set_active(project_id, cx));
                this.open_agent(chat_id, window, cx);
            }));
        let activity = h_flex()
            .w_full()
            .max_w(crate::ui::design::center_content_frame_max_w())
            .mx_auto()
            .px(crate::ui::design::agent_chat_gutter_x())
            .py_3()
            .gap_3()
            .items_start()
            .child(v_flex().flex_1().min_w(px(0.)).gap_1()
                .child(div().text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(pocketcomet_runtime_tone(runtime, cx)).child(status))
                .child(div().text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t2(cx)).whitespace_normal().child(detail)))
            .child(open_work);
        let back = style::secondary_button_compact("pocketcomet-chat-back", "Back")
            .icon(IconName::ArrowLeft)
            .on_click(cx.listener(|this, _, _, cx| {
                this.pocketcomet_selected_chat = None;
                cx.notify();
            }));
        let header = crate::ui::design::header::bar(cx)
            .child(
                crate::ui::design::header::title_col(cx)
                    .child(crate::ui::design::header::title(thread_title, cx))
                    .child(crate::ui::design::header::subtitle(
                        format!("{project_name} · {conversation_name}{teammate}"),
                        cx,
                    )),
            )
            .child(crate::ui::design::header::actions().child(back));
        let body = if responses.is_empty() {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child("Choro has not completed a reply in this thread yet.")
                .into_any_element()
        } else {
            v_flex()
                .id("pocketcomet-chat-transcript")
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .w_full()
                        .max_w(crate::ui::design::center_content_frame_max_w())
                        .mx_auto()
                        .px(crate::ui::design::agent_chat_gutter_x())
                        .py(crate::ui::design::center_column_pad_y())
                        .gap_5()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx))
                                .child(format!(
                                    "This is a read-only view. Continue the conversation in {conversation_name} and tag Choro when you want another reply."
                                )),
                        )
                        .children(responses.into_iter().rev().take(20).collect::<Vec<_>>().into_iter().rev().map(|(text, created_at)| {
                            v_flex()
                                .w_full()
                                .gap_1()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(crate::ui::design::indicator::lucide_icon(
                                            lucide_icons::Icon::Orbit,
                                            crate::ui::design::t3(cx),
                                            crate::ui::design::icon_sm(),
                                        ))
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_ui())
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .text_color(crate::ui::design::t2(cx))
                                                .child("Choro"),
                                        )
                                        .child(
                                            div()
                                                .text_size(crate::ui::design::text_label())
                                                .text_color(crate::ui::design::t4(cx))
                                                .child(branch_relative_time(created_at as i64)),
                                        ),
                                )
                                .child(
                                    div()
                                        .w_full()
                                        .pl_6()
                                        .text_size(crate::ui::design::text_body())
                                        .line_height(gpui::relative(1.5))
                                        .text_color(crate::ui::design::t1(cx))
                                        .whitespace_normal()
                                        .child(text),
                                )
                        })),
                )
                .into_any_element()
        };
        v_flex()
            .size_full()
            .child(header)
            .child(activity)
            .child(body)
            .into_any_element()
    }
}
