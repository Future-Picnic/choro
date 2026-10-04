//! Delegated Expert rows beneath a lead in the sidebar.
//!
//! Children never appear in the ordinary agent lists; they live only under
//! their lead, behind a chevron the user opens. Disclosure changes nothing but
//! this view's own fold state, and a click on a row is the one way to open it.
use super::*;
use crate::state::delegation::display::DelegatedTaskRow;

/// What an agent row shows in its trailing status slot. Review outranks every
/// other state: while a reviewer process holds the conversation the row is
/// teal, even for a lead whose earlier Bandmate assignments keep the toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowIndicator {
    Review,
    Bandmates,
    Working,
    Chevron,
    Empty,
}

pub(super) fn agent_row_indicator(
    reviewing: bool,
    has_delegations: bool,
    working: bool,
    delegation_working: bool,
    hovered: bool,
) -> RowIndicator {
    match (hovered, has_delegations) {
        (true, true) => RowIndicator::Chevron,
        (true, false) => RowIndicator::Empty,
        _ if reviewing => RowIndicator::Review,
        (false, true) if working && delegation_working => RowIndicator::Bandmates,
        _ if working => RowIndicator::Working,
        (false, true) => RowIndicator::Chevron,
        (false, false) => RowIndicator::Empty,
    }
}

/// The collapsed project's single indicator. Attention first, then review,
/// so a parent under review stays visibly teal beside busy Bandmates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProjectIndicator {
    Waiting,
    Review,
    Bandmates,
    Working,
    Empty,
}

pub(super) fn project_indicator(
    waiting: bool,
    working: bool,
    reviewing: bool,
    delegating: bool,
    collapsed: bool,
) -> ProjectIndicator {
    if waiting {
        ProjectIndicator::Waiting
    } else if !(working && collapsed) {
        ProjectIndicator::Empty
    } else if reviewing {
        ProjectIndicator::Review
    } else if delegating {
        ProjectIndicator::Bandmates
    } else {
        ProjectIndicator::Working
    }
}

#[cfg(test)]
mod indicator_tests {
    use super::*;

    #[test]
    fn a_lead_with_past_bandmate_assignments_shows_teal_while_reviewed() {
        // Review makes the row Working; historical delegations keep the toggle.
        assert_eq!(
            agent_row_indicator(true, true, true, false, false),
            RowIndicator::Review
        );
        // Even a stale "delegation working" flag cannot outrank the review.
        assert_eq!(
            agent_row_indicator(true, true, true, true, false),
            RowIndicator::Review
        );
        assert_eq!(
            agent_row_indicator(true, false, true, false, false),
            RowIndicator::Review
        );
        // Hover keeps the disclosure usable; the review returns on mouse-out.
        assert_eq!(
            agent_row_indicator(true, true, true, false, true),
            RowIndicator::Chevron
        );
        // Without a review the existing treatments are unchanged.
        assert_eq!(
            agent_row_indicator(false, true, true, true, false),
            RowIndicator::Bandmates
        );
        assert_eq!(
            agent_row_indicator(false, true, true, false, false),
            RowIndicator::Working
        );
        assert_eq!(
            agent_row_indicator(false, true, false, false, false),
            RowIndicator::Chevron
        );
        assert_eq!(
            agent_row_indicator(false, false, true, false, false),
            RowIndicator::Working
        );
        assert_eq!(
            agent_row_indicator(false, false, false, false, false),
            RowIndicator::Empty
        );
    }

    #[test]
    fn collapsed_project_keeps_review_teal_beside_busy_bandmates() {
        assert_eq!(
            project_indicator(false, true, true, true, true),
            ProjectIndicator::Review
        );
        assert_eq!(
            project_indicator(true, true, true, true, true),
            ProjectIndicator::Waiting,
            "a person waiting still comes first"
        );
        assert_eq!(
            project_indicator(false, true, false, true, true),
            ProjectIndicator::Bandmates
        );
        assert_eq!(
            project_indicator(false, true, true, false, false),
            ProjectIndicator::Empty,
            "expanded projects show status on their rows"
        );
    }
}

impl ProjectList {
    /// The teal "Reviewing code" spinner. It paints in the shared activity
    /// layer, so progress packets never rebuild the row; the tooltip names
    /// the state for anyone who can't rely on colour.
    pub(super) fn review_indicator(
        &self,
        id: impl Into<gpui::ElementId>,
        cx: &App,
    ) -> gpui::AnyElement {
        div()
            .id(id)
            .flex_none()
            .size(px(16.))
            .tooltip(|window, cx| Tooltip::new("Reviewing code").build(window, cx))
            .child(
                self.activity_anchors
                    .placeholder(16., Some(crate::ui::design::teal(cx))),
            )
            .into_any_element()
    }

    /// The trailing slot of an agent row (project and All agents layouts).
    pub(super) fn render_agent_row_indicator(
        &self,
        agent: &SidebarAgent,
        working: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let activity = self.delegation_activity_for(agent.id, cx);
        let indicator = agent_row_indicator(
            agent.reviewing,
            agent.has_delegations,
            working,
            activity == DelegationActivity::Working,
            hovered,
        );
        if agent.has_delegations {
            return Some(self.render_delegation_toggle(agent.id, indicator, cx));
        }
        let slot = |child| div().flex_none().w(px(34.)).flex().justify_end().child(child);
        match indicator {
            RowIndicator::Review => Some(
                slot(self.review_indicator(
                    ("agent-reviewing", agent.id.as_u128() as u64),
                    cx,
                ))
                .into_any_element(),
            ),
            RowIndicator::Working => {
                Some(slot(self.activity_anchors.placeholder(16., None)).into_any_element())
            }
            _ => None,
        }
    }

    pub(super) fn delegation_activity_for(&self, agent_id: Uuid, cx: &App) -> DelegationActivity {
        self.model
            .read(cx)
            .agents
            .get(&agent_id)
            .map(|agent| agent.delegation)
            .unwrap_or_default()
    }

    /// A lead's Bandmate disclosure. Its glyph carries the row's status, so a
    /// lead under review shows the teal spinner here and keeps the toggle.
    pub(super) fn render_delegation_toggle(
        &self,
        agent_id: Uuid,
        indicator: RowIndicator,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let expanded = self.expanded_delegations.contains(&agent_id);
        let glyph = match indicator {
            RowIndicator::Review => self
                .activity_anchors
                .placeholder(16., Some(crate::ui::design::teal(cx))),
            RowIndicator::Bandmates => self
                .activity_anchors
                .placeholder(16., Some(crate::ui::design::amber(cx))),
            RowIndicator::Working => self.activity_anchors.placeholder(16., None),
            RowIndicator::Chevron | RowIndicator::Empty => {
                crate::ui::design::indicator::lucide_icon(
                    if expanded {
                        lucide_icons::Icon::ChevronDown
                    } else {
                        lucide_icons::Icon::ChevronRight
                    },
                    crate::ui::design::t3(cx),
                    crate::ui::design::icon_sm(),
                )
                .into_any_element()
            }
        };
        let reviewing = indicator == RowIndicator::Review;
        style::sidebar_agent_status_button(
            ("sidebar-delegation-toggle", agent_id.as_u128() as u64),
            glyph,
            cx,
        )
        .tooltip(match (reviewing, expanded) {
            (true, true) => "Reviewing code. Hide bandmates",
            (true, false) => "Reviewing code. Show bandmates",
            (false, true) => "Hide bandmates",
            (false, false) => "Show bandmates",
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            if !this.expanded_delegations.remove(&agent_id) {
                this.expanded_delegations.insert(agent_id);
            }
            let expanded = this.expanded_delegations.clone();
            this.model
                .update(cx, |model, cx| model.set_expanded(expanded, cx));
            cx.notify();
        }))
        .into_any_element()
    }

    pub(super) fn render_delegated_task_row(
        &self,
        project: ProjectId,
        parent: Uuid,
        task: &DelegatedTaskRow,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let child = task.child_agent_id;
        let amber = crate::ui::design::amber(cx);
        let icon_color = match task.activity {
            DelegationActivity::Working | DelegationActivity::Attention => amber,
            DelegationActivity::Paused | DelegationActivity::Idle => crate::ui::design::t4(cx),
        };
        let attention = task.activity == DelegationActivity::Attention;
        let title = SharedString::from(task.label.clone());
        let status = match task.activity {
            DelegationActivity::Attention => "Needs your input",
            DelegationActivity::Paused => "Paused",
            _ => task.status.label(),
        };
        let tooltip = SharedString::from(format!("{} · {status}\n{}", task.label, task.goal));
        let content = h_flex()
            .w_full()
            .min_w(px(0.))
            .gap_2()
            .items_center()
            .line_height(px(18.))
            .child(
                h_flex()
                    .flex_none()
                    .w(crate::ui::design::icon_sm())
                    .justify_center()
                    .child(crate::ui::design::indicator::bandmate_icon(
                        task.bandmate_index,
                        icon_color,
                        crate::ui::design::icon_sm(),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .truncate()
                    .text_size(crate::ui::design::text_ui())
                    .text_color(if attention {
                        amber
                    } else {
                        crate::ui::design::t2(cx)
                    })
                    .child(title),
            )
            .child(
                div()
                    .flex_none()
                    .max_w(gpui::relative(0.45))
                    .truncate()
                    .text_size(crate::ui::design::text_label())
                    .text_color(if attention {
                        amber
                    } else {
                        crate::ui::design::t3(cx)
                    })
                    .child(status),
            )
            .into_any_element();
        style::sidebar_expert_row_button(
            ("sidebar-delegated-task", task.task_id.as_u128() as u64),
            content,
            cx,
        )
        .tooltip(tooltip)
        .on_click(cx.listener(move |this, _, window, cx| {
            cx.stop_propagation();
            this.open_delegated_task(project, parent, child, window, cx);
        }))
        .into_any_element()
    }

    /// Open the lead, then the assignment's conversation in the Expert panel.
    /// An assignment that has not started yet only opens its lead.
    fn open_delegated_task(
        &mut self,
        project: ProjectId,
        parent: Uuid,
        child: Option<Uuid>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_agent(project, parent, window, cx);
        let Some(child) = child else {
            return;
        };
        if let Some(center) = self.center.as_ref().and_then(WeakEntity::upgrade) {
            center.update(cx, |center, cx| {
                center.open_delegated_task(child, window, cx);
            });
        }
    }
}
