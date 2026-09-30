//! The recent-agents list under the new-agent composer.
//!
//! Selecting a project opens this screen, so the composer is the first thing
//! you meet. These rows are the second: the project's most recent conversations,
//! close enough to step back into without going looking for them.

use super::*;

/// Rows shown under the composer. Enough to recognise what you were doing,
/// short enough that the composer stays the subject of the screen.
const RECENT_AGENTS: usize = 4;
const WEEK_SECS: u64 = 7 * 24 * 60 * 60;

fn weekly_outcome_preview(outcome: &str) -> String {
    let flattened = outcome
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if flattened.chars().count() <= 220 {
        return flattened;
    }
    format!(
        "{}…",
        flattened.chars().take(219).collect::<String>().trim_end()
    )
}

impl CenterArea {
    pub(super) fn render_new_agent_recent_agents(
        &self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let mut agents = self.agents.read(cx).records_for_project(project);
        if agents.is_empty() {
            return None;
        }
        agents.sort_by_key(|agent| std::cmp::Reverse(self.recent_agent_activity(agent, cx)));
        agents.truncate(RECENT_AGENTS);

        let rows = agents
            .into_iter()
            .map(|agent| self.render_recent_agent_row(&agent, cx))
            .collect::<Vec<_>>();

        Some(
            v_flex()
                .w_full()
                .pt_6()
                .gap_1()
                .child(
                    div()
                        .px_1()
                        .pb_1()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t4(cx))
                        .child("Latest agents"),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    pub(super) fn render_fleet_weekly_digest(
        &self,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let cutoff = unix_now_secs().saturating_sub(WEEK_SECS);
        let records = self.agents.read(cx);
        let mut entries = self
            .agent_summaries
            .values()
            .filter(|summary| summary.updated_at >= cutoff)
            .filter_map(|summary| {
                let agent = records.agent(summary.agent_id)?.clone();
                (agent.project_id == project).then(|| (agent, summary.clone()))
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|(_, summary)| std::cmp::Reverse(summary.updated_at));
        if entries.is_empty() {
            return None;
        }
        let count = entries.len();
        let rows = entries.into_iter().map(|(agent, summary)| {
            let agent_id = agent.id;
            let outcome = summary
                .outcome_text
                .as_deref()
                .filter(|outcome| !outcome.trim().is_empty())
                .map(weekly_outcome_preview);
            h_flex()
                .id(SharedString::from(format!("weekly-brain-agent-{agent_id}")))
                .w_full()
                .min_w(px(0.))
                .items_start()
                .gap_2()
                .px_1p5()
                .py_2()
                .rounded(crate::ui::design::r_sm())
                .cursor_pointer()
                .hover(|row| row.bg(crate::ui::design::hover(cx)))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_agent(agent_id, window, cx);
                }))
                .child(
                    div()
                        .mt(px(5.))
                        .child(crate::ui::agent_status_style::status_dot(
                            agent.status,
                            6.,
                            cx,
                        )),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w(px(0.))
                        .gap_0p5()
                        .child(
                            h_flex()
                                .w_full()
                                .min_w(px(0.))
                                .gap_2()
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(crate::ui::design::text_ui())
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(crate::ui::design::t1(cx))
                                        .child(agent.title),
                                )
                                .child(
                                    div()
                                        .flex_none()
                                        .text_size(crate::ui::design::text_label())
                                        .text_color(crate::ui::design::t4(cx))
                                        .child(branch_relative_time(summary.updated_at as i64)),
                                ),
                        )
                        .when_some(outcome, |column, outcome| {
                            column.child(
                                div()
                                    .max_h(px(32.))
                                    .overflow_hidden()
                                    .text_size(crate::ui::design::text_label())
                                    .line_height(gpui::relative(1.35))
                                    .text_color(crate::ui::design::t3(cx))
                                    .child(outcome),
                            )
                        }),
                )
        });

        Some(
            v_flex()
                .w_full()
                .pt_6()
                .gap_1()
                .child(
                    h_flex()
                        .px_1()
                        .pb_1()
                        .child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t4(cx))
                                .child("What changed this week"),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_label())
                                .text_color(crate::ui::design::t4(cx))
                                .child(format!("{count} agents")),
                        ),
                )
                .children(rows)
                .into_any_element(),
        )
    }

    fn render_recent_agent_row(
        &self,
        agent: &AgentRecord,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let status = self
            .agent_chats
            .read(cx)
            .session(agent.id)
            .map(|session| session.status);
        // Waiting is the only state worth colouring: it is the one that means
        // the agent cannot continue without you.
        let (dot, waiting) = match status {
            Some(AgentChatStatus::Running | AgentChatStatus::Cancelling) => {
                (crate::ui::design::accent(cx), false)
            }
            Some(AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady) => {
                (crate::ui::design::amber(cx), true)
            }
            Some(AgentChatStatus::Failed) => (crate::ui::design::rose(cx), false),
            _ => (crate::ui::design::t4(cx), false),
        };
        let detail = match status {
            Some(AgentChatStatus::Running) => "Running".to_string(),
            Some(AgentChatStatus::Cancelling) => "Stopping".to_string(),
            Some(AgentChatStatus::WaitingForUser) => "Waiting for you".to_string(),
            Some(AgentChatStatus::PlanReady) => "Plan ready".to_string(),
            Some(AgentChatStatus::Failed) => "Failed".to_string(),
            _ if !agent.changed_files.is_empty() => {
                format!("{} files changed", agent.changed_files.len())
            }
            _ => String::new(),
        };
        let age = branch_relative_time(self.recent_agent_activity(agent, cx) as i64);
        let agent_id = agent.id;

        h_flex()
            .id(SharedString::from(format!("recent-agent-{agent_id}")))
            .w_full()
            .items_center()
            .gap_2()
            .px_1p5()
            .py_1p5()
            .rounded(crate::ui::design::r_sm())
            .cursor_pointer()
            .hover(|style| style.bg(crate::ui::design::hover(cx)))
            .child(div().flex_none().size(px(5.)).rounded_full().bg(dot))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(if waiting {
                        crate::ui::design::amber(cx)
                    } else {
                        crate::ui::design::t1(cx)
                    })
                    .child(SharedString::from(agent.title.clone())),
            )
            .when(!detail.is_empty(), |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t4(cx))
                        .child(SharedString::from(detail)),
                )
            })
            .when(!age.is_empty(), |row| {
                row.child(
                    div()
                        .flex_none()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t4(cx))
                        .child(SharedString::from(age)),
                )
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_agent(agent_id, window, cx);
            }))
            .into_any_element()
    }

    /// Newest meaningful timestamp: live chat activity when the session is
    /// loaded, otherwise the record's own clock.
    fn recent_agent_activity(&self, agent: &AgentRecord, cx: &App) -> u64 {
        self.agent_chats
            .read(cx)
            .session(agent.id)
            .map(|session| session.last_activity_at)
            .filter(|activity| *activity > 0)
            .unwrap_or(agent.updated_at)
    }
}

#[cfg(test)]
mod tests {
    use super::weekly_outcome_preview;

    #[test]
    fn weekly_outcome_preview_flattens_and_bounds_outcomes() {
        assert_eq!(weekly_outcome_preview("Done\n\nVerified"), "Done Verified");
        let excerpt = weekly_outcome_preview(&"x".repeat(400));
        assert_eq!(excerpt.chars().count(), 220);
        assert!(excerpt.ends_with('…'));
    }
}
