use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use gpui::{App, AppContext, Entity};
use ide_core::{agents, AgentKind, AgentRuntimeKind};
use uuid::Uuid;

use super::{AgentChatState, AgentRecords, TerminalManager};

const REFRESH_INTERVAL: Duration = Duration::from_secs(3);

/// Shared, background-refreshed transcript activity for agent status UI.
pub struct AgentActivityCache {
    agents: Entity<AgentRecords>,
    agent_chats: Entity<AgentChatState>,
    terminals: Entity<TerminalManager>,
    updated_at: HashMap<Uuid, SystemTime>,
}

impl AgentActivityCache {
    pub fn view(
        agents: Entity<AgentRecords>,
        agent_chats: Entity<AgentChatState>,
        terminals: Entity<TerminalManager>,
        cx: &mut App,
    ) -> Entity<Self> {
        cx.new(|cx| {
            cx.spawn(async move |this, cx| loop {
                let Some(cache) = this.upgrade() else {
                    break;
                };
                let poll = cache
                    .update(cx, |cache: &mut Self, cx| cache.poll_queries(cx))
                    .unwrap_or_default();
                let activity = cx
                    .background_executor()
                    .spawn(async move { resolve_activity(poll) })
                    .await;
                if cache
                    .update(cx, |cache: &mut Self, cx| {
                        if cache.updated_at != activity {
                            cache.updated_at = activity;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                cx.background_executor().timer(REFRESH_INTERVAL).await;
            })
            .detach();

            Self {
                agents,
                agent_chats,
                terminals,
                updated_at: HashMap::new(),
            }
        })
    }

    pub fn updated_at(&self, agent_id: Uuid) -> Option<SystemTime> {
        self.updated_at.get(&agent_id).copied()
    }

    fn poll_queries(&self, cx: &App) -> Vec<ActivityQuery> {
        let records = self.agents.read(cx).all_records();
        let chats = self.agent_chats.read(cx);
        let terminals = self.terminals.read(cx);
        records
            .into_iter()
            .filter_map(|agent| {
                let live_chat_session = (agent.runtime == AgentRuntimeKind::Chat)
                    .then(|| {
                        chats.session(agent.id).and_then(|session| {
                            session
                                .chat_session_id
                                .clone()
                                .or_else(|| session.cli_session_id.clone())
                        })
                    })
                    .flatten();
                let terminal_session = terminals
                    .agent_record_session(agent.project_id, agent.id)
                    .and_then(|session| session.agent_session_id.clone());
                let session_id = live_chat_session
                    .or(terminal_session)
                    .or_else(|| agent.chat_session_id.clone())
                    .or_else(|| agent.cli_session_id.clone())?;
                Some(ActivityQuery {
                    agent_id: agent.id,
                    provider: agent.provider,
                    cwd: agent.runtime_path().to_path_buf(),
                    session_id,
                })
            })
            .collect()
    }
}

struct ActivityQuery {
    agent_id: Uuid,
    provider: AgentKind,
    cwd: PathBuf,
    session_id: String,
}

fn resolve_activity(queries: Vec<ActivityQuery>) -> HashMap<Uuid, SystemTime> {
    let lookups = queries
        .iter()
        .map(|query| (query.provider, query.cwd.clone(), query.session_id.clone()))
        .collect::<Vec<_>>();
    let by_session = agents::chat_updated_at_batch(&lookups);
    queries
        .into_iter()
        .filter_map(|query| {
            by_session
                .get(&query.session_id)
                .copied()
                .map(|updated_at| (query.agent_id, updated_at))
        })
        .collect()
}
