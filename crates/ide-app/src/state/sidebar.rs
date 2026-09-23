//! Presentation state independent of any mounted sidebar or animation frame.
use super::{
    agent_chat::{AgentChatEvent, ChatChange},
    agent_navigation::{self, AgentNavigationRuntime},
    delegation::{
        display::{self, DelegationActivity},
        DelegationHandle,
    },
    *,
};
use gpui::{App, AppContext, Context, Entity, EventEmitter, Task};
use ide_core::{AgentRecord, AgentStatus, ProjectId};
use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant, SystemTime},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SidebarAgent {
    pub id: Uuid,
    pub project_id: ProjectId,
    pub title: String,
    pub status: AgentStatus,
    pub updated_at: u64,
    order: usize,
    pub runtime: AgentNavigationRuntime,
    pub delegation: DelegationActivity,
    pub has_delegations: bool,
    pub pocketcomet: bool,
    pub solo: bool,
}

#[cfg(all(test, feature = "ui-layout-tests"))]
mod tests {
    use super::*;

    #[gpui::test]
    fn legacy_activity_expires_without_an_unrelated_redraw(cx: &mut gpui::TestAppContext) {
        let (model, activity, id) = cx.update(|cx| {
            let project = ide_core::Project::from_path("/in-memory/expiry".into());
            let mut config = ide_core::AppConfig::default();
            config.projects.push(project.clone());
            let workspace = cx.new(|_| Workspace::in_memory(config));
            let provider = ide_core::AgentKind::Codex;
            let model = ide_core::AgentModel::default_for(provider);
            let mut agent = AgentRecord::new(
                project.id,
                project.path,
                "Legacy",
                "",
                provider,
                model,
                model.default_effort(),
                Default::default(),
            );
            agent.runtime = ide_core::AgentRuntimeKind::Chat;
            agent.started_at = Some(1);
            agent.chat_session_id = Some("legacy-session".into());
            let id = agent.id;
            let records = cx.new(|_| AgentRecords::in_memory(vec![agent]));
            let chats = cx.new(|_| AgentChatState::new());
            let terminals = cx.new(|_| TerminalManager::new());
            let activity = cx.new(|_| {
                AgentActivityCache::in_memory(records.clone(), chats.clone(), terminals.clone())
            });
            let git = cx.new(|_| GitStates::in_memory(workspace.clone()));
            let model = SidebarModel::new(
                workspace,
                records,
                chats,
                activity.clone(),
                terminals,
                git,
                cx,
            );
            let now = model.read(cx).now(cx);
            activity.update(cx, |activity, cx| {
                activity.set_fixture_activity(id, now, cx)
            });
            (model, activity, id)
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                model.read(cx).agents[&id].runtime,
                AgentNavigationRuntime::Working
            )
        });
        cx.background_executor.advance_clock(Duration::from_secs(9));
        cx.run_until_parked();
        // Fresh activity replaces the old deadline. Its cancelled timer must
        // neither repaint nor switch the latest state to Waiting.
        cx.update(|cx| {
            let now = model.read(cx).now(cx);
            activity.update(cx, |activity, cx| {
                activity.set_fixture_activity(id, now, cx)
            });
        });
        cx.run_until_parked();
        cx.background_executor.advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                model.read(cx).agents[&id].runtime,
                AgentNavigationRuntime::Working
            )
        });
        cx.background_executor.advance_clock(Duration::from_secs(9));
        cx.run_until_parked();
        cx.update(|cx| {
            assert_eq!(
                model.read(cx).agents[&id].runtime,
                AgentNavigationRuntime::Waiting
            );
            assert!(model.read(cx).next_expiry.is_none());
        });
    }
}

impl SidebarAgent {
    pub fn is_active_solo(&self) -> bool {
        self.solo
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SidebarProject {
    pub changes: usize,
    pub scripts: Vec<gpui::SharedString>,
    pub working: usize,
    pub delegating: usize,
    pub waiting: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct SidebarChange {
    pub agents: Vec<Uuid>,
    pub projects: Vec<ProjectId>,
    pub received_at: Instant,
}

pub(crate) struct SidebarModel {
    pub agents: HashMap<Uuid, SidebarAgent>,
    pub projects: HashMap<ProjectId, SidebarProject>,
    pub assignments: HashMap<Uuid, Vec<display::DelegatedTaskRow>>,
    expanded: HashSet<Uuid>,
    order: HashMap<Uuid, usize>,
    workspace: Entity<Workspace>,
    records: Entity<AgentRecords>,
    chats: Entity<AgentChatState>,
    activity: Entity<AgentActivityCache>,
    terminals: Entity<TerminalManager>,
    git: Entity<GitStates>,
    expiry: Option<Task<()>>,
    next_expiry: Option<SystemTime>,
    sequences: HashMap<Uuid, u64>,
    #[cfg(test)]
    clock_epoch: (SystemTime, Instant),
}

impl EventEmitter<SidebarChange> for SidebarModel {}

impl SidebarModel {
    pub fn new(
        workspace: Entity<Workspace>,
        records: Entity<AgentRecords>,
        chats: Entity<AgentChatState>,
        activity: Entity<AgentActivityCache>,
        terminals: Entity<TerminalManager>,
        git: Entity<GitStates>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.subscribe(&chats, |this: &mut Self, _, event, cx| {
                if let AgentChatEvent::SessionChanged(change) = event {
                    this.chat_changed(change, cx);
                }
            })
            .detach();
            cx.subscribe(&records, |this: &mut Self, _, event, cx| {
                if let super::agents::AgentRecordsEvent::RecordChanged { agent_id, .. } = event {
                    let parent = this
                        .records
                        .read(cx)
                        .agent(*agent_id)
                        .and_then(|a| a.delegation.as_ref().map(|b| b.parent_agent_id));
                    this.refresh(Some(parent.unwrap_or(*agent_id)), Instant::now(), cx);
                }
            })
            .detach();
            cx.observe(&workspace, |this: &mut Self, _, cx| {
                this.refresh(None, Instant::now(), cx)
            })
            .detach();
            cx.subscribe(&activity, |this: &mut Self, _, change, cx| {
                for id in &change.0 {
                    this.refresh(Some(*id), Instant::now(), cx);
                }
            })
            .detach();
            cx.observe(&terminals, |this: &mut Self, _, cx| {
                this.refresh(None, Instant::now(), cx)
            })
            .detach();
            cx.observe(&git, |this: &mut Self, _, cx| {
                this.refresh_projects(Instant::now(), cx)
            })
            .detach();
            if let Some(handle) = cx.try_global::<DelegationHandle>().cloned() {
                cx.subscribe(&handle.0, |this: &mut Self, _, change, cx| {
                    for id in &change.parents {
                        this.refresh(Some(*id), Instant::now(), cx);
                    }
                })
                .detach();
            }
            let mut model = Self {
                agents: HashMap::new(),
                projects: HashMap::new(),
                assignments: HashMap::new(),
                expanded: HashSet::new(),
                order: HashMap::new(),
                workspace,
                records,
                chats,
                activity,
                terminals,
                git,
                expiry: None,
                next_expiry: None,
                sequences: HashMap::new(),
                #[cfg(test)]
                clock_epoch: (SystemTime::now(), cx.background_executor().now()),
            };
            model.refresh(None, Instant::now(), cx);
            model
        })
    }

    pub fn set_expanded(&mut self, expanded: HashSet<Uuid>, cx: &mut Context<Self>) {
        if self.expanded == expanded {
            return;
        }
        self.expanded = expanded;
        self.refresh(None, Instant::now(), cx);
    }

    fn chat_changed(&mut self, change: &ChatChange, cx: &mut Context<Self>) {
        if self
            .sequences
            .get(&change.agent_id)
            .is_some_and(|seq| *seq >= change.sequence)
        {
            return;
        }
        self.sequences.insert(change.agent_id, change.sequence);
        if change.categories.navigation || change.categories.identity {
            let parent = self
                .records
                .read(cx)
                .agent(change.agent_id)
                .and_then(|a| a.delegation.as_ref().map(|b| b.parent_agent_id));
            self.refresh(
                Some(parent.unwrap_or(change.agent_id)),
                change.received_at,
                cx,
            );
        }
    }

    pub fn records_for_project(&self, project: ProjectId) -> Vec<SidebarAgent> {
        let mut records = self
            .agents
            .values()
            .filter(|a| a.project_id == project)
            .cloned()
            .collect::<Vec<_>>();
        records.sort_by_key(|a| (std::cmp::Reverse(a.updated_at), a.order));
        records
    }

    fn derive(&self, record: &AgentRecord, now: SystemTime, cx: &App) -> Option<SidebarAgent> {
        if record
            .delegation
            .as_ref()
            .is_some_and(|binding| binding.task_id.is_some())
            || record
                .origin
                .as_ref()
                .is_some_and(ide_core::AgentOrigin::is_pocketcomet_chat)
        {
            return None;
        }
        let chats = self.chats.read(cx);
        let needs_user = |id| {
            chats.session(id).is_some_and(|s| {
                s.pending_approval.is_some()
                    || s.pending_user_input.is_some()
                    || s.status == agent_chat::AgentChatStatus::PlanReady
            })
        };
        let (delegation, has_delegations) = cx
            .try_global::<DelegationHandle>()
            .map(|handle| {
                let runs = &handle.0.read(cx).runs;
                let activity = display::parent_delegation_activity(runs, record.id, &needs_user);
                let visible = runs
                    .iter()
                    .filter(|r| r.parent_agent_id == record.id && !r.status.terminal())
                    .any(|r| {
                        r.tasks.iter().any(|task| {
                            let activity = display::task_activity(
                                r.status,
                                task,
                                task.attempt().is_some_and(|a| needs_user(a.child_agent_id)),
                            );
                            !task.status.terminal()
                                && (activity == DelegationActivity::Attention
                                    || (activity == DelegationActivity::Working
                                        && task.status.occupies_slot()))
                        })
                    });
                (activity, visible)
            })
            .unwrap_or((DelegationActivity::Idle, false));
        let provider = agent_navigation::provider_navigation_runtime(
            record,
            record.project_id,
            chats,
            self.activity.read(cx),
            self.terminals.read(cx),
            now,
        );
        let runtime = if record.status.is_finished() || provider == AgentNavigationRuntime::Waiting
        {
            provider
        } else {
            match delegation {
                DelegationActivity::Attention => AgentNavigationRuntime::Waiting,
                DelegationActivity::Working => AgentNavigationRuntime::Working,
                _ => provider,
            }
        };
        Some(SidebarAgent {
            id: record.id,
            project_id: record.project_id,
            title: record.title.clone(),
            status: record.status,
            updated_at: record.updated_at,
            order: self.order.get(&record.id).copied().unwrap_or(usize::MAX),
            runtime,
            delegation,
            has_delegations,
            pocketcomet: record.origin.as_ref().is_some_and(|o| o.is_pocketcomet()),
            solo: record.is_active_solo(),
        })
    }

    fn now(&self, _cx: &App) -> SystemTime {
        #[cfg(test)]
        {
            self.clock_epoch.0
                + _cx
                    .background_executor()
                    .now()
                    .duration_since(self.clock_epoch.1)
        }
        #[cfg(not(test))]
        {
            SystemTime::now()
        }
    }

    fn refresh(&mut self, only: Option<Uuid>, received_at: Instant, cx: &mut Context<Self>) {
        let _probe = crate::ui::performance::UiProbe::new("sidebar.model");
        let now = self.now(cx);
        let records = self.records.read(cx);
        // Preserve the record owner's stable tie order, without cloning records
        // or doing one linear record lookup for every row in a full refresh.
        if let Some(id) = only {
            let next = self.order.len();
            self.order.entry(id).or_insert(next);
        } else {
            for record in records.iter_records() {
                let next = self.order.len();
                self.order.entry(record.id).or_insert(next);
            }
        }
        let updates = if let Some(id) = only {
            vec![(
                id,
                records
                    .agent(id)
                    .and_then(|record| self.derive(record, now, cx)),
            )]
        } else {
            let ids = records.iter_records().map(|a| a.id).collect::<HashSet<_>>();
            records
                .iter_records()
                .map(|record| (record.id, self.derive(record, now, cx)))
                .chain(
                    self.agents
                        .keys()
                        .filter(|id| !ids.contains(id))
                        .map(|id| (*id, None)),
                )
                .collect::<Vec<_>>()
        };
        let mut changed = Vec::new();
        let mut affected = HashSet::new();
        for (id, next) in updates {
            if self.agents.get(&id) == next.as_ref() {
                continue;
            }
            if let Some(previous) = self.agents.remove(&id) {
                affected.insert(previous.project_id);
            }
            if let Some(next) = next {
                affected.insert(next.project_id);
                self.agents.insert(id, next);
            }
            changed.push(id);
        }
        self.assignments
            .retain(|id, _| self.expanded.contains(id) && self.agents.contains_key(id));
        if let Some(handle) = cx.try_global::<DelegationHandle>() {
            let runs = &handle.0.read(cx).runs;
            let chats = self.chats.read(cx);
            for id in self
                .expanded
                .iter()
                .copied()
                .filter(|id| only.is_none_or(|target| target == *id))
            {
                let needs_user = |child| {
                    chats.session(child).is_some_and(|s| {
                        s.pending_approval.is_some()
                            || s.pending_user_input.is_some()
                            || s.status == agent_chat::AgentChatStatus::PlanReady
                    })
                };
                let mut state = display::parent_delegation_state(runs, id, &needs_user);
                state.tasks.retain(|t| {
                    !t.status.terminal()
                        && (t.activity == DelegationActivity::Attention
                            || (t.activity == DelegationActivity::Working
                                && t.status.occupies_slot()))
                });
                if self.assignments.get(&id) != Some(&state.tasks) {
                    self.assignments.insert(id, state.tasks);
                    if !changed.contains(&id) {
                        changed.push(id);
                    }
                }
            }
        }
        let projects = self.update_projects(
            if only.is_some() {
                Some(&affected)
            } else {
                None
            },
            cx,
        );
        self.schedule_expiry(now, cx);
        if !changed.is_empty() || !projects.is_empty() {
            cx.emit(SidebarChange {
                agents: changed,
                projects,
                received_at,
            });
            cx.notify();
        }
    }

    fn refresh_projects(&mut self, received_at: Instant, cx: &mut Context<Self>) {
        let projects = self.update_projects(None, cx);
        if !projects.is_empty() {
            cx.emit(SidebarChange {
                agents: vec![],
                projects,
                received_at,
            });
            cx.notify();
        }
    }

    fn update_projects(&mut self, only: Option<&HashSet<ProjectId>>, cx: &App) -> Vec<ProjectId> {
        let workspace = self.workspace.read(cx);
        let ids = workspace
            .projects
            .iter()
            .map(|p| p.id)
            .collect::<HashSet<_>>();
        let mut changed = self
            .projects
            .keys()
            .filter(|id| !ids.contains(id))
            .copied()
            .collect::<Vec<_>>();
        self.projects.retain(|id, _| ids.contains(id));
        for project in &workspace.projects {
            if only.is_some_and(|ids| !ids.contains(&project.id)) {
                continue;
            }
            let mut next = SidebarProject {
                changes: self
                    .git
                    .read(cx)
                    .get(project.id)
                    .and_then(|git| {
                        let git = git.read(cx);
                        git.is_repo
                            .then(|| git.snapshot.as_ref().map_or(0, |s| s.entries.len()))
                    })
                    .unwrap_or(0),
                scripts: self.terminals.read(cx).running_scripts(project.id),
                ..Default::default()
            };
            for agent in self
                .agents
                .values()
                .filter(|a| a.project_id == project.id && !a.status.is_finished())
            {
                match agent.runtime {
                    AgentNavigationRuntime::Working => {
                        next.working += 1;
                        next.delegating +=
                            usize::from(agent.delegation == DelegationActivity::Working);
                    }
                    AgentNavigationRuntime::Waiting => next.waiting += 1,
                    _ => {}
                }
            }
            if self.projects.get(&project.id) != Some(&next) {
                self.projects.insert(project.id, next);
                changed.push(project.id);
            }
        }
        changed
    }

    fn schedule_expiry(&mut self, now: SystemTime, cx: &mut Context<Self>) {
        let deadline = self
            .records
            .read(cx)
            .iter_records()
            .filter(|a| {
                !a.status.is_finished() && !self.chats.read(cx).has_authoritative_status(a.id)
            })
            .filter_map(|a| {
                self.activity.read(cx).updated_at(a.id).or_else(|| {
                    self.chats.read(cx).session(a.id).and_then(|s| {
                        std::time::UNIX_EPOCH.checked_add(Duration::from_secs(s.last_activity_at))
                    })
                })
            })
            .filter_map(|at| at.checked_add(ide_core::agents::WORKING_WINDOW))
            .filter(|at| *at > now)
            .min();
        if self.next_expiry == deadline {
            return;
        }
        self.expiry = None;
        self.next_expiry = deadline;
        if let Some(deadline) = deadline {
            self.expiry = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(deadline.duration_since(now).unwrap_or(Duration::ZERO))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.next_expiry = None;
                    this.refresh(None, Instant::now(), cx);
                });
            }));
        }
    }
}
