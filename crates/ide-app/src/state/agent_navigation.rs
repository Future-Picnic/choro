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
                if session.last_activity_at > 0 && session_id.is_some_and(|id|
                    !terminals.attention_suppressed(id, UNIX_EPOCH + Duration::from_secs(session.last_activity_at))) {
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
