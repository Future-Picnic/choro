//! Shared provider presentation for the desktop sidebar and remote home.
use super::agent_chat::AgentChatStatus;
use super::{AgentActivityCache, AgentChatState, TerminalManager};
use ide_core::{agents, AgentRecord, AgentRuntimeKind, ProjectId};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentNavigationRuntime {
    NotStarted,
    Working,
    Waiting,
    Open,
    Idle,
    Ended,
}

pub fn provider_navigation_runtime(
    agent: &AgentRecord,
    project: ProjectId,
    chats: &AgentChatState,
    activity: &AgentActivityCache,
    terminals: &TerminalManager,
    now: SystemTime,
) -> AgentNavigationRuntime {
    if agent.status.is_finished() {
        return AgentNavigationRuntime::Idle;
    }
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
        return match status {
            Some(AgentChatStatus::Running | AgentChatStatus::Cancelling) => {
                AgentNavigationRuntime::Working
            }
            Some(AgentChatStatus::WaitingForUser | AgentChatStatus::PlanReady) => {
                AgentNavigationRuntime::Waiting
            }
            Some(AgentChatStatus::Failed) => AgentNavigationRuntime::Ended,
            Some(AgentChatStatus::Idle) if chats.has_authoritative_status(agent.id) => {
                let session = chats.session(agent.id).unwrap();
                let session_id = session.chat_session_id.as_deref().or(session.cli_session_id.as_deref());
                if session.completed_response_at().is_some_and(|at| at > 0 && session_id.is_some_and(|id|
                    !terminals.attention_suppressed(id, UNIX_EPOCH + Duration::from_secs(at)))) {
                    AgentNavigationRuntime::Waiting
                } else {
                    AgentNavigationRuntime::Idle
                }
            }
            _ if agent.started_at.is_some() => {
                let manager = terminals;
                if let Some(session_id) = session_id
                    .as_deref()
                    .or(agent.chat_session_id.as_deref())
                    .or(agent.cli_session_id.as_deref())
                {
                    let updated_at = activity.updated_at(agent.id).or_else(|| {
                        last_activity_at.map(|secs| UNIX_EPOCH + Duration::from_secs(secs))
                    });
                    if let Some(updated_at) = updated_at {
                        if let Some(runtime) = transcript_navigation_runtime(
                            updated_at,
                            now,
                            manager.attention_suppressed(session_id, updated_at),
                        ) {
                            return runtime;
                        }
                    }
                }
                AgentNavigationRuntime::Idle
            }
            _ => AgentNavigationRuntime::NotStarted,
        };
    }

    let manager = terminals;
    if let Some(session) = manager.agent_record_session(project, agent.id) {
        if session.exited {
            return AgentNavigationRuntime::Ended;
        }

        let session_id = session
            .agent_session_id
            .as_deref()
            .or(agent.cli_session_id.as_deref());
        if let Some(session_id) = session_id {
            if let Some(updated_at) = activity.updated_at(agent.id) {
                if let Some(runtime) = transcript_navigation_runtime(
                    updated_at,
                    now,
                    manager.attention_suppressed(session_id, updated_at),
                ) {
                    return runtime;
                }
            }
        }

        return AgentNavigationRuntime::Open;
    }

    if agent.started_at.is_some() || agent.cli_session_id.is_some() {
        AgentNavigationRuntime::Idle
    } else {
        AgentNavigationRuntime::NotStarted
    }
}

fn transcript_navigation_runtime(
    updated_at: SystemTime,
    now: SystemTime,
    seen: bool,
) -> Option<AgentNavigationRuntime> {
    if now
        .duration_since(updated_at)
        .map(|age| age < agents::WORKING_WINDOW)
        .unwrap_or(false)
    {
        Some(AgentNavigationRuntime::Working)
    } else if !seen {
        Some(AgentNavigationRuntime::Waiting)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "ui-layout-tests")]
    #[gpui::test]
    fn completed_chat_answers_need_attention_immediately_for_local_and_tumble_tasks(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{App, AppContext};
        use ide_core::{AgentKind, AgentModel, AgentOrigin, AgentStatus};

        cx.update(|cx| {
            let mut agent = AgentRecord::new(
                ProjectId::new(),
                "/in-memory/completion".into(),
                "Task",
                "",
                AgentKind::Codex,
                AgentModel::default_for(AgentKind::Codex),
                Default::default(),
                Default::default(),
            );
            agent.runtime = AgentRuntimeKind::Chat;
            agent.status = AgentStatus::InProgress;
            agent.started_at = Some(1);
            let records = cx.new(|_| super::super::AgentRecords::in_memory(vec![agent.clone()]));
            let terminals = cx.new(|_| TerminalManager::new());
            let chats = cx.new(|_| AgentChatState::new());
            let activity = cx.new(|_| {
                AgentActivityCache::in_memory(records, chats.clone(), terminals.clone())
            });
            // A deterministic response newer than the terminal manager's startup
            // baseline, without a real backend, filesystem polling, or sleeps.
            let completed_at = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() + 60;
            let now = UNIX_EPOCH + Duration::from_secs(completed_at);
            activity.update(cx, |activity, cx| activity.set_fixture_activity(agent.id, now, cx));
            chats.update(cx, |chats, cx| {
                chats.fixture_change(agent.id, AgentChatStatus::Idle, "", cx);
                chats.ensure_session(agent.id, "Task", cx).chat_session_id = Some("completion-fixture".into());
            });
            let runtime = |agent: &AgentRecord, cx: &App| {
                provider_navigation_runtime(agent, agent.project_id, chats.read(cx), activity.read(cx), terminals.read(cx), now)
            };
            assert_eq!(runtime(&agent, cx), AgentNavigationRuntime::Idle,
                "runtime bookkeeping alone is not a completed answer");
            chats.update(cx, |chats, _| {
                chats.sessions.get_mut(&agent.id).unwrap().messages.push(
                    super::super::agent_chat::AgentChatMessage::Assistant {
                        message_id: Some("answer-1".into()),
                        text: "Finished the task".into(),
                        created_at: completed_at,
                    },
                );
            });
            for origin in [None, Some(AgentOrigin::PocketComet {
                workspace_id: "workspace".into(),
                project_id: "project".into(),
                task_id: "task".into(),
                task_title: "Task".into(),
            })] {
                agent.origin = origin;
                assert_eq!(runtime(&agent, cx), AgentNavigationRuntime::Waiting,
                    "a completed answer needs attention even while transcript activity is fresh");
            }
            chats.update(cx, |chats, cx| chats.fixture_change(agent.id, AgentChatStatus::Running, "", cx));
            assert_eq!(runtime(&agent, cx), AgentNavigationRuntime::Working);
        });
    }

    #[test]
    fn navigation_attention_follows_activity_and_acknowledgement() {
        let updated = UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(
            transcript_navigation_runtime(updated, updated, false),
            Some(AgentNavigationRuntime::Working)
        );
        let settled = updated + agents::WORKING_WINDOW;
        assert_eq!(
            transcript_navigation_runtime(updated, settled, false),
            Some(AgentNavigationRuntime::Waiting)
        );
        assert_eq!(transcript_navigation_runtime(updated, settled, true), None);
        let new_response = settled + Duration::from_secs(30);
        assert_eq!(
            transcript_navigation_runtime(
                new_response,
                new_response + agents::WORKING_WINDOW,
                false
            ),
            Some(AgentNavigationRuntime::Waiting)
        );
    }
}
