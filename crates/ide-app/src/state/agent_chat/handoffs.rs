use super::*;

impl AgentChatState {
    pub fn has_queued_work(&self, agent_id: Uuid) -> bool {
        self.handoffs_sending.contains(&agent_id)
            || self
                .session(agent_id)
                .is_some_and(|session| !session.queued_turns.is_empty())
    }

    /// Outgoing teammate requests share FIFO ordering with ordinary messages.
    /// They are transport operations, never prompts for the source backend.
    pub fn handoff_must_wait(&self, agent_id: Uuid) -> bool {
        self.has_queued_work(agent_id)
            || self.session(agent_id).is_some_and(|session| {
                !matches!(
                    session.status,
                    AgentChatStatus::Idle | AgentChatStatus::Failed
                )
            })
    }

    pub fn queue_agent_handoff(
        &mut self,
        agent_id: Uuid,
        text: String,
        handoff: QueuedAgentHandoff,
        cx: &mut Context<Self>,
    ) {
        let Some(session) = self.sessions.get_mut(&agent_id) else {
            return;
        };
        let label = if handoff.kind == "ask" {
            "Question to"
        } else {
            "Task for"
        };
        session.queued_turns.push(QueuedChatTurn {
            id: Uuid::new_v4(),
            text,
            display_text: Some(format!(
                "{label} {}: {}",
                handoff.target_title, handoff.original_text
            )),
            tags: Vec::new(),
            mode: session.interaction_mode,
            created_at: unix_now(),
            handoff: Some(handoff),
            studio_request: None,
        });
        if session.status == AgentChatStatus::Idle {
            self.schedule_next_queued_turn(agent_id, cx);
        }
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }

    pub(super) fn send_queued_handoff(
        &mut self,
        agent_id: Uuid,
        turn: QueuedChatTurn,
        cx: &mut Context<Self>,
    ) {
        let Some(handoff) = turn.handoff.clone() else {
            return;
        };
        let Some(session) = self.sessions.get(&agent_id) else {
            return;
        };
        let source_title = session.title.clone();
        self.handoffs_sending.insert(agent_id);
        let text = turn.text.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    LocalStore::open_default().and_then(|store| {
                        store.send_agent_message(
                            agent_id,
                            handoff.target_agent_id,
                            &text,
                            &handoff.kind,
                            None,
                        )
                    })
                })
                .await;
            this.update(cx, |state, cx| {
                state.handoffs_sending.remove(&agent_id);
                let Some(session) = state.sessions.get_mut(&agent_id) else {
                    return;
                };
                match result {
                    Ok(message) => {
                        let handoff = turn.handoff.as_ref().expect("queued handoff");
                        let item = AgentChatTimelineItem::AgentMessage(AgentMessageCard {
                            id: message.id,
                            source_agent_id: agent_id,
                            source_title,
                            target_agent_id: Some(handoff.target_agent_id),
                            target_title: Some(handoff.target_title.clone()),
                            text: message.text,
                            kind: message.kind,
                            created_at: message.created_at,
                        });
                        session.timeline.push(item.clone());
                        persist_timeline_item(agent_id, item, cx);
                        state.schedule_next_queued_turn(agent_id, cx);
                    }
                    Err(error) => {
                        // Keep the request editable/retryable, without a tight
                        // automatic retry loop or letting later turns overtake it.
                        let mut turn = turn;
                        turn.display_text = Some(format!(
                            "Send failed — {}",
                            turn.display_text.as_deref().unwrap_or("Teammate request")
                        ));
                        session.queued_turns.insert(0, turn);
                        eprintln!("could not send queued teammate request: {error:#}");
                    }
                }
                state.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
            })
            .ok();
        })
        .detach();
        self.publish_change(agent_id, ChatChangeCategories::CONTENT, cx);
    }
}
