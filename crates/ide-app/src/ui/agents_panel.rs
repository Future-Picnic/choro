use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gpui::{
    div, prelude::FluentBuilder, px, App, AppContext, Context, Entity, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window,
};
use gpui_component::{h_flex, v_flex, Icon, IconName};
use ide_core::{agents, AgentRecord, AgentRuntimeKind, AgentStatus, ProjectId};
use uuid::Uuid;

use crate::notifications;
use crate::state::agent_chat::AgentChatStatus;
use crate::state::{AgentActivityCache, AgentChatState, AgentRecords, TerminalManager, Workspace};
use crate::ui::agent_status_style::{status_accent, status_icon};
use crate::ui::center::CenterArea;
use crate::ui::logo_spinner::logo_spinner;

const REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const LANE_PREVIEW_LIMIT: usize = 5;

fn compact_relative_time(updated_at: SystemTime) -> SharedString {
    let secs = SystemTime::now()
        .duration_since(updated_at)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    match secs {
        0..=59 => "now".into(),
        60..=3599 => format!("{}m", secs / 60).into(),
        3600..=86_399 => format!("{}h", secs / 3600).into(),
        86_400..=2_591_999 => format!("{}d", secs / 86_400).into(),
        _ => format!("{}mo", secs / 2_592_000).into(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentRunStatus {
    NotStarted,
    Working,
    Waiting,
    Open,
    Idle,
    Ended,
}

/// Right panel tab: app-owned agent lanes for the active project.
pub struct AgentsPanel {
    workspace: Entity<Workspace>,
    terminals: Entity<TerminalManager>,
    agent_chats: Entity<AgentChatState>,
    agents: Entity<AgentRecords>,
    agent_activity: Entity<AgentActivityCache>,
    center: WeakEntity<CenterArea>,
    chats_project: Option<ProjectId>,
    collapsed: HashSet<AgentStatus>,
    expanded_lanes: HashSet<AgentStatus>,
    waiting_notifications: HashSet<String>,
}

impl AgentsPanel {
    pub fn view(
        workspace: Entity<Workspace>,
        terminals: Entity<TerminalManager>,
        agent_chats: Entity<AgentChatState>,
        agents: Entity<AgentRecords>,
        agent_activity: Entity<AgentActivityCache>,
        center: WeakEntity<CenterArea>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&workspace, |_, _, cx| cx.notify()).detach();
            cx.observe(&terminals, |_, _, cx| cx.notify()).detach();
            cx.observe(&agent_chats, |_, _, cx| cx.notify()).detach();
            cx.observe(&agents, |_, _, cx| cx.notify()).detach();
            cx.observe(&agent_activity, |_, _, cx| cx.notify()).detach();

            // Poll CLI transcript stores only to adopt session ids and keep
            // runtime attention markers fresh for app-owned agents.
            cx.spawn(async move |this, cx| loop {
                let Some(panel) = this.upgrade() else {
                    break;
                };
                let poll = panel
                    .update(cx, |this: &mut Self, cx| {
                        this.workspace.read(cx).active_project().map(|p| {
                            let mut paths: Vec<PathBuf> = this
                                .agents
                                .read(cx)
                                .records_for_project(p.id)
                                .iter()
                                .filter(|agent| agent.runtime == AgentRuntimeKind::Terminal)
                                .map(|agent| agent.runtime_path().to_path_buf())
                                .collect();
                            paths.push(p.path.clone());
                            paths.sort();
                            paths.dedup();
                            (p.id, p.name.clone(), paths)
                        })
                    })
                    .ok()
                    .flatten();
                if let Some((project, project_name, paths)) = poll {
                    let chats_by_cwd: HashMap<PathBuf, Vec<agents::AgentChat>> = cx
                        .background_executor()
                        .spawn(async move {
                            paths
                                .into_iter()
                                .map(|path| {
                                    let chats = agents::list_chats(&path);
                                    (path, chats)
                                })
                                .collect()
                        })
                        .await;
                    panel
                        .update(cx, |this: &mut Self, cx| {
                            let adopted = this.terminals.update(cx, |manager, cx| {
                                manager.adopt_agent_ids(&chats_by_cwd, cx)
                            });
                            if !adopted.is_empty() {
                                this.agents.update(cx, |agents, cx| {
                                    for (agent_id, session_id) in adopted {
                                        agents.set_cli_session_id(agent_id, session_id, cx);
                                    }
                                });
                            }
                            this.terminals.update(cx, |manager, cx| {
                                if manager.sessions.iter().any(|s| s.agent_record_id.is_some()) {
                                    cx.notify();
                                }
                            });
                            this.chats_project = Some(project);
                            this.notify_waiting_agents(project, &project_name, cx);
                            cx.notify();
                        })
                        .ok();
                }
                cx.background_executor().timer(REFRESH_INTERVAL).await;
            })
            .detach();

            Self {
                workspace,
                terminals,
                agent_chats,
                agents,
                agent_activity,
                center,
                chats_project: None,
                collapsed: [AgentStatus::Done, AgentStatus::Rejected]
                    .into_iter()
                    .collect(),
                expanded_lanes: HashSet::new(),
                waiting_notifications: HashSet::new(),
            }
        })
    }

    fn notify_waiting_agents(
        &mut self,
        project: ProjectId,
        project_name: &str,
        cx: &mut Context<Self>,
    ) {
        let records = self.agents.read(cx).records_for_project(project);
        let waiting = {
            let manager = self.terminals.read(cx);
            let chats = self.agent_chats.read(cx);
            let mut waiting = Vec::new();
            for agent in records {
                if agent.runtime == AgentRuntimeKind::Chat {
                    let (status, session_id, last_activity_at) = chats
                        .session(agent.id)
                        .map(|session| {
                            (
                                Some(session.status),
                                session
                                    .chat_session_id
                                    .clone()
                                    .or_else(|| session.cli_session_id.clone()),
                                Some(session.last_activity_at),
                            )
                        })
                        .unwrap_or((None, None, None));
                    if matches!(
                        status,
                        Some(AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady)
                    ) {
                        waiting.push((format!("chat:{}", agent.id), agent.title));
                        continue;
                    }
                    if !matches!(status, Some(AgentChatStatus::Idle)) || agent.started_at.is_none()
                    {
                        continue;
                    }
                    let Some(session_id) = session_id
                        .as_deref()
                        .or(agent.chat_session_id.as_deref())
                        .or(agent.cli_session_id.as_deref())
                    else {
                        continue;
                    };
                    let Some(updated_at) = self
                        .agent_activity
                        .read(cx)
                        .updated_at(agent.id)
                        .or_else(|| {
                            last_activity_at.map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
                        })
                    else {
                        continue;
                    };
                    let working = std::time::SystemTime::now()
                        .duration_since(updated_at)
                        .map(|age| age < agents::WORKING_WINDOW)
                        .unwrap_or(false);
                    if working || manager.attention_suppressed(session_id, updated_at) {
                        continue;
                    }
                    let updated_secs = updated_at
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or_default();
                    waiting.push((
                        format!("chat:{}:{session_id}:{updated_secs}", agent.id),
                        agent.title,
                    ));
                    continue;
                }
                let Some(session) = manager.agent_record_session(project, agent.id) else {
                    continue;
                };
                if session.exited {
                    continue;
                }
                let Some(session_id) = session
                    .agent_session_id
                    .as_deref()
                    .or(agent.cli_session_id.as_deref())
                else {
                    continue;
                };
                let Some(updated_at) = self.agent_activity.read(cx).updated_at(agent.id) else {
                    continue;
                };
                let working = std::time::SystemTime::now()
                    .duration_since(updated_at)
                    .map(|age| age < agents::WORKING_WINDOW)
                    .unwrap_or(false);
                if working || manager.attention_suppressed(session_id, updated_at) {
                    continue;
                }
                let updated_secs = updated_at
                    .duration_since(UNIX_EPOCH)
                    .map(|duration| duration.as_secs())
                    .unwrap_or_default();
                waiting.push((
                    format!("{}:{session_id}:{updated_secs}", agent.id),
                    agent.title,
                ));
            }
            waiting
        };

        for (key, title) in waiting {
            if self.waiting_notifications.insert(key) {
                notifications::notify_agent_waiting(&title, project_name);
            }
        }
    }

    pub fn open_new_agent_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_new_agent_composer(window, cx);
            });
        }
    }

    fn runtime_for(&self, agent: &AgentRecord, project: ProjectId, cx: &App) -> AgentRunStatus {
        if agent.runtime == AgentRuntimeKind::Chat {
            let (status, session_id, last_activity_at) = self
                .agent_chats
                .read(cx)
                .session(agent.id)
                .map(|session| {
                    (
                        Some(session.status),
                        session
                            .chat_session_id
                            .clone()
                            .or_else(|| session.cli_session_id.clone()),
                        Some(session.last_activity_at),
                    )
                })
                .unwrap_or((None, None, None));
            return match status {
                Some(AgentChatStatus::Running | AgentChatStatus::Cancelling) => {
                    AgentRunStatus::Working
                }
                Some(AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady) => {
                    AgentRunStatus::Waiting
                }
                Some(AgentChatStatus::Failed) => AgentRunStatus::Ended,
                _ if agent.started_at.is_some() => {
                    let manager = self.terminals.read(cx);
                    if let Some(session_id) = session_id
                        .as_deref()
                        .or(agent.chat_session_id.as_deref())
                        .or(agent.cli_session_id.as_deref())
                    {
                        let updated_at =
                            self.agent_activity
                                .read(cx)
                                .updated_at(agent.id)
                                .or_else(|| {
                                    last_activity_at
                                        .map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
                                });
                        if let Some(updated_at) = updated_at {
                            let working = std::time::SystemTime::now()
                                .duration_since(updated_at)
                                .map(|age| age < agents::WORKING_WINDOW)
                                .unwrap_or(false);
                            if working {
                                return AgentRunStatus::Working;
                            }
                            if !manager.attention_suppressed(session_id, updated_at) {
                                return AgentRunStatus::Waiting;
                            }
                        }
                    }
                    AgentRunStatus::Idle
                }
                _ => AgentRunStatus::NotStarted,
            };
        }

        let manager = self.terminals.read(cx);
        if let Some(session) = manager.agent_record_session(project, agent.id) {
            if session.exited {
                return AgentRunStatus::Ended;
            }
            let session_id = session
                .agent_session_id
                .as_deref()
                .or(agent.cli_session_id.as_deref());
            if let Some(session_id) = session_id {
                if let Some(updated_at) = self.agent_activity.read(cx).updated_at(agent.id) {
                    let working = std::time::SystemTime::now()
                        .duration_since(updated_at)
                        .map(|age| age < agents::WORKING_WINDOW)
                        .unwrap_or(false);
                    if working {
                        return AgentRunStatus::Working;
                    }
                    if !manager.attention_suppressed(session_id, updated_at) {
                        return AgentRunStatus::Waiting;
                    }
                }
            }
            return AgentRunStatus::Open;
        }
        if agent.started_at.is_some() || agent.cli_session_id.is_some() {
            AgentRunStatus::Idle
        } else {
            AgentRunStatus::NotStarted
        }
    }

    fn agent_activity_label(
        &self,
        agent: &AgentRecord,
        project: ProjectId,
        cx: &App,
    ) -> Option<SharedString> {
        let live_chat_time = (agent.runtime == AgentRuntimeKind::Chat)
            .then(|| {
                self.agent_chats
                    .read(cx)
                    .session(agent.id)
                    .map(|session| UNIX_EPOCH + Duration::from_secs(session.last_activity_at))
            })
            .flatten();

        let manager = self.terminals.read(cx);
        let transcript_time = manager
            .agent_record_session(project, agent.id)
            .and_then(|session| {
                session
                    .agent_session_id
                    .as_deref()
                    .or(agent.chat_session_id.as_deref())
                    .or(agent.cli_session_id.as_deref())
            })
            .or(agent.chat_session_id.as_deref())
            .or(agent.cli_session_id.as_deref())
            .and_then(|_| self.agent_activity.read(cx).updated_at(agent.id));

        let fallback_time = if agent.updated_at > 0 {
            Some(UNIX_EPOCH + Duration::from_secs(agent.updated_at))
        } else {
            agent
                .started_at
                .map(|started_at| UNIX_EPOCH + Duration::from_secs(started_at))
        };
        live_chat_time
            .or(transcript_time)
            .or(fallback_time)
            .map(compact_relative_time)
    }

    fn open_agent(&mut self, agent_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        let Some(project) = self.workspace.read(cx).active_project().map(|p| p.id) else {
            return;
        };
        self.agents
            .update(cx, |agents, cx| agents.select(project, agent_id, cx));
        if let Some(center) = self.center.upgrade() {
            center.update(cx, |center, cx| {
                center.open_agent(agent_id, window, cx);
            });
        }
    }

    fn render_runtime_spinner(&self, ix: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        logo_spinner(16., "agent-panel-logo", ix, crate::ui::design::t3(cx))
    }

    fn render_agent_row(
        &self,
        ix: usize,
        agent: &AgentRecord,
        selected: bool,
        runtime: AgentRunStatus,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let waiting = runtime == AgentRunStatus::Waiting;
        let amber = crate::ui::design::amber(cx);
        let agent_id = agent.id;
        let activity_label = self.agent_activity_label(agent, agent.project_id, cx);

        h_flex()
            .id(("agent-row", ix))
            .mx_1()
            .px_2()
            .py_1()
            .gap_1p5()
            .items_center()
            .rounded(crate::ui::design::r_sm())
            .when(selected, |row| {
                row.bg(crate::ui::design::surface_2(cx).opacity(0.58))
                    .border_1()
                    .border_color(crate::ui::design::t3(cx).opacity(0.08))
            })
            .when(waiting, |row| row.bg(amber.opacity(0.1)))
            .hover(|style| style.bg(crate::ui::design::hover(cx)))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_agent(agent_id, window, cx);
            }))
            // Solo marker: the agent works on its own lane, not the active branch.
            .when(agent.is_solo(), |row| {
                row.child(crate::ui::design::indicator::solo_icon(
                    crate::ui::design::sky(cx),
                    crate::ui::design::icon_sm(),
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_size(crate::ui::design::text_body())
                    .truncate()
                    .when(waiting, |t| {
                        t.font_weight(gpui::FontWeight::SEMIBOLD).text_color(amber)
                    })
                    .child(SharedString::from(agent.title.clone())),
            )
            .when(runtime == AgentRunStatus::Working, |row| {
                row.child(
                    div()
                        .flex_none()
                        .w(px(34.))
                        .flex()
                        .justify_end()
                        .child(self.render_runtime_spinner(ix, cx)),
                )
            })
            .when(runtime != AgentRunStatus::Working, |row| {
                row.when_some(activity_label, |row, label| {
                    row.child(
                        div()
                            .flex_none()
                            .w(px(34.))
                            .flex()
                            .justify_end()
                            .text_size(crate::ui::design::text_label())
                            .text_color(crate::ui::design::t3(cx).opacity(0.74))
                            .child(label),
                    )
                })
            })
            .into_any_element()
    }

    fn render_lane(
        &self,
        status: AgentStatus,
        agents: &[AgentRecord],
        selected: Option<Uuid>,
        start_ix: usize,
        project: ProjectId,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let count = agents.len();
        // Empty lanes are hidden entirely to keep the board focused.
        if count == 0 {
            return div().into_any_element();
        }
        let collapsed = self.collapsed.contains(&status);
        let accent = status_accent(status, cx);
        let hover_bg = crate::ui::design::hover(cx);
        let mut lane = v_flex().w_full().gap_0p5().child(
            h_flex()
                .id(("agent-lane-header", start_ix))
                .mx_1()
                .mt_2()
                .px_2()
                .py_1p5()
                .items_center()
                .gap_2()
                .rounded(crate::ui::design::r_sm())
                .cursor_pointer()
                .hover(move |row| row.bg(hover_bg))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.collapsed.remove(&status) {
                        this.collapsed.insert(status);
                    }
                    cx.notify();
                }))
                .child(
                    Icon::new(if collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    })
                    .size(crate::ui::design::icon_sm())
                    .text_color(crate::ui::design::t3(cx)),
                )
                .child(status_icon(status, accent))
                .child(
                    div()
                        .text_size(crate::ui::design::text_body())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(crate::ui::design::t1(cx).opacity(0.82))
                        .child(status.label()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(crate::ui::design::text_label())
                        .text_color(crate::ui::design::t3(cx))
                        .child(count.to_string()),
                ),
        );

        if collapsed {
            return lane.into_any_element();
        }

        {
            let expanded = self.expanded_lanes.contains(&status);
            let visible_count = if expanded {
                count
            } else {
                count.min(LANE_PREVIEW_LIMIT)
            };
            let hidden_count = count.saturating_sub(LANE_PREVIEW_LIMIT);

            for (offset, agent) in agents.iter().enumerate().take(visible_count) {
                lane = lane.child(self.render_agent_row(
                    start_ix + offset,
                    agent,
                    selected == Some(agent.id),
                    self.runtime_for(agent, project, cx),
                    cx,
                ));
            }
            if count > LANE_PREVIEW_LIMIT {
                lane = lane.child(
                    h_flex()
                        .id(("agent-lane-toggle", start_ix))
                        .mx_1()
                        .px_2()
                        .py_1()
                        .gap_1p5()
                        .items_center()
                        .rounded(crate::ui::design::r_sm())
                        .cursor_pointer()
                        .text_size(crate::ui::design::text_ui())
                        .text_color(crate::ui::design::t3(cx))
                        .hover(|row| row.bg(crate::ui::design::hover(cx)))
                        .child(
                            Icon::new(if expanded {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(crate::ui::design::icon_sm()),
                        )
                        .child(if expanded {
                            "Show fewer agents".to_string()
                        } else {
                            format!("Show {hidden_count} more agents")
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            if !this.expanded_lanes.remove(&status) {
                                this.expanded_lanes.insert(status);
                            }
                            cx.notify();
                        })),
                );
            }
        }
        lane.into_any_element()
    }
}

impl Render for AgentsPanel {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(project) = self.workspace.read(cx).active_project().map(|p| p.id) else {
            return v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .text_size(crate::ui::design::text_body())
                .text_color(crate::ui::design::t3(cx))
                .child("Open a project to see agents")
                .into_any_element();
        };

        let agents = self.agents.read(cx).records_for_project(project);
        let selected = self.agents.read(cx).selected_agent_id(project);
        let new_button =
            crate::ui::style::context_panel_action_button("agents-new", IconName::Plus, "New", cx)
                .tooltip("Create an app-owned agent")
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_new_agent_dialog(window, cx);
                }));

        let header = crate::ui::design::header::panel_bar(cx)
            .child(crate::ui::design::header::panel_identity(
                None, "Agents", cx,
            ))
            .child(div().flex_1())
            .child(new_button);

        if agents.is_empty() {
            return v_flex()
                .size_full()
                .child(header)
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .text_color(crate::ui::design::t3(cx))
                        .child(Icon::new(IconName::Bot).size_8())
                        .child(
                            div()
                                .text_size(crate::ui::design::text_body())
                                .child("No agents yet"),
                        )
                        .child(
                            div()
                                .text_size(crate::ui::design::text_ui())
                                .child("Create a scoped agent for this project"),
                        )
                        .child(
                            crate::ui::style::primary_button("empty-new-agent", "New Agent", cx)
                                .icon(IconName::Plus)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_new_agent_dialog(window, cx);
                                })),
                        ),
                )
                .into_any_element();
        }

        let mut ix = 0;
        let mut list = v_flex()
            .id("agent-lanes")
            .flex_1()
            .min_h(px(0.))
            .px_1()
            .pb_3()
            .overflow_y_scroll();
        for status in AgentStatus::ALL {
            let lane_agents: Vec<AgentRecord> = agents
                .iter()
                .filter(|agent| agent.status == status)
                .cloned()
                .collect();
            list = list.child(self.render_lane(status, &lane_agents, selected, ix, project, cx));
            ix += lane_agents.len() + 10;
        }

        v_flex()
            .size_full()
            .child(header)
            .child(list)
            .into_any_element()
    }
}
