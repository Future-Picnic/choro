use super::*;
use crate::ui::{agent_status_style, design};
use gpui::{canvas, point, AnyTooltip, AnyView, Bounds, MouseMoveEvent, Pixels, Task};
use std::cell::Cell;
use std::rc::Rc;

#[derive(Default)]
pub(super) struct SidebarAgentHover {
    row: Option<HoveredAgentRow>,
    row_bounds: Option<Bounds<Pixels>>,
    visible_row_bounds: Option<Bounds<Pixels>>,
    card_bounds: Option<Bounds<Pixels>>,
    sidebar_bounds: Option<Bounds<Pixels>>,
    view: Option<AnyView>,
    timer: Option<Task<()>>,
    row_rendered: Cell<bool>,
}

impl SidebarAgentHover {
    pub(super) fn begin_render(&self) {
        self.row_rendered.set(false);
    }

    pub(super) fn finish_render(&mut self) {
        if !self.row_rendered.get() {
            self.clear();
        }
    }

    fn clear(&mut self) {
        self.row = None;
        self.row_bounds = None;
        self.visible_row_bounds = None;
        self.card_bounds = None;
        self.view = None;
        self.timer = None;
    }
}

/// Keep a separate, observed view so an open card follows live agent/PR state.
struct AgentHoverCard {
    owner: WeakEntity<ProjectList>,
    agent_id: Uuid,
}

impl Render for AgentHoverCard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.owner
            .update(cx, |list, cx| {
                list.render_agent_hover_card(self.agent_id, window, cx)
            })
            .unwrap_or_else(|_| div().into_any_element())
    }
}

impl ProjectList {
    pub(super) fn agent_sidebar_bounds(&self, cx: &Context<Self>) -> impl IntoElement {
        let owner = cx.entity().downgrade();
        canvas(
            move |bounds, _, cx| {
                let _ = owner.update(cx, |list, cx| {
                    if list.agent_hover.sidebar_bounds != Some(bounds) {
                        list.agent_hover.sidebar_bounds = Some(bounds);
                        cx.notify();
                    }
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    pub(super) fn set_agent_card_hover(
        &mut self,
        row: HoveredAgentRow,
        agent_id: Uuid,
        hovered: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !hovered {
            if self.agent_hover.row == Some(row) && self.agent_hover.view.is_none() {
                self.agent_hover.row = None;
                self.agent_hover.timer = None;
            }
            return;
        }
        if self.agent_hover.row == Some(row) && self.agent_hover.view.is_some() {
            self.agent_hover.timer = None;
            return;
        }
        self.agent_hover.row = Some(row);
        self.agent_hover.row_bounds = None;
        self.agent_hover.visible_row_bounds = None;
        self.agent_hover.card_bounds = None;
        self.agent_hover.view = None;
        let owner = cx.entity().downgrade();
        let build = self.agent_hover_card_builder(agent_id, cx);
        self.agent_hover.timer = Some(window.spawn(cx, async move |cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = cx.update(|window, cx| {
                let view = build(window, cx);
                let _ = owner.update(cx, |list, cx| {
                    if list.agent_hover.row == Some(row) {
                        list.agent_hover.view = Some(view);
                        list.agent_hover.timer = None;
                        cx.notify();
                    }
                });
            });
        }));
    }

    /// Submit through GPUI's tooltip layer so the card escapes sidebar clipping.
    /// Its anchor follows the row's painted bounds, never the mouse position.
    pub(super) fn agent_hover_anchor(
        &self,
        row: HoveredAgentRow,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        if self.agent_hover.row == Some(row) {
            self.agent_hover.row_rendered.set(true);
        }
        let owner = cx.entity().downgrade();
        let paint_owner = owner.clone();
        canvas(
            move |bounds, window, cx| {
                let _ = owner.update(cx, |list, cx| {
                    if list.agent_hover.row == Some(row)
                        && list.agent_hover.row_bounds != Some(bounds)
                    {
                        list.agent_hover.row_bounds = Some(bounds);
                        cx.notify();
                    }
                });
                let Some(owner) = owner.upgrade() else { return };
                let state = &owner.read(cx).agent_hover;
                if state.row != Some(row) {
                    return;
                }
                let Some(sidebar) = state.sidebar_bounds else {
                    return;
                };
                let visible_row = bounds.intersect(&window.content_mask().bounds);
                if visible_row.size.height <= px(0.)
                    || visible_row.size.width <= px(0.)
                    || window.viewport_size().width - sidebar.right() < px(32.)
                    || window.viewport_size().height - bounds.top() < px(24.)
                {
                    owner.update(cx, |list, cx| {
                        list.agent_hover.clear();
                        cx.notify();
                    });
                    return;
                }
                let Some(view) = &state.view else { return };
                let weak = owner.downgrade();
                window.set_tooltip(AnyTooltip {
                    view: view.clone(),
                    // GPUI adds one pixel; the card has eight pixels of shadow padding.
                    mouse_position: point(sidebar.right() - px(1.), bounds.top() - px(9.)),
                    check_visible_and_update: Rc::new(move |card, window, cx| {
                        weak.update(cx, |list, cx| {
                            if list.agent_hover.row != Some(row) {
                                return false;
                            }
                            list.agent_hover.visible_row_bounds = Some(visible_row);
                            list.agent_hover.card_bounds = Some(card);
                            list.update_agent_card_visibility(row, window, cx);
                            list.agent_hover.view.is_some()
                        })
                        .unwrap_or(false)
                    }),
                });
            },
            move |_, _, window, cx| {
                let Some(owner) = paint_owner.upgrade() else {
                    return;
                };
                if owner.read(cx).agent_hover.view.is_some()
                    && owner.read(cx).agent_hover.row == Some(row)
                {
                    // Track the exit delay directly. A full-window refresh for
                    // every pointer move also rebuilds unrelated Studio/chat UI.
                    let owner = paint_owner.clone();
                    window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
                        if phase.bubble() {
                            let _ = owner.update(cx, |list, cx| {
                                list.update_agent_card_visibility(row, window, cx);
                            });
                        }
                    });
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    fn update_agent_card_visibility(
        &mut self,
        row: HoveredAgentRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.agent_hover.row != Some(row) || self.agent_hover.view.is_none() {
            return;
        }
        let mouse = window.mouse_position();
        let inside = [
            self.agent_hover.visible_row_bounds,
            self.agent_hover.card_bounds,
        ]
        .into_iter()
        .flatten()
        .any(|bounds| bounds.contains(&mouse));
        if inside {
            self.agent_hover.timer = None;
        } else if self.agent_hover.timer.is_none() {
            let owner = cx.entity().downgrade();
            self.agent_hover.timer = Some(window.spawn(cx, async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(500))
                    .await;
                let _ = owner.update(cx, |list, cx| {
                    if list.agent_hover.row == Some(row) {
                        list.agent_hover.clear();
                        cx.notify();
                    }
                });
            }));
        }
    }

    pub(super) fn agent_hover_card_builder(
        &self,
        agent_id: Uuid,
        cx: &Context<Self>,
    ) -> impl Fn(&mut Window, &mut App) -> gpui::AnyView + 'static {
        let owner = cx.entity().downgrade();
        let center = self.center.clone();
        move |_, cx| {
            cx.new(|cx| {
                if let Some(owner) = owner.upgrade() {
                    cx.observe(&owner, |_, _, cx| cx.notify()).detach();
                }
                if let Some(center) = center.upgrade() {
                    cx.observe(&center, |_, _, cx| cx.notify()).detach();
                }
                AgentHoverCard {
                    owner: owner.clone(),
                    agent_id,
                }
            })
            .into()
        }
    }

    fn render_agent_hover_card(
        &mut self,
        agent_id: Uuid,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return div().into_any_element();
        };
        let project = agent.project_id;
        let Some(project_record) = self
            .workspace
            .read(cx)
            .projects
            .iter()
            .find(|p| p.id == project)
        else {
            return div().into_any_element();
        };
        let show_project = matches!(
            self.agent_hover.row,
            Some(HoveredAgentRow::Pinned(_) | HoveredAgentRow::Attention(_))
        );
        let project_identity = show_project.then(|| {
            h_flex()
                .flex_1()
                .min_w(px(0.))
                .gap_1()
                .child(div().flex_none().child(project_icon_element(
                    &project_record.icon,
                    &project_record.icon_color,
                    project_record.icon_image_path.as_deref(),
                    design::icon_sm(),
                    design::icon_sm(),
                    cx,
                )))
                .child(
                    div()
                        .min_w(px(0.))
                        .truncate()
                        .text_color(design::t2(cx))
                        .child(project_record.name.clone()),
                )
        });
        let runtime = self.runtime_for_agent(project, &agent, cx);
        let session_status = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|s| s.status);
        let reviewing = self.agent_chats.read(cx).review_blocks_writing(agent_id);
        let (activity, accent) = if reviewing {
            ("Reviewing code", design::teal(cx))
        } else if agent.status.is_finished() {
            (
                agent.status.label(),
                agent_status_style::status_accent(agent.status, cx),
            )
        } else {
            match session_status {
                Some(AgentChatStatus::Failed) => ("Failed", design::rose(cx)),
                Some(AgentChatStatus::Cancelling) => ("Stopping", design::amber(cx)),
                Some(AgentChatStatus::PlanReady) => ("Plan ready", design::amber(cx)),
                _ => match runtime {
                    ProjectAgentRuntime::Working => ("Working", design::sage(cx)),
                    ProjectAgentRuntime::Waiting => ("Needs attention", design::amber(cx)),
                    ProjectAgentRuntime::Open => ("Terminal open", design::sky(cx)),
                    ProjectAgentRuntime::Idle => ("Idle", design::t2(cx)),
                    ProjectAgentRuntime::Ended => ("Session ended", design::t2(cx)),
                    ProjectAgentRuntime::NotStarted => ("Not started", design::t2(cx)),
                },
            }
        };
        let links = self
            .center
            .update(cx, |center, cx| center.agent_hover_links(&agent, cx))
            .unwrap_or_default();
        let metadata = format!("{} · {}", agent.provider.label(), agent.model_label());
        // Role survives completion, even if a newer run has no assignments yet.
        // Opening this card only reads the coordinator's existing state.
        let is_lead = cx
            .try_global::<crate::state::delegation::DelegationHandle>()
            .is_some_and(|handle| {
                handle
                    .0
                    .read(cx)
                    .runs
                    .iter()
                    .any(|run| run.parent_agent_id == agent_id && !run.tasks.is_empty())
            });
        let sidebar_right = self
            .agent_hover
            .sidebar_bounds
            .map_or(px(0.), |b| b.right());
        let row_top = self.agent_hover.row_bounds.map_or(px(8.), |b| b.top());
        let width = (window.viewport_size().width - sidebar_right - px(16.))
            .max(px(0.))
            .min(px(344.));
        let height = (window.viewport_size().height - row_top - px(8.)).max(px(0.));
        // The outer padding leaves the shadow room inside GPUI's tooltip bounds
        // and doubles as a forgiving pointer corridor around the card.
        div()
            .p_2()
            .child(
                div()
                    .id("sidebar-agent-hover-card")
                    .w(width)
                    .max_h(height)
                    .overflow_x_hidden()
                    .overflow_y_scroll()
                    .rounded(design::r_lg())
                    .bg(design::surface(cx))
                    .shadow(design::menu_shadow())
                    .text_size(design::text_ui())
                    .text_color(design::t1(cx))
                    .child(
                        v_flex()
                            .p_3()
                            .gap_2()
                            .child(
                                div()
                                    .w_full()
                                    .text_size(design::text_body())
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(agent.title.clone()),
                            )
                            .child(
                                h_flex()
                                    .w_full()
                                    .min_w(px(0.))
                                    .gap_1p5()
                                    .child(div().size(px(6.)).flex_none().rounded_full().bg(accent))
                                    .child(div().flex_none().text_color(accent).child(activity))
                                    .when(!agent.status.is_finished(), |row| {
                                        row.child(
                                            div()
                                                .flex_none()
                                                .text_color(design::t3(cx))
                                                .child(format!("· {}", agent.status.label())),
                                        )
                                    })
                                    .children(project_identity),
                            ),
                    )
                    .when(is_lead || agent.solo_branch.is_some(), |card| {
                        card.child(
                            v_flex()
                                .px_1()
                                .pb_1()
                                .when(is_lead, |column| {
                                    column.child(
                                        h_flex()
                                            .px_2()
                                            .py_1()
                                            .gap_2()
                                            .items_center()
                                            .child(design::indicator::lead_icon(
                                                design::amber(cx),
                                                design::icon_sm(),
                                            ))
                                            .child(div().text_color(design::t2(cx)).child("Lead")),
                                    )
                                })
                                .when_some(agent.solo_branch.clone(), |column, branch| {
                                    column.child(
                                        h_flex()
                                            .px_2()
                                            .py_1()
                                            .gap_2()
                                            .child(design::indicator::solo_icon(
                                                design::sky(cx),
                                                design::icon_sm(),
                                            ))
                                            .child(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .truncate()
                                                    .text_color(design::t2(cx))
                                                    .child(branch),
                                            )
                                            .child(
                                                div()
                                                    .text_size(design::text_label())
                                                    .text_color(design::t3(cx))
                                                    .child(if agent.is_active_solo() {
                                                        "Solo"
                                                    } else {
                                                        "Solo history"
                                                    }),
                                            ),
                                    )
                                }),
                        )
                    })
                    .when(!links.is_empty(), |card| {
                        card.child(
                            v_flex()
                                .id("agent-hover-links")
                                .border_t_1()
                                .border_color(design::line(cx))
                                .p_1()
                                .max_h((window.viewport_size().height * 0.45).min(px(280.)))
                                .overflow_y_scroll()
                                .children(links),
                        )
                    })
                    .child(
                        h_flex()
                            .p_3()
                            .gap_2()
                            .border_t_1()
                            .border_color(design::line(cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(design::text_label())
                                    .text_color(design::t3(cx))
                                    .child(metadata),
                            )
                            .child(
                                style::dialog_neutral_button("hover-open-agent", "Open agent", cx)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        this.open_agent(project, agent_id, window, cx);
                                    })),
                            ),
                    ),
            )
            .into_any_element()
    }
}
