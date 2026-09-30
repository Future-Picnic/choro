//! Delegated Expert rows beneath a lead in the sidebar.
//!
//! Children never appear in the ordinary agent lists; they live only under
//! their lead, behind a chevron the user opens. Disclosure changes nothing but
//! this view's own fold state, and a click on a row is the one way to open it.
use super::*;
use crate::state::delegation::display::{self, DelegatedTaskRow, ParentDelegationState};
use crate::state::delegation::DelegationHandle;

impl ProjectList {
    /// The lead's delegated work, from the application-owned coordinator.
    pub(super) fn delegation_state_for(&self, agent_id: Uuid, cx: &App) -> ParentDelegationState {
        let Some(handle) = cx.try_global::<DelegationHandle>() else {
            return ParentDelegationState::default();
        };
        let chats = self.agent_chats.read(cx);
        let needs_user = |child: Uuid| {
            chats.session(child).is_some_and(|session| {
                session.pending_approval.is_some()
                    || session.pending_user_input.is_some()
                    || session.status == AgentChatStatus::PlanReady
            })
        };
        let runs = &handle.0.read(cx).runs;
        let mut state = display::parent_delegation_state(runs, agent_id, &needs_user);
        // Keep finished conversations discoverable beneath their lead, without
        // reviving the activity, working count, or live-run state.
        if !state.has_live_run() {
            state.tasks = display::assignment_overview(runs, agent_id, &needs_user)
                .into_iter()
                .map(|entry| DelegatedTaskRow {
                    run_id: entry.run_id,
                    task_id: entry.task_id,
                    bandmate_index: entry.bandmate_index,
                    child_agent_id: entry.child_agent_id,
                    expert: entry.expert,
                    label: entry.label,
                    goal: entry.goal,
                    status: entry.status,
                    activity: entry.activity,
                })
                .collect();
        }
        state
    }

    pub(super) fn render_delegation_toggle(
        &self,
        agent_id: Uuid,
        state: &ParentDelegationState,
        working: bool,
        hovered: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let expanded = self.expanded_delegations.contains(&agent_id);
        let glyph = if working && !hovered {
            if state.activity == DelegationActivity::Working {
                delegation_spinner(
                    16.,
                    "sidebar-lead-experts-spinner",
                    agent_id.as_u128() as usize,
                    crate::ui::design::amber(cx),
                )
            } else {
                logo_spinner(
                    16.,
                    "sidebar-lead-spinner",
                    agent_id.as_u128() as usize,
                    crate::ui::design::t3(cx),
                )
            }
        } else {
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
        };
        style::sidebar_agent_status_button(
            ("sidebar-delegation-toggle", agent_id.as_u128() as u64),
            glyph,
            cx,
        )
        .tooltip(if expanded {
            "Hide bandmates"
        } else {
            "Show bandmates"
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            if !this.expanded_delegations.remove(&agent_id) {
                this.expanded_delegations.insert(agent_id);
            }
            cx.notify();
        }))
        .into_any_element()
    }

    pub(super) fn render_delegated_task_rows(
        &self,
        project: ProjectId,
        parent: Uuid,
        indent: gpui::Pixels,
        state: &ParentDelegationState,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        v_flex()
            .w_full()
            .min_w(px(0.))
            .pl(indent)
            .pr_1()
            .py_1()
            .children(
                state
                    .tasks
                    .iter()
                    .map(|task| self.render_delegated_task_row(project, parent, task, cx)),
            )
            .into_any_element()
    }

    fn render_delegated_task_row(
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
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_delegated_task(child, window, cx);
            });
        }
    }
}
