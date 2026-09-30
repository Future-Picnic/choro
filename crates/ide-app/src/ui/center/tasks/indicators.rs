use super::*;

impl CenterArea {
    /// Resolve the agent implementing `reference` and (best-effort) its ship PR,
    /// keeping the PR state synced. Shared by the board-row indicators and the
    /// Implement control.
    pub(in crate::ui::center) fn linked_agent_and_pr(
        &mut self,
        project: ProjectId,
        reference: &TaskRef,
        cx: &mut Context<Self>,
    ) -> (
        Option<AgentRecord>,
        Option<crate::ui::git::git_panel::BranchPullRequest>,
    ) {
        let Some(agent) = self
            .agents
            .read(cx)
            .active_task_implementor(project, reference)
        else {
            return (None, None);
        };
        if !self.agent_ship_pr_targets.contains_key(&agent.id) {
            if let (Some(repo_path), Some(branch)) = (
                agent.ship_pr_repo_path.clone(),
                agent.ship_pr_branch.clone(),
            ) {
                self.track_agent_ship_pr_branch(agent.id, repo_path, branch, cx);
            }
        }
        self.sync_agent_ship_pull_request(agent.id, cx);
        let pr = self.agent_ship_prs.get(&agent.id).cloned();
        (Some(agent), pr)
    }

    /// Compact PR + linked-agent indicators shown on dense board rows.
    pub(in crate::ui::center) fn render_task_indicators(
        &mut self,
        project: ProjectId,
        reference: &TaskRef,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let implementors = self.agents.read(cx).task_implementors(project, reference);
        let (agent, pr) = self.linked_agent_and_pr(project, reference, cx);
        let Some(agent) = agent else {
            return Vec::new();
        };

        let mut pills: Vec<gpui::AnyElement> = Vec::new();

        if let Some(pr) = pr {
            let (status, accent) = crate::ui::git::git_panel::pull_request_status_style(&pr, cx);
            let url = pr.url.clone();
            let number = pr.number;
            let tip = SharedString::from(format!("PR #{number} · {status} — open"));
            pills.push(
                h_flex()
                    .id(("task-ind-pr", agent.id.as_u128() as u64))
                    .h(crate::ui::design::control_h())
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .rounded(crate::ui::design::r_sm())
                    .text_color(accent)
                    .text_size(crate::ui::design::text_ui())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .hover(move |pill| pill.bg(accent.opacity(0.14)))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(move |_, _, _| crate::ui::git::git_panel::open_url(&url))
                    .child(crate::ui::branch_icon::pr_icon(accent))
                    .child(div().child(format!("#{number}")))
                    .into_any_element(),
            );
        }

        let agent_id = agent.id;
        let (agent_accent, agent_state): (gpui::Hsla, &str) = match agent.status {
            AgentStatus::InProgress => (crate::ui::design::amber(cx), "running"),
            AgentStatus::Done => (crate::ui::design::sage(cx), "done"),
            _ => (crate::ui::design::t3(cx), "linked"),
        };
        let tip = SharedString::from(format!("Agent · {agent_state} — open"));
        if implementors.len() == 1 {
            pills.push(
                h_flex()
                    .id(("task-ind-agent", agent_id.as_u128() as u64))
                    .h(crate::ui::design::control_h())
                    .w(px(28.))
                    .items_center()
                    .justify_center()
                    .rounded(crate::ui::design::r_sm())
                    .text_color(agent_accent)
                    .cursor_pointer()
                    .hover(move |pill| pill.bg(agent_accent.opacity(0.14)))
                    .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_agent(agent_id, window, cx);
                    }))
                    .child(Icon::new(IconName::Bot).size(crate::ui::design::icon_md()))
                    .into_any_element(),
            );
        } else {
            let center = cx.entity().clone();
            let history = implementors;
            pills.push(
                style::header_dropdown_button(
                    ("task-ind-agent-history", agent_id.as_u128() as u64),
                    cx,
                )
                .w(px(38.))
                .p_0()
                .text_color(agent_accent)
                .child(Icon::new(IconName::Bot).size(crate::ui::design::icon_md()))
                .tooltip("Implementation agents")
                .dropdown_menu(move |mut menu, window, _| {
                    for (index, implementation) in history.iter().cloned().enumerate() {
                        let implementation_id = implementation.id;
                        menu = menu.item(
                            PopupMenuItem::element(move |_, cx| {
                                task_implementation_agent_history_row(
                                    &implementation,
                                    index == 0,
                                    cx,
                                )
                            })
                            .checked(index == 0)
                            .on_click(window.listener_for(
                                &center,
                                move |this: &mut Self, _, window, cx| {
                                    this.open_agent(implementation_id, window, cx);
                                },
                            )),
                        );
                    }
                    menu
                })
                .into_any_element(),
            );
        }

        pills
    }

    /// Keep the implementation action in one stable header position. Once work
    /// has started it creates a new linked agent without replacing prior runs.
    pub(in crate::ui::center) fn render_task_implement_action(
        &mut self,
        project: ProjectId,
        summary: &TaskSummary,
        detail: Option<&TaskDetail>,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let id_seed = task_element_id(&summary.reference);
        let has_implementor = self
            .agents
            .read(cx)
            .active_task_implementor(project, &summary.reference)
            .is_some();

        let summary = summary.clone();
        let detail = detail.cloned();
        Some(
            div()
                .relative()
                .flex_none()
                .child(
                    style::implement_button(
                        ("task-implement", id_seed),
                        if has_implementor {
                            "Reimplement"
                        } else {
                            "Implement"
                        },
                        cx,
                    )
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_implementation_agent_for_task(
                            project,
                            summary.clone(),
                            detail.clone(),
                            window,
                            cx,
                        );
                    })),
                )
                .child(crate::ui::onboarding::target_marker(
                    crate::ui::onboarding::SpotlightTarget::TaskImplement,
                    cx,
                ))
                .into_any_element(),
        )
    }
}

pub(super) fn task_implementation_agent_history_row(
    agent: &AgentRecord,
    latest: bool,
    cx: &App,
) -> gpui::AnyElement {
    let color = crate::ui::agent_status_style::implement_status_color(agent.status, cx);
    let when = super::super::time::branch_relative_time(agent.created_at as i64);
    let meta = if when.is_empty() {
        format!("{} · {}", agent.model_short_label(), agent.status.label())
    } else {
        format!(
            "{} · {} · {when}",
            agent.model_short_label(),
            agent.status.label()
        )
    };
    h_flex()
        .w(px(280.))
        .min_w(px(0.))
        .items_center()
        .gap_2()
        .child(
            Icon::new(IconName::Bot)
                .size(crate::ui::design::icon_md())
                .text_color(color),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .gap(px(1.))
                .child(
                    div()
                        .truncate()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t1(cx))
                        .child(agent.title.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(meta),
                ),
        )
        .when(latest, |row| {
            row.child(
                div()
                    .flex_none()
                    .text_size(crate::ui::design::text_label())
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(crate::ui::design::accent(cx))
                    .child("Latest"),
            )
        })
        .into_any_element()
}
