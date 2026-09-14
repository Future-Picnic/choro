//! Reusable chat hydration and dispatch, independent of any mounted view.
use super::agent_chat::*;
use anyhow::{ensure, Result};
use gpui::Context;
use ide_core::{local_store::LocalStore, AgentRecord};

pub(crate) fn load_history_from_store(
    store: &LocalStore,
    agent: &AgentRecord,
) -> Result<Vec<AgentChatTimelineItem>> {
    let events = store.load_timeline_events(agent.id)?;
    let mut timeline = events
        .iter()
        .filter_map(timeline_item_from_store_event)
        .collect::<Vec<_>>();
    if timeline.is_empty() {
        for m in store.load_chat_messages(agent.id)? {
            let message = match m.role.as_str() {
                "user" => AgentChatMessage::User {
                    text: m.text,
                    display_text: None,
                    tags: vec![],
                    created_at: m.created_at,
                },
                "assistant" => AgentChatMessage::Assistant {
                    message_id: m.backend_message_id,
                    text: m.text,
                    created_at: m.created_at,
                },
                _ => continue,
            };
            timeline.push(AgentChatTimelineItem::Message(message));
        }
    }
    ensure!(agent.started_at.is_none() || agent.chat_session_id.is_some() || agent.cli_session_id.is_some(),
        "Cannot resume this chat: its provider session ID is missing. The saved conversation and files are preserved.");
    Ok(timeline)
}

pub(crate) fn hydrate(session: &mut AgentChatSession, timeline: Vec<AgentChatTimelineItem>) {
    let mut changed_files = ChangedFilesSummary::default();
    session.messages.clear();
    session.work_log.clear();
    session.pending_user_input = None;
    for item in &timeline {
        match item {
            AgentChatTimelineItem::Message(m) => session.messages.push(m.clone()),
            AgentChatTimelineItem::WorkLog(e) => session.work_log.push(e.clone()),
            AgentChatTimelineItem::PendingUserInput(p) => {
                session.pending_user_input = Some(p.clone())
            }
            AgentChatTimelineItem::ChangedFiles(s) => changed_files.merge_turn(s),
            _ => (),
        }
    }
    session.changed_files = load_persisted_file_ledger(session.agent_id)
        .filter(|s| s.ledger_revision >= changed_files.ledger_revision)
        .unwrap_or(changed_files);
    session.proposed_plan = None;
    session.timeline = timeline;
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dispatch_loaded(
    chats: &mut AgentChatState,
    agent_id: uuid::Uuid,
    text: String,
    display_text: Option<String>,
    tags: Vec<AgentChatMessageTag>,
    mode: AgentInteractionMode,
    queued: bool,
    read_only: bool,
    cx: &mut Context<AgentChatState>,
) {
    if queued {
        chats.queue_turn(agent_id, text, display_text, tags, mode, cx);
    } else {
        let created_at = ide_core::agents::unix_now();
        chats.append_message(
            agent_id,
            AgentChatMessage::User {
                text: text.clone(),
                display_text,
                tags,
                created_at,
            },
            cx,
        );
        if read_only {
            chats.send_read_only_turn(agent_id, text, mode, cx);
        } else {
            chats.send_turn(agent_id, text, mode, cx);
        }
    }
}

pub(crate) fn managed_send(
    chats: &mut AgentChatState,
    agent: AgentRecord,
    timeline: Vec<AgentChatTimelineItem>,
    text: String,
    mode: AgentInteractionMode,
    cx: &mut Context<AgentChatState>,
) -> Result<u64> {
    // Caller has reserved the boundary and persisted the delivery first.
    let id = agent.id;
    let mode = protocol::managed::interaction_mode(&agent, mode);
    let session = chats.ensure_session(id, agent.title.clone(), cx);
    if session.timeline.is_empty() {
        hydrate(session, timeline);
    }
    session.chat_session_id = agent
        .chat_session_id
        .clone()
        .or(session.chat_session_id.clone());
    session.cli_session_id = agent
        .cli_session_id
        .clone()
        .or(session.cli_session_id.clone());
    session.hidden_from_notifications = agent
        .delegation
        .as_ref()
        .is_some_and(|b| b.task_id.is_some());
    session.interaction_mode = mode;
    chats.start_backend(agent, mode, cx)?;
    dispatch_loaded(
        chats,
        id,
        text,
        Some("Delegation coordination".into()),
        vec![],
        mode,
        false,
        false,
        cx,
    );
    Ok(chats.backend_generation(id))
}
