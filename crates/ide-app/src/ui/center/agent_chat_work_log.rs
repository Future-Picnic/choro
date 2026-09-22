use super::agent_chat_runtime::{
    AGENT_CODE_REVIEW_PROMPT, AGENT_CODE_REVIEW_REQUEST_MARKER, AGENT_VERIFY_REQUEST_MARKER,
};
use super::*;

#[derive(Clone, Copy)]
enum AgentActivityKind {
    Compacting,
    Working,
    Planning,
    Reviewing,
    Verifying,
}

impl AgentActivityKind {
    fn label(self) -> &'static str {
        match self {
            Self::Compacting => "Compacting context…",
            Self::Working => "Working",
            Self::Planning => "Planning",
            Self::Reviewing => "Reviewing",
            Self::Verifying => "Verifying",
        }
    }

    fn display_label(self, running_for: u64) -> String {
        match self {
            // The turn timer includes work before compaction began.
            Self::Compacting => self.label().to_string(),
            _ => format!("{} for {}", self.label(), compact_duration(running_for)),
        }
    }

    fn center_icon(self) -> Option<lucide_icons::Icon> {
        match self {
            Self::Working | Self::Compacting => None,
            Self::Planning => Some(lucide_icons::Icon::ListChecks),
            Self::Reviewing => Some(lucide_icons::Icon::SearchCode),
            Self::Verifying => Some(lucide_icons::Icon::Check),
        }
    }
}

fn agent_activity_kind(session: &crate::state::agent_chat::AgentChatSession) -> AgentActivityKind {
    if session.is_compacting && session.status == AgentChatStatus::Running {
        return AgentActivityKind::Compacting;
    }
    let latest_user_text = session.timeline.iter().rev().find_map(|item| match item {
        AgentChatTimelineItem::Message(AgentChatMessage::User { text, .. }) => Some(text.as_str()),
        _ => None,
    });
    if latest_user_text.is_some_and(|text| text.starts_with(AGENT_VERIFY_REQUEST_MARKER)) {
        AgentActivityKind::Verifying
    } else if latest_user_text.is_some_and(|text| {
        text.starts_with(AGENT_CODE_REVIEW_REQUEST_MARKER) || text == AGENT_CODE_REVIEW_PROMPT
    }) {
        AgentActivityKind::Reviewing
    } else if session.interaction_mode == AgentInteractionMode::Plan {
        AgentActivityKind::Planning
    } else {
        AgentActivityKind::Working
    }
}

impl CenterArea {
    pub(super) fn render_agent_work_log_group(
        &self,
        agent: &AgentRecord,
        index: usize,
        activity: &[AgentChatTimelineItem],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let agent_id = agent.id;
        let artifact_filter = VisualizationArtifactFilter::new(agent.id, agent.runtime_path());
        let expanded = self
            .agent_chat_expanded_work_log_groups
            .contains(&(agent_id, index));
        let total = activity_step_count(activity, &artifact_filter);
        let failed = activity.iter().any(|item| {
            matches!(
                item,
                AgentChatTimelineItem::WorkLog(entry)
                    if !is_noise_work_log(entry) && entry.status == WorkLogStatus::Failed
            )
        });
        let in_progress = activity.iter().any(|item| {
            matches!(
                item,
                AgentChatTimelineItem::WorkLog(entry)
                    if !is_noise_work_log(entry)
                        && matches!(entry.status, WorkLogStatus::Pending | WorkLogStatus::InProgress)
            )
        });
        let (icon, tone) = if failed {
            (IconName::TriangleAlert, crate::ui::design::rose(cx))
        } else if in_progress {
            (IconName::Loader, crate::ui::design::amber(cx))
        } else {
            (IconName::Check, crate::ui::design::t3(cx))
        };
        let summary = activity_step_summary(total, in_progress);

        v_flex()
            .w_full()
            .mt(px(-2.))
            .gap_0p5()
            .child(
                h_flex()
                    .id((
                        "agent-chat-work-log-group",
                        (agent_id.as_u128() as u64).wrapping_add(index as u64),
                    ))
                    .w_full()
                    .min_w(px(0.))
                    .gap_1p5()
                    .items_center()
                    .px_1()
                    .py(px(1.))
                    .rounded(crate::ui::design::r_sm())
                    .cursor_pointer()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.36)))
                    .child(
                        gpui_component::Icon::new(if expanded {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(crate::ui::design::icon_sm()),
                    )
                    .child(
                        gpui_component::Icon::new(icon)
                            .size(crate::ui::design::icon_sm())
                            .text_color(tone),
                    )
                    .child(div().font_weight(gpui::FontWeight::MEDIUM).child(summary))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let key = (agent_id, index);
                        if !this.agent_chat_expanded_work_log_groups.remove(&key) {
                            this.agent_chat_expanded_work_log_groups.insert(key);
                        }
                        this.remeasure_agent_chat_list(agent_id);
                        cx.notify();
                    })),
            )
            .when(expanded, |group| {
                group.child(
                    v_flex()
                        .ml_4()
                        .gap_0p5()
                        .children(activity.iter().enumerate().filter_map(
                            |(offset, item)| match item {
                                AgentChatTimelineItem::WorkLog(entry)
                                    if !is_noise_work_log(entry) =>
                                {
                                    Some(self.render_agent_work_log_entry(
                                        agent_id,
                                        index + offset,
                                        entry,
                                        cx,
                                    ))
                                }
                                AgentChatTimelineItem::FileChangeActivity(file_change)
                                    if !file_change.observed
                                        && !file_change.file.clears_projection
                                        && !artifact_filter.is_artifact(&file_change.file.path) =>
                                {
                                    Some(self.render_file_change_activity(file_change, cx))
                                }
                                _ => None,
                            },
                        )),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_agent_work_log_entry(
        &self,
        agent_id: Uuid,
        index: usize,
        entry: &crate::state::agent_chat::WorkLogEntry,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let is_stop = is_agent_stop_work_log(entry);
        let icon = if is_stop {
            IconName::CircleX
        } else {
            match entry.status {
                crate::state::agent_chat::WorkLogStatus::Failed => IconName::TriangleAlert,
                crate::state::agent_chat::WorkLogStatus::Completed => IconName::Check,
                crate::state::agent_chat::WorkLogStatus::Pending
                | crate::state::agent_chat::WorkLogStatus::InProgress => IconName::Loader,
            }
        };
        let tone = if is_stop {
            crate::ui::design::amber(cx)
        } else {
            match entry.status {
                crate::state::agent_chat::WorkLogStatus::Failed => crate::ui::design::rose(cx),
                crate::state::agent_chat::WorkLogStatus::Pending
                | crate::state::agent_chat::WorkLogStatus::InProgress => {
                    crate::ui::design::amber(cx)
                }
                crate::state::agent_chat::WorkLogStatus::Completed => crate::ui::design::t3(cx),
            }
        };
        let detail = work_log_display_detail(entry);
        let expandable = detail
            .as_deref()
            .is_some_and(|detail| work_log_text_needs_collapse(detail));
        let expanded = self
            .agent_chat_expanded_work_log_entries
            .contains(&(agent_id, index));
        let title = compact_work_log_title(entry);

        v_flex()
            .w_full()
            .min_w(px(0.))
            .mt(px(-2.))
            .gap_0p5()
            .child(
                h_flex()
                    .id(("agent-chat-work-log-entry", index))
                    .w_full()
                    .min_w(px(0.))
                    .gap_1p5()
                    .items_center()
                    .px_1()
                    .py(px(1.))
                    .rounded(crate::ui::design::r_sm())
                    .when(is_stop, |row| {
                        row.border_1()
                            .border_color(crate::ui::design::amber(cx).opacity(0.18))
                            .bg(crate::ui::design::amber(cx).opacity(0.07))
                            .px_2()
                            .py_1()
                    })
                    .when(expandable, |row| {
                        row.cursor_pointer()
                            .hover(|row| row.bg(crate::ui::design::hover(cx).opacity(0.36)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let key = (agent_id, index);
                                if !this.agent_chat_expanded_work_log_entries.remove(&key) {
                                    this.agent_chat_expanded_work_log_entries.insert(key);
                                }
                                this.remeasure_agent_chat_list(agent_id);
                                cx.notify();
                            }))
                    })
                    .text_size(crate::ui::design::text_ui())
                    .text_color(crate::ui::design::t3(cx))
                    .when(expandable, |row| {
                        row.child(
                            gpui_component::Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(crate::ui::design::icon_sm()),
                        )
                    })
                    .child(
                        gpui_component::Icon::new(icon)
                            .size(crate::ui::design::icon_sm())
                            .text_color(tone),
                    )
                    .child(
                        div()
                            .max_w(px(780.))
                            .min_w(px(0.))
                            .truncate()
                            .when(is_stop, |label| {
                                label
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(crate::ui::design::t2(cx))
                            })
                            .child(title),
                    )
                    .when(expandable && !expanded, |row| {
                        row.child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .text_color(crate::ui::design::t3(cx).opacity(0.62))
                                .child("Show command"),
                        )
                    }),
            )
            .when(expandable && expanded, |block| {
                let detail = detail.unwrap_or_default();
                block.child(
                    div()
                        .ml_5()
                        .max_h(px(260.))
                        .overflow_y_scrollbar()
                        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                        .rounded(px(crate::ui::style::RADIUS))
                        .border_1()
                        .border_color(crate::ui::design::line(cx))
                        .bg(crate::ui::style::surface(cx))
                        .px_2()
                        .py_2()
                        .font_family(crate::ui::design::FONT_MONO)
                        .text_size(crate::ui::design::text_ui())
                        .line_height(gpui::relative(1.35))
                        .text_color(crate::ui::design::t3(cx))
                        .children(detail.lines().map(|line| div().child(line.to_string()))),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_agent_activity_indicator(
        &self,
        session: &crate::state::agent_chat::AgentChatSession,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let now = unix_now_secs();
        let running_for = session
            .started_running_at
            .map(|started| now.saturating_sub(started))
            .unwrap_or(0);
        let quiet_for = now.saturating_sub(session.last_activity_at);
        let displayed_running_for = running_for.max(quiet_for);
        let activity = agent_activity_kind(session);
        let spinner = logo_spinner(
            20.,
            "agent-chat-activity-logo",
            session.agent_id.as_u128() as usize,
            crate::ui::design::t3(cx),
        );
        let activity_icon = if let Some(center_icon) = activity.center_icon() {
            div()
                .relative()
                .size(px(20.))
                .flex_none()
                .child(spinner)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(crate::ui::design::indicator::lucide_icon(
                            center_icon,
                            crate::ui::design::accent(cx),
                            crate::ui::design::icon_sm(),
                        )),
                )
                .into_any_element()
        } else {
            spinner
        };

        h_flex()
            .w_full()
            .max_w(px(860.))
            .min_h(px(34.))
            .items_center()
            .gap_2()
            .px_1()
            .py_1()
            .child(activity_icon)
            .child(
                v_flex().flex_1().min_w(px(0.)).child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::style::focus_text(cx))
                        .child(activity.display_label(displayed_running_for)),
                ),
            )
            .into_any_element()
    }
}

fn activity_step_count(
    activity: &[AgentChatTimelineItem],
    artifact_filter: &VisualizationArtifactFilter,
) -> usize {
    activity
        .iter()
        .map(|item| match item {
            AgentChatTimelineItem::WorkLog(entry) if !is_noise_work_log(entry) => {
                entry.count.max(1)
            }
            AgentChatTimelineItem::FileChangeActivity(file_change)
                if !file_change.observed
                    && !file_change.file.clears_projection
                    && !artifact_filter.is_artifact(&file_change.file.path) =>
            {
                1
            }
            _ => 0,
        })
        .sum()
}

fn activity_step_summary(total: usize, in_progress: bool) -> String {
    match (in_progress, total) {
        (true, 1) => "Working on 1 step".to_string(),
        (true, count) => format!("Working on {count} steps"),
        (false, 1) => "Worked on 1 step".to_string(),
        (false, count) => format!("Worked on {count} steps"),
    }
}

#[cfg(test)]
mod activity_group_tests {
    use super::*;

    #[test]
    fn compaction_indicator_overrides_activity_then_restores_it() {
        let agent = AgentRecord::new(
            ide_core::ProjectId(Uuid::nil()),
            std::path::PathBuf::from("/tmp/choro-compaction-test"),
            "Chat",
            "",
            AgentKind::Codex,
            AgentModel::default_for(AgentKind::Codex),
            AgentEffort::default(),
            AgentAccessMode::default(),
        );
        let mut session = CenterArea::transient_hydration_session(&agent);
        session.set_status(AgentChatStatus::Running);
        for (mode, text, label) in [
            (AgentInteractionMode::Default, "hello", "Working"),
            (AgentInteractionMode::Plan, "plan", "Planning"),
            (
                AgentInteractionMode::Default,
                AGENT_CODE_REVIEW_PROMPT,
                "Reviewing",
            ),
            (
                AgentInteractionMode::Default,
                AGENT_VERIFY_REQUEST_MARKER,
                "Verifying",
            ),
        ] {
            session.interaction_mode = mode;
            session.timeline = vec![AgentChatTimelineItem::Message(AgentChatMessage::User {
                text: text.to_string(),
                display_text: None,
                tags: Vec::new(),
                created_at: 0,
            })];
            session.is_compacting = true;
            assert_eq!(
                agent_activity_kind(&session).display_label(900),
                "Compacting context…"
            );
            session.is_compacting = false;
            assert_eq!(agent_activity_kind(&session).label(), label);
        }
    }

    #[test]
    fn activity_summary_uses_clear_singular_and_progress_copy() {
        assert_eq!(activity_step_summary(1, false), "Worked on 1 step");
        assert_eq!(activity_step_summary(3, false), "Worked on 3 steps");
        assert_eq!(activity_step_summary(2, true), "Working on 2 steps");
    }

    #[test]
    fn activity_count_excludes_reasoning_and_counts_commands_and_file_edits() {
        let activity = vec![
            AgentChatTimelineItem::WorkLog(crate::state::agent_chat::WorkLogEntry::new(
                "reasoning",
                "reasoning",
                WorkLogEntryKind::Step,
                "Reasoning",
                WorkLogStatus::Completed,
            )),
            AgentChatTimelineItem::WorkLog(crate::state::agent_chat::WorkLogEntry::new(
                "command",
                "command",
                WorkLogEntryKind::Command,
                "cargo test",
                WorkLogStatus::Completed,
            )),
            AgentChatTimelineItem::FileChangeActivity(
                crate::state::agent_chat::FileChangeActivity::new(
                    "edit:src/main.rs",
                    "turn-a",
                    crate::state::agent_chat::FileChangeStat::new("src/main.rs", 2, 1),
                    false,
                    1,
                ),
            ),
        ];
        let filter = VisualizationArtifactFilter::new(Uuid::nil(), Path::new("/tmp/project"));

        assert_eq!(activity_step_count(&activity, &filter), 2);
    }
}
