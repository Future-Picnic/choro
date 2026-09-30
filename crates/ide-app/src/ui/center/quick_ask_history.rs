use super::*;

use crate::state::quick_ask::{quick_ask_question_for_agent_chat, quick_ask_question_text};
use crate::ui::quick_ask::QuickAskSubmit;

/// Width of the conversations column inside the bounded history frame.
const QUICK_ASK_HISTORY_LIST_W: f32 = 304.0;

#[derive(Clone)]
struct QuickAskHistorySession {
    id: Uuid,
    exchanges: Vec<StoredQuickAskExchange>,
    newest_at: u64,
}

impl QuickAskHistorySession {
    fn first_question(&self) -> String {
        self.exchanges
            .first()
            .map(|exchange| quick_ask_question_text(&exchange.question))
            .unwrap_or_else(|| "Quick Ask conversation".to_string())
    }

    fn latest(&self) -> Option<&StoredQuickAskExchange> {
        self.exchanges.last()
    }

    fn scope_label(&self) -> &str {
        self.latest()
            .and_then(|exchange| exchange.project_name.as_deref())
            .unwrap_or("General")
    }
}

impl CenterArea {
    pub(super) fn render_quick_ask_history(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let history = self.quick_ask.read(cx).history().to_vec();
        let all_sessions = quick_ask_history_sessions(&history);
        let query = self
            .quick_ask_history_search
            .read(cx)
            .value()
            .trim()
            .to_string();
        let sessions = all_sessions
            .iter()
            .filter(|session| quick_ask_session_matches(session, &query))
            .cloned()
            .collect::<Vec<_>>();
        let selected_is_present = self
            .quick_ask_selected_session
            .is_some_and(|session_id| sessions.iter().any(|session| session.id == session_id));
        if !selected_is_present {
            self.quick_ask_selected_session = sessions.first().map(|session| session.id);
        }
        let selected = self
            .quick_ask_selected_session
            .and_then(|session_id| sessions.iter().find(|session| session.id == session_id))
            .cloned();

        let new_question = self.quick_ask_new_question_button(
            "quick-ask-history-new-question",
            "New Question",
            cx,
        );
        let conversation_count = all_sessions.len();
        let exchange_count = history.len();
        let subtitle = if conversation_count == 0 {
            "Your Quick Ask conversations will appear here".to_string()
        } else {
            format!(
                "{} · {}",
                pluralize(conversation_count, "conversation", "conversations"),
                pluralize(exchange_count, "answer", "answers")
            )
        };
        // The page header shares the bounded frame below it (list + content),
        // so the title and actions sit on the frame's edges rather than the
        // narrower default header measure.
        let frame_max_w =
            px(QUICK_ASK_HISTORY_LIST_W) + crate::ui::design::center_content_frame_max_w();
        let header = crate::ui::design::header::bar(cx)
            .max_w(frame_max_w)
            .child(
                crate::ui::design::header::title_col(cx)
                    .child(crate::ui::design::header::title("Ask History", cx))
                    .child(crate::ui::design::header::subtitle(subtitle, cx)),
            )
            .child(crate::ui::design::header::actions().child(new_question));

        let body = if all_sessions.is_empty() {
            self.render_quick_ask_history_empty(cx)
        } else {
            h_flex()
                .size_full()
                .min_h(px(0.))
                .child(self.render_quick_ask_history_list(
                    &sessions,
                    all_sessions.len(),
                    &query,
                    cx,
                ))
                .child(
                    selected
                        .map(|session| self.render_quick_ask_history_detail(&session, window, cx))
                        .unwrap_or_else(|| self.render_quick_ask_history_no_results(&query, cx)),
                )
                .into_any_element()
        };

        // The master-detail area lives in a bounded, centered frame instead of
        // stretching to the window edges: the conversation list would
        // otherwise hug the far-left of a fullscreen window with an ocean of
        // dead space before the centered transcript. The frame width is the
        // list column plus the agent-chat content frame, so transcript rows
        // land exactly on their usual measure inside it.
        v_flex()
            .size_full()
            .overflow_hidden()
            .bg(crate::ui::design::base(cx))
            .child(header)
            .child(div().w_full().h(px(1.)).bg(crate::ui::design::line(cx)))
            .child(
                div().flex_1().min_h(px(0.)).w_full().p_4().child(
                    div()
                        .size_full()
                        .max_w(frame_max_w)
                        .mx_auto()
                        .rounded(crate::ui::design::r_lg())
                        .border_1()
                        .border_color(crate::ui::design::line(cx))
                        .overflow_hidden()
                        .child(body),
                ),
            )
            .into_any_element()
    }

    fn render_quick_ask_history_no_results(
        &self,
        query: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_2()
            .px_6()
            .child(
                Icon::new(IconName::Search)
                    .size(crate::ui::design::icon_lg())
                    .text_color(crate::ui::design::t4(cx)),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_body())
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(crate::ui::design::t2(cx))
                    .child("No matching conversations"),
            )
            .child(
                div()
                    .max_w(px(360.))
                    .text_center()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t4(cx))
                    .child(format!("Nothing in Ask History matches “{query}”.")),
            )
            .into_any_element()
    }

    fn render_quick_ask_history_empty(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let new_question = self.quick_ask_new_question_button(
            "quick-ask-history-empty-new-question",
            "Ask a Question",
            cx,
        );
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .px_6()
            .child(
                div()
                    .size(px(48.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(crate::ui::design::r_lg())
                    .bg(crate::ui::design::accent_soft(cx))
                    .child(
                        Icon::new(IconName::Asterisk)
                            .size(px(22.))
                            .text_color(crate::ui::design::accent(cx)),
                    ),
            )
            .child(
                div()
                    .text_size(crate::ui::design::text_head())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t1(cx))
                    .child("No Quick Ask conversations yet"),
            )
            .child(
                div()
                    .max_w(px(390.))
                    .text_center()
                    .text_size(crate::ui::design::text_body())
                    .line_height(gpui::relative(1.5))
                    .text_color(crate::ui::design::t3(cx))
                    .child(
                        "Ask something small without starting an agent. Completed answers stay here for you to revisit.",
                    ),
            )
            .child(div().h(px(2.)))
            .child(new_question)
            .into_any_element()
    }

    fn render_quick_ask_history_list(
        &self,
        sessions: &[QuickAskHistorySession],
        total: usize,
        query: &str,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let count = sessions.len();
        let count_label = if query.is_empty() {
            count.to_string()
        } else {
            format!("{count} of {total}")
        };
        v_flex()
            .w(px(QUICK_ASK_HISTORY_LIST_W))
            .h_full()
            .flex_none()
            .min_h(px(0.))
            .border_r_1()
            .border_color(crate::ui::design::line(cx))
            .bg(crate::ui::design::surface(cx))
            .child(
                h_flex()
                    .w_full()
                    .h(px(42.))
                    .flex_none()
                    .px_3()
                    .items_center()
                    .child(
                        div()
                            .text_size(crate::ui::design::text_ui())
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(crate::ui::design::t2(cx))
                            .child("Conversations"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t4(cx))
                            .child(count_label),
                    ),
            )
            .child(
                div().w_full().flex_none().px_2().pb_2().child(
                    Input::new(&self.quick_ask_history_search)
                        .small()
                        .prefix(IconName::Search),
                ),
            )
            .child(
                v_flex()
                    .id("quick-ask-history-conversation-list")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .p_2()
                    .gap_0p5()
                    .when(sessions.is_empty(), |list| {
                        list.child(
                            div()
                                .w_full()
                                .px_2()
                                .py_3()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t4(cx))
                                .child("No matches"),
                        )
                    })
                    .children(
                        sessions
                            .iter()
                            .map(|session| self.render_quick_ask_history_session_row(session, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_quick_ask_history_session_row(
        &self,
        session: &QuickAskHistorySession,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let session_id = session.id;
        let selected = self.quick_ask_selected_session == Some(session_id);
        let title = session.first_question();
        let age = branch_relative_time(session.newest_at.min(i64::MAX as u64) as i64);
        let meta = format!(
            "{} · {} · {}",
            session.scope_label(),
            pluralize(session.exchanges.len(), "turn", "turns"),
            age
        );

        style::master_list_row(
            ("quick-ask-history-session", session.id.as_u128() as u64),
            selected,
            cx,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.quick_ask_selected_session = Some(session_id);
            cx.notify();
        }))
        .child(
            v_flex()
                .w_full()
                .min_w(px(0.))
                .px_2()
                .py_1p5()
                .gap_1()
                .items_start()
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(crate::ui::design::text_body())
                        .font_weight(if selected {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(crate::ui::design::t1(cx))
                        .child(title),
                )
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t4(cx))
                        .child(meta),
                ),
        )
        .into_any_element()
    }

    fn render_quick_ask_history_detail(
        &mut self,
        session: &QuickAskHistorySession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let title = clipped_line(&session.first_question(), 96);
        let latest = session.latest();
        let detail_meta = latest
            .map(|exchange| {
                format!(
                    "{} · {} · {}",
                    session.scope_label(),
                    exchange.provider,
                    exchange.model_label
                )
            })
            .unwrap_or_else(|| "General".to_string());
        let start_agent = self.quick_ask_start_agent_button(session, cx);

        // When this session is the one loaded in the shared Quick Ask state,
        // the detail is a live conversation: it shows the in-flight question,
        // the thinking indicator, and errors, and its transcript comes from
        // the state so a just-completed answer appears immediately.
        let (live_session, pending_question, phase, error, live_scope, live_agent) = {
            let state = self.quick_ask.read(cx);
            let is_live = state.session_id() == session.id;
            (
                is_live.then(|| state.session().to_vec()),
                is_live
                    .then(|| state.pending_question().map(str::to_string))
                    .flatten(),
                state.phase(),
                is_live.then(|| state.error().map(str::to_string)).flatten(),
                is_live.then(|| state.scope()),
                state.agent().clone(),
            )
        };
        let is_live = live_session.is_some();
        let thinking_here = is_live && phase == QuickAskPhase::Thinking;
        if !thinking_here {
            self.quick_ask_history_pending_started_at = None;
        } else if self.quick_ask_history_pending_started_at.is_none() {
            // A submission from another surface (the floating panel) still
            // needs a stable anchor for the elapsed timer.
            self.quick_ask_history_pending_started_at = Some(unix_now_secs());
        }
        let pending_started_at = self
            .quick_ask_history_pending_started_at
            .unwrap_or_else(unix_now_secs);

        let exchanges = live_session.unwrap_or_else(|| session.exchanges.clone());
        let exchange_count = exchanges.len();
        let transcript = exchanges
            .iter()
            .enumerate()
            .map(|(turn_index, exchange)| {
                self.render_quick_ask_exchange_as_agent(exchange, turn_index, window, cx)
            })
            .collect::<Vec<_>>();
        let pending_message = pending_question.map(|question| {
            let (project_id, provider, model_label) = {
                let scope_project = match live_scope {
                    Some(QuickAskScope::Project(project_id)) => Some(project_id),
                    _ => None,
                };
                (
                    scope_project,
                    live_agent.provider,
                    live_agent.model_label().to_string(),
                )
            };
            self.render_quick_ask_pending_as_agent(
                session.id,
                project_id,
                provider,
                &model_label,
                question,
                exchange_count.saturating_mul(2),
                pending_started_at,
                window,
                cx,
            )
        });
        let thinking = thinking_here
            .then(|| self.render_quick_ask_thinking_as_agent(session.id, pending_started_at, cx));
        let error_notice = error.map(|error| {
            div()
                .w_full()
                .px(crate::ui::design::agent_chat_gutter_x())
                .pb_2()
                .child(
                    div()
                        .w_full()
                        .max_w(crate::ui::design::agent_chat_content_max_w())
                        .mx_auto()
                        .child(style::agent_attention_strip(
                            "copy-quick-ask-history-error",
                            error,
                            cx,
                        )),
                )
                .into_any_element()
        });
        let composer = self.render_quick_ask_history_composer(
            session,
            phase,
            live_scope,
            &live_agent,
            window,
            cx,
        );

        v_flex()
            .flex_1()
            .min_w(px(0.))
            .h_full()
            .min_h(px(0.))
            .bg(crate::ui::design::base(cx))
            .child(
                // The bar spans the pane, but its content sits on the same
                // measure as the transcript rows below (gutter + max_w +
                // mx_auto), so title and actions stay attached to the
                // conversation column instead of drifting to the pane edges
                // when the sidebar is closed and the pane runs wide.
                div()
                    .w_full()
                    .flex_none()
                    .px(crate::ui::design::agent_chat_gutter_x())
                    .border_b_1()
                    .border_color(crate::ui::design::line(cx))
                    .child(
                        h_flex()
                            .w_full()
                            .min_w(px(0.))
                            .max_w(crate::ui::design::agent_chat_content_max_w())
                            .mx_auto()
                            .min_h(px(58.))
                            .py_2()
                            .gap_3()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .child(
                                        div()
                                            .w_full()
                                            .truncate()
                                            .text_size(crate::ui::design::text_body())
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(crate::ui::design::t1(cx))
                                            .child(title),
                                    )
                                    .child(
                                        div()
                                            .w_full()
                                            .truncate()
                                            .text_size(crate::ui::design::text_label())
                                            .text_color(crate::ui::design::t4(cx))
                                            .child(detail_meta),
                                    ),
                            )
                            .child(start_agent),
                    ),
            )
            .child(
                v_flex()
                    .id("quick-ask-history-transcript")
                    .flex_1()
                    .min_h(px(0.))
                    .overflow_y_scrollbar()
                    .child(
                        v_flex()
                            .w_full()
                            .py(crate::ui::design::center_column_pad_y())
                            .children(transcript)
                            .children(pending_message)
                            .children(thinking)
                            .children(error_notice),
                    ),
            )
            .child(composer)
            .into_any_element()
    }

    fn quick_ask_history_composer_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.quick_ask_history_composer {
            return input.clone();
        }
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .auto_grow(1, 5)
                .placeholder("Continue this conversation…")
        });
        cx.subscribe(&input, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })
        .detach();
        self.quick_ask_history_composer = Some(input.clone());
        input
    }

    fn submit_quick_ask_history_followup(
        &mut self,
        session_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(input) = self.quick_ask_history_composer.clone() else {
            return;
        };
        let question = input.read(cx).value().trim().to_string();
        if question.is_empty() || self.quick_ask.read(cx).phase() == QuickAskPhase::Thinking {
            return;
        }
        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.quick_ask_history_pending_started_at = Some(unix_now_secs());
        self.quick_ask.update(cx, |state, cx| {
            if state.session_id() != session_id {
                state.continue_conversation(session_id, cx);
            }
            state.submit(question, Vec::new(), cx);
        });
        cx.notify();
    }

    /// The follow-up composer under a history transcript: the same shared
    /// composer components the panel uses, minus per-question controls —
    /// scope and model are inherited from the conversation and shown as
    /// quiet meta instead.
    fn render_quick_ask_history_composer(
        &mut self,
        session: &QuickAskHistorySession,
        phase: QuickAskPhase,
        live_scope: Option<QuickAskScope>,
        live_agent: &ide_core::config::GenerationAgent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let session_id = session.id;
        let input = self.quick_ask_history_composer_input(window, cx);
        let has_draft = !input.read(cx).value().trim().is_empty();
        let can_send = has_draft && phase != QuickAskPhase::Thinking;
        let scope_label = match live_scope {
            Some(QuickAskScope::Project(project_id)) => self
                .workspace
                .read(cx)
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.name.clone())
                .unwrap_or_else(|| "General".to_string()),
            Some(QuickAskScope::General) => "General".to_string(),
            None => session.scope_label().to_string(),
        };
        let composer_meta = format!("{scope_label} · {}", live_agent.model_label());
        let view = cx.entity().clone();

        div()
            .w_full()
            .flex_none()
            .px(crate::ui::design::agent_chat_gutter_x())
            .pb_4()
            .pt_1()
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.))
                    .max_w(crate::ui::design::agent_chat_content_max_w())
                    .mx_auto()
                    .key_context("QuickAsk")
                    .on_action(cx.listener(move |this, _: &QuickAskSubmit, window, cx| {
                        this.submit_quick_ask_history_followup(session_id, window, cx);
                    }))
                    .child(
                        style::composer_frame(cx)
                            .child(
                                v_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .min_h(crate::ui::design::composer_input_min_h())
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_h(px(0.))
                                            .child(style::composer_text_input(&input).h_full()),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .min_w(px(0.))
                                            .truncate()
                                            .text_size(crate::ui::design::text_ui())
                                            .text_color(crate::ui::design::t4(cx))
                                            .child(composer_meta),
                                    )
                                    .child(div().flex_1())
                                    .child({
                                        let button =
                                            style::composer_send("quick-ask-history-send", cx)
                                                .tooltip(move |window, cx| {
                                                    gpui_component::tooltip::Tooltip::new(
                                                        if phase == QuickAskPhase::Thinking {
                                                            "Quick Ask is thinking…"
                                                        } else {
                                                            "Ask"
                                                        },
                                                    )
                                                    .build(window, cx)
                                                });
                                        if can_send {
                                            button
                                                .cursor_pointer()
                                                .hover(|button| {
                                                    button.bg(crate::ui::design::accent_2(cx))
                                                })
                                                .on_click(move |_, window, cx| {
                                                    view.update(cx, |this, cx| {
                                                        this.submit_quick_ask_history_followup(
                                                            session_id, window, cx,
                                                        );
                                                    });
                                                })
                                        } else {
                                            button.opacity(0.55)
                                        }
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn quick_ask_new_question_button(
        &self,
        id: &'static str,
        label: &'static str,
        cx: &mut Context<Self>,
    ) -> Button {
        let quick_ask = self.quick_ask.clone();
        style::primary_button_compact(id, label, cx)
            .icon(IconName::Plus)
            .tooltip("Open a fresh Quick Ask")
            .on_click(move |_, _, cx| {
                quick_ask.update(cx, |state, cx| {
                    state.begin_session(cx);
                    state.request_panel_open(cx);
                });
            })
    }

    fn quick_ask_start_agent_button(
        &self,
        session: &QuickAskHistorySession,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let latest = session.latest();
        let (direct_project, projects) = {
            let workspace = self.workspace.read(cx);
            let original_project =
                latest
                    .and_then(|exchange| exchange.project_id)
                    .filter(|project_id| {
                        workspace
                            .projects
                            .iter()
                            .any(|project| project.id == *project_id)
                    });
            let direct_project = original_project.or_else(|| {
                latest
                    .is_some_and(|exchange| exchange.project_id.is_none())
                    .then_some(workspace.active)
                    .flatten()
            });
            let projects = workspace
                .projects
                .iter()
                .map(|project| (project.id, project.name.clone()))
                .collect::<Vec<_>>();
            (direct_project, projects)
        };

        let prompt = quick_ask_agent_prompt(session);
        let session_id = session.id;
        if let Some(project_id) = direct_project {
            return style::secondary_button_compact(
                ("quick-ask-history-start-agent", session_id.as_u128() as u64),
                "Start Agent",
            )
            .icon(IconName::Bot)
            .tooltip("Open an unsent agent draft with this conversation")
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_new_agent_with_prompt(project_id, prompt.clone(), window, cx);
            }))
            .into_any_element();
        }

        let center = cx.entity().clone();
        style::secondary_button_compact(
            ("quick-ask-history-start-agent", session_id.as_u128() as u64),
            if projects.is_empty() {
                "No Project"
            } else {
                "Choose Project"
            },
        )
        .icon(IconName::Bot)
        .dropdown_caret(!projects.is_empty())
        .disabled(projects.is_empty())
        .tooltip(if projects.is_empty() {
            "Add a project before starting an agent"
        } else {
            "Choose where to open the unsent agent draft"
        })
        .dropdown_menu(move |mut menu, window, _| {
            for (project_id, project_name) in projects.clone() {
                let prompt = prompt.clone();
                menu = menu.item(
                    PopupMenuItem::new(project_name)
                        .icon(IconName::FolderOpen)
                        .on_click(window.listener_for(
                            &center,
                            move |this: &mut Self, _, window, cx| {
                                this.open_new_agent_with_prompt(
                                    project_id,
                                    prompt.clone(),
                                    window,
                                    cx,
                                );
                            },
                        )),
                );
            }
            menu
        })
        .into_any_element()
    }

    /// Render a persisted Quick Ask turn through the actual agent-chat message
    /// path. This is deliberately not a look-alike: both surfaces now share the
    /// same Markdown parser, list/table/code renderers, metadata, and row shell.
    pub(crate) fn render_quick_ask_exchange_as_agent(
        &self,
        exchange: &StoredQuickAskExchange,
        turn_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent = self.quick_ask_presentation_agent(
            exchange.session_id,
            exchange.project_id,
            quick_ask_provider(&exchange.provider),
            &exchange.model_label,
            exchange.created_at,
            cx,
        );
        let (question, answer) = quick_ask_agent_messages(exchange);
        let message_index = turn_index.saturating_mul(2);

        v_flex()
            .w_full()
            .min_w(px(0.))
            .child(self.render_agent_chat_message_row(&agent, message_index, &question, window, cx))
            .child(self.render_agent_chat_message_row(
                &agent,
                message_index + 1,
                &answer,
                window,
                cx,
            ))
            .into_any_element()
    }

    pub(crate) fn render_quick_ask_pending_as_agent(
        &self,
        session_id: Uuid,
        project_id: Option<ProjectId>,
        provider: AgentKind,
        model_label: &str,
        question: String,
        message_index: usize,
        created_at: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent = self.quick_ask_presentation_agent(
            session_id,
            project_id,
            provider,
            model_label,
            created_at,
            cx,
        );
        let message = AgentChatMessage::User {
            text: quick_ask_question_for_agent_chat(&question),
            display_text: None,
            tags: Vec::new(),
            created_at,
        };
        self.render_agent_chat_message_row(&agent, message_index, &message, window, cx)
    }

    pub(crate) fn render_quick_ask_thinking_as_agent(
        &self,
        session_id: Uuid,
        started_at: u64,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let session = AgentChatSession {
            agent_id: session_id,
            title: "Quick Ask".to_string(),
            chat_session_id: None,
            cli_session_id: None,
            hidden_from_notifications: true,
            is_compacting: false,
            status: AgentChatStatus::Running,
            interaction_mode: AgentInteractionMode::Default,
            composer_text: String::new(),
            messages: Vec::new(),
            timeline: Vec::new(),
            queued_turns: Vec::new(),
            work_log: Vec::new(),
            pending_user_input: None,
            pending_approval: None,
            proposed_plan: None,
            changed_files: crate::state::agent_chat::ChangedFilesSummary::default(),
            usage: None,
            started_running_at: Some(started_at),
            last_activity_at: started_at,
        };
        let content = self.render_agent_activity_indicator(&session, cx);
        self.wrap_agent_chat_row(content)
    }

    fn quick_ask_presentation_agent(
        &self,
        session_id: Uuid,
        project_id: Option<ProjectId>,
        provider: AgentKind,
        model_label: &str,
        created_at: u64,
        cx: &App,
    ) -> AgentRecord {
        let workspace = self.workspace.read(cx);
        let project = project_id.and_then(|project_id| {
            workspace
                .projects
                .iter()
                .find(|project| project.id == project_id)
        });
        let project_id = project
            .map(|project| project.id)
            .or(project_id)
            .unwrap_or(ProjectId(Uuid::nil()));
        let project_path = project
            .map(|project| project.path.clone())
            .unwrap_or_default();
        let model = AgentModel::default_for(provider);
        let mut agent = AgentRecord::new(
            project_id,
            project_path,
            "Quick Ask",
            "",
            provider,
            model,
            AgentEffort::Low,
            AgentAccessMode::AskForApproval,
        );
        agent.id = session_id;
        agent.created_at = created_at;
        agent.updated_at = created_at;
        if provider == AgentKind::OpenCode {
            agent.external_model_label = Some(model_label.to_string());
        }
        agent
    }
}

fn quick_ask_provider(label: &str) -> AgentKind {
    match label {
        "Claude" => AgentKind::Claude,
        "OpenCode" => AgentKind::OpenCode,
        _ => AgentKind::Codex,
    }
}

fn quick_ask_agent_messages(
    exchange: &StoredQuickAskExchange,
) -> (AgentChatMessage, AgentChatMessage) {
    (
        AgentChatMessage::User {
            text: quick_ask_question_for_agent_chat(&exchange.question),
            display_text: None,
            tags: Vec::new(),
            created_at: exchange.created_at,
        },
        AgentChatMessage::Assistant {
            message_id: Some(exchange.id.to_string()),
            text: exchange.answer.clone(),
            created_at: exchange.created_at,
        },
    )
}

fn quick_ask_session_matches(session: &QuickAskHistorySession, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    session.exchanges.iter().any(|exchange| {
        exchange.question.to_lowercase().contains(&needle)
            || exchange.answer.to_lowercase().contains(&needle)
            || exchange.provider.to_lowercase().contains(&needle)
            || exchange.model_label.to_lowercase().contains(&needle)
            || (exchange.project_name.is_none() && "general".contains(&needle))
            || exchange
                .project_name
                .as_deref()
                .is_some_and(|name| name.to_lowercase().contains(&needle))
    })
}

fn quick_ask_history_sessions(history: &[StoredQuickAskExchange]) -> Vec<QuickAskHistorySession> {
    let mut sessions = Vec::<QuickAskHistorySession>::new();
    let mut indices = HashMap::<Uuid, usize>::new();
    for exchange in history {
        if let Some(index) = indices.get(&exchange.session_id).copied() {
            sessions[index].exchanges.push(exchange.clone());
        } else {
            indices.insert(exchange.session_id, sessions.len());
            sessions.push(QuickAskHistorySession {
                id: exchange.session_id,
                exchanges: vec![exchange.clone()],
                newest_at: exchange.created_at,
            });
        }
    }
    for session in &mut sessions {
        // The persisted global list is newest-first; transcripts read oldest-first.
        session.exchanges.reverse();
    }
    sessions
}

fn quick_ask_agent_prompt(session: &QuickAskHistorySession) -> String {
    let mut prompt = String::from(
        "Continue from this Quick Ask conversation. The transcript below is reference context; do not assume any action has already been taken.\n\n",
    );
    for exchange in &session.exchanges {
        prompt.push_str("User:\n");
        prompt.push_str(exchange.question.trim());
        prompt.push_str("\n\nAssistant:\n");
        prompt.push_str(exchange.answer.trim());
        prompt.push_str("\n\n");
    }
    prompt.push_str("Continue helping from here, but do not send or change anything until I ask.");
    prompt
}

fn clipped_line(value: &str, limit: usize) -> String {
    let compact = compact_whitespace(value);
    if compact.chars().count() <= limit {
        return compact;
    }
    let mut clipped = compact
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    clipped.push('…');
    clipped
}

fn pluralize(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exchange(
        session_id: Uuid,
        question: &str,
        answer: &str,
        created_at: u64,
    ) -> StoredQuickAskExchange {
        StoredQuickAskExchange {
            id: Uuid::new_v4(),
            session_id,
            project_id: None,
            project_name: None,
            question: question.to_string(),
            answer: answer.to_string(),
            provider: "Codex".to_string(),
            model_label: "GPT-5.6 Luna".to_string(),
            created_at,
        }
    }

    #[test]
    fn global_history_groups_sessions_and_restores_transcript_order() {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let history = vec![
            exchange(first, "follow up", "second answer", 30),
            exchange(second, "newer session", "answer", 20),
            exchange(first, "first question", "first answer", 10),
        ];

        let sessions = quick_ask_history_sessions(&history);

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, first);
        assert_eq!(sessions[0].exchanges[0].question, "first question");
        assert_eq!(sessions[0].exchanges[1].question, "follow up");
        assert_eq!(sessions[1].id, second);
    }

    #[test]
    fn quick_ask_turns_keep_markdown_when_adapted_to_agent_messages() {
        let session_id = Uuid::new_v4();
        let stored = exchange(
            session_id,
            "What is this?",
            "It is designed around:\n\n- Projects\n- Agents\n- Tasks",
            42,
        );

        let (question, answer) = quick_ask_agent_messages(&stored);

        assert!(matches!(
            question,
            AgentChatMessage::User { text, created_at: 42, .. }
                if text == "What is this?"
        ));
        assert!(matches!(
            answer,
            AgentChatMessage::Assistant { text, created_at: 42, .. }
                if text.contains("- Projects\n- Agents\n- Tasks")
        ));
    }

    #[test]
    fn quick_ask_images_reach_the_agent_attachment_renderer() {
        let session_id = Uuid::new_v4();
        let image = PathBuf::from("/tmp/quick-ask-reference.png");
        let stored_question = crate::ui::center::attachment_helpers::prompt_with_attached_files(
            "What is in this image?",
            std::slice::from_ref(&image),
        );
        let stored = exchange(session_id, &stored_question, "An image answer", 42);

        let (question, _) = quick_ask_agent_messages(&stored);
        let AgentChatMessage::User { text, .. } = question else {
            panic!("expected user message");
        };
        let (visible_text, attachments) =
            crate::ui::center::attachment_helpers::split_prompt_attached_files(&text);

        assert_eq!(visible_text, "What is in this image?");
        assert_eq!(attachments, vec![image]);
    }

    #[test]
    fn agent_handoff_contains_the_whole_conversation_in_order() {
        let session_id = Uuid::new_v4();
        let session = QuickAskHistorySession {
            id: session_id,
            newest_at: 2,
            exchanges: vec![
                exchange(session_id, "first question", "first answer", 1),
                exchange(session_id, "follow up", "second answer", 2),
            ],
        };

        let prompt = quick_ask_agent_prompt(&session);

        let first = prompt.find("first question").unwrap();
        let follow_up = prompt.find("follow up").unwrap();
        assert!(first < follow_up);
        assert!(prompt.contains("first answer"));
        assert!(prompt.contains("second answer"));
    }

    #[test]
    fn history_search_matches_questions_answers_projects_and_models() {
        let session_id = Uuid::new_v4();
        let mut stored = exchange(session_id, "How does shipping work?", "It opens a PR", 1);
        stored.project_name = Some("Choro Desktop".to_string());
        stored.model_label = "GPT-5.6 Luna".to_string();
        let session = QuickAskHistorySession {
            id: session_id,
            newest_at: 1,
            exchanges: vec![stored],
        };

        assert!(quick_ask_session_matches(&session, "shipping"));
        assert!(quick_ask_session_matches(&session, "opens a pr"));
        assert!(quick_ask_session_matches(&session, "choro desktop"));
        assert!(quick_ask_session_matches(&session, "luna"));
        assert!(!quick_ask_session_matches(&session, "unrelated"));

        let general = QuickAskHistorySession {
            id: session_id,
            newest_at: 1,
            exchanges: vec![exchange(session_id, "question", "answer", 1)],
        };
        assert!(quick_ask_session_matches(&general, "general"));
    }
}
