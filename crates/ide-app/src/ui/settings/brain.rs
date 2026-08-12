use super::*;

fn short_outcome(outcome: Option<&str>) -> Option<&str> {
    outcome.map(str::trim).filter(|outcome| !outcome.is_empty())
}

fn summary_matches(agent: &AgentRecord, summary: &str, outcome: Option<&str>, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    agent.title.to_lowercase().contains(&query)
        || summary.to_lowercase().contains(&query)
        || short_outcome(outcome).is_some_and(|outcome| outcome.to_lowercase().contains(&query))
}

fn summary_markdown_style() -> TextViewStyle {
    TextViewStyle::default()
        .paragraph_gap(gpui::rems(0.55))
        .heading_font_size(|level, _| match level {
            1 => px(18.),
            2 => px(15.),
            _ => px(14.),
        })
}

fn open_knowledge_summary(
    agent_id: Uuid,
    agent_title: String,
    markdown: String,
    window: &mut Window,
    cx: &mut App,
) {
    window.open_dialog(cx, move |dialog, window, cx| {
        dialog
            .w(px(720.))
            .margin_top(px(64.))
            .title(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(crate::ui::confirm::icon_badge(
                        IconName::BookOpen,
                        crate::ui::design::accent(cx),
                        cx,
                    ))
                    .child(
                        div()
                            .min_w(px(0.))
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(agent_title.clone()),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .h(px(520.))
                    .min_h(px(0.))
                    .overflow_hidden()
                    .child(
                        TextView::markdown(
                            (
                                "settings-knowledge-summary-modal",
                                agent_id.as_u128() as u64,
                            ),
                            markdown.clone(),
                            window,
                            cx,
                        )
                        .h_full()
                        .selectable(true)
                        .scrollable(true)
                        .style(summary_markdown_style()),
                    ),
            )
    });
}

impl SettingsView {
    fn render_knowledge_agent(
        &mut self,
        agent: AgentRecord,
        summary: ide_core::local_store::StoredAgentSummary,
        _window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let title = if agent.title.trim().is_empty() {
            "Untitled agent".to_string()
        } else {
            agent.title.clone()
        };
        let modal_title = title.clone();
        let markdown = summary.summary_text.clone();
        let outcome = short_outcome(summary.outcome_text.as_deref()).unwrap_or("");

        h_flex()
            .w_full()
            .min_w(px(0.))
            .min_h(px(58.))
            .items_center()
            .gap_3()
            .px_3()
            .py_2p5()
            .border_t_1()
            .border_color(crate::ui::design::line(cx).opacity(0.55))
            .child(
                div()
                    .w(px(220.))
                    .flex_none()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_body())
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(crate::ui::design::t1(cx))
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .whitespace_normal()
                    .text_size(crate::ui::design::text_ui())
                    .line_height(gpui::relative(1.35))
                    .text_color(crate::ui::design::t2(cx))
                    .child(outcome.to_string()),
            )
            .child(
                crate::ui::style::secondary_button_compact(
                    (
                        "settings-knowledge-summary-expand",
                        agent_id.as_u128() as u64,
                    ),
                    "Expand",
                )
                .on_click(cx.listener(move |_, _, window, cx| {
                    open_knowledge_summary(
                        agent_id,
                        modal_title.clone(),
                        markdown.clone(),
                        window,
                        cx,
                    );
                })),
            )
            .into_any_element()
    }

    pub(super) fn render_brain_section(
        &mut self,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::AnyElement {
        let selected_project = self.brain_project;
        let project_label = selected_project
            .and_then(|id| {
                self.projects
                    .iter()
                    .find(|candidate| candidate.id == id)
                    .map(|candidate| candidate.name.clone())
            })
            .unwrap_or_else(|| "Choose project".to_string());
        let query = self.brain_search.read(cx).value().trim().to_lowercase();
        let summaries = self
            .brain_summaries
            .iter()
            .cloned()
            .map(|summary| (summary.agent_id, summary))
            .collect::<HashMap<_, _>>();
        let mut agents = self
            .brain_agents
            .iter()
            .filter(|agent| {
                !agent.hidden_doc_assistant && Some(agent.project_id) == selected_project
            })
            .cloned()
            .filter_map(|agent| {
                let summary = summaries.get(&agent.id)?.clone();
                summary_matches(
                    &agent,
                    &summary.summary_text,
                    summary.outcome_text.as_deref(),
                    &query,
                )
                .then_some((agent, summary))
            })
            .collect::<Vec<_>>();
        agents.sort_by_key(|(agent, summary)| {
            std::cmp::Reverse(summary.updated_at.max(agent.updated_at))
        });
        let no_results = agents.is_empty();

        let mut agent_list = v_flex()
            .w_full()
            .min_w(px(0.))
            .overflow_hidden()
            .rounded(crate::ui::design::r_lg())
            .border_1()
            .border_color(crate::ui::design::line_2(cx))
            .bg(crate::ui::design::surface(cx).opacity(0.55))
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.))
                    .h(px(36.))
                    .items_center()
                    .gap_3()
                    .px_3()
                    .bg(crate::ui::design::surface_2(cx).opacity(0.72))
                    .text_size(crate::ui::design::text_label())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::t3(cx))
                    .child(div().w(px(220.)).flex_none().child("AGENT"))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .child("SHORT SUMMARY"),
                    )
                    .child(div().w(px(66.)).flex_none()),
            );
        for (agent, summary) in agents {
            agent_list = agent_list.child(self.render_knowledge_agent(agent, summary, window, cx));
        }

        v_flex()
            .w_full()
            .min_w(px(0.))
            .overflow_hidden()
            .gap_4()
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.))
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w_full()
                            .flex_1()
                            .min_w(px(0.))
                            .overflow_hidden()
                            .child(
                                Input::new(&self.brain_search)
                                    .w_full()
                                    .prefix(IconName::Search),
                            ),
                    )
                    .child({
                        let projects = self.projects.clone();
                        let view = cx.entity();
                        crate::ui::style::dialog_neutral_button(
                            "settings-knowledge-project-picker",
                            project_label,
                            cx,
                        )
                        .flex_none()
                        .icon(IconName::ChevronDown)
                        .dropdown_menu(move |mut menu, window_ref, _| {
                            for source in projects.clone() {
                                let id = source.id;
                                menu = menu.item(
                                    PopupMenuItem::new(source.name.clone())
                                        .checked(selected_project == Some(id))
                                        .on_click(window_ref.listener_for(
                                            &view,
                                                move |this: &mut SettingsView, _, _, cx| {
                                                    this.brain_project = Some(id);
                                                    cx.notify();
                                                },
                                        )),
                                );
                            }
                            menu
                        })
                    }),
            )
            .when(no_results, |page| {
                page.child(
                    v_flex()
                        .w_full()
                        .items_center()
                        .gap_2()
                        .py_12()
                        .child(
                            Icon::new(IconName::BookOpen)
                                .size(crate::ui::design::icon_lg())
                                .text_color(crate::ui::design::t4(cx)),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(crate::ui::design::t2(cx))
                                .child(if query.is_empty() {
                                    "No summaries in this project yet"
                                } else {
                                    "No matching agent summaries"
                                }),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t4(cx))
                                .child(if query.is_empty() {
                                    "Summaries appear here after an agent finishes, ships, or rejoins."
                                } else {
                                    "Try another word or choose a different project."
                                }),
                        ),
                )
            })
            .when(!no_results, |page| page.child(agent_list))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn agent(title: &str) -> AgentRecord {
        AgentRecord::new(
            ProjectId::new(),
            PathBuf::new(),
            title,
            "",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::Medium,
            ide_core::AgentAccessMode::default(),
        )
    }

    #[test]
    fn short_outcome_ignores_missing_or_blank_values() {
        assert_eq!(short_outcome(None), None);
        assert_eq!(short_outcome(Some("   ")), None);
        assert_eq!(
            short_outcome(Some("  Shipped the fix.  ")),
            Some("Shipped the fix.")
        );
    }

    #[test]
    fn search_matches_agent_title_outcome_and_full_summary() {
        let agent = agent("Backend migration");
        assert!(summary_matches(&agent, "## Decisions", None, "backend"));
        assert!(summary_matches(
            &agent,
            "## Decisions",
            Some("Shipped the SQLite migration"),
            "shipped"
        ));
        assert!(summary_matches(
            &agent,
            "## Decisions\nUse SQLite",
            None,
            "sqlite"
        ));
        assert!(!summary_matches(
            &agent,
            "Nothing relevant",
            Some("No interface work"),
            "frontend"
        ));
    }
}
