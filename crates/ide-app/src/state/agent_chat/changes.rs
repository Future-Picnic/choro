//! Cheap, per-session invalidation. A text chunk must not invalidate navigation.
use super::*;
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChatChangeCategories {
    pub conversation: bool,
    pub navigation: bool,
    pub identity: bool,
    pub controls: bool,
}

impl ChatChangeCategories {
    pub const CONVERSATION: Self = Self {
        conversation: true,
        navigation: false,
        identity: false,
        controls: false,
    };
    pub const CONTROLS: Self = Self {
        conversation: false,
        navigation: false,
        identity: false,
        controls: true,
    };
    pub const CONTENT: Self = Self {
        conversation: true,
        navigation: false,
        identity: false,
        controls: true,
    };
}

#[derive(Clone, Debug)]
pub struct ChatChange {
    pub agent_id: Uuid,
    pub generation: u64,
    pub sequence: u64,
    pub categories: ChatChangeCategories,
    pub received_at: Instant,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NavigationSnapshot {
    status: AgentChatStatus,
    question: bool,
    approval: bool,
    hidden: bool,
    // A generation establishes authoritative runtime state, including Idle.
    managed: bool,
    settled_activity: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct IdentitySnapshot {
    title: String,
    chat_session_id: Option<String>,
    cli_session_id: Option<String>,
}

#[derive(Default)]
pub(super) struct ChatChangeTracker {
    sequence: u64,
    navigation: HashMap<Uuid, NavigationSnapshot>,
    identity: HashMap<Uuid, IdentitySnapshot>,
}

impl ChatChangeTracker {
    fn record(
        &mut self,
        id: Uuid,
        generation: u64,
        session: Option<&AgentChatSession>,
        mut categories: ChatChangeCategories,
    ) -> ChatChange {
        let navigation = session.map(|s| NavigationSnapshot {
            status: s.status,
            question: s.pending_user_input.is_some(),
            approval: s.pending_approval.is_some(),
            hidden: s.hidden_from_notifications,
            managed: generation > 0,
            settled_activity: matches!(s.status, AgentChatStatus::Idle | AgentChatStatus::Failed)
                .then_some(s.last_activity_at),
        });
        let identity = session.map(|s| IdentitySnapshot {
            title: s.title.clone(),
            chat_session_id: s.chat_session_id.clone(),
            cli_session_id: s.cli_session_id.clone(),
        });
        categories.navigation |= self.navigation.get(&id) != navigation.as_ref();
        categories.identity |= self.identity.get(&id) != identity.as_ref();
        if let Some(value) = navigation {
            self.navigation.insert(id, value);
        } else {
            self.navigation.remove(&id);
        }
        if let Some(value) = identity {
            self.identity.insert(id, value);
        } else {
            self.identity.remove(&id);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .expect("chat change sequence exhausted");
        ChatChange {
            agent_id: id,
            generation,
            sequence: self.sequence,
            categories,
            received_at: Instant::now(),
        }
    }
}

impl AgentChatState {
    #[cfg(test)]
    pub(crate) fn fixture_change(
        &mut self,
        id: Uuid,
        status: AgentChatStatus,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        self.backend_generations.entry(id).or_insert(1);
        let session = self.ensure_session(id, "Fixture", cx);
        session.status = status;
        session.last_activity_at += 1;
        session.composer_text.push_str(text);
        self.publish_change(id, ChatChangeCategories::CONVERSATION, cx);
    }

    /// All session mutations publish here. Comparisons are bounded metadata;
    /// timeline length and response size do not affect navigation invalidation.
    pub(crate) fn publish_change(
        &mut self,
        id: Uuid,
        categories: ChatChangeCategories,
        cx: &mut Context<Self>,
    ) {
        let generation = self.backend_generation(id);
        let mut change =
            self.change_tracker
                .record(id, generation, self.sessions.get(&id), categories);
        if let Some(received_at) = self.current_event_received_at {
            change.received_at = received_at;
        }
        cx.emit(AgentChatEvent::SessionChanged(change));
        // All consumers subscribe to SessionChanged. A broad notify here would
        // schedule a window redraw merely because some rendered view read state.
    }

    pub(crate) fn has_authoritative_status(&self, id: Uuid) -> bool {
        self.backend_generation(id) > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decisions_completion_failure_and_restart_publish_navigation_without_debounce() {
        let id = Uuid::new_v4();
        let mut tracker = ChatChangeTracker::default();
        let mut session = empty_session_for_test(id);
        let statuses = [
            AgentChatStatus::Idle,
            AgentChatStatus::Running,
            AgentChatStatus::WaitingForUser,
            AgentChatStatus::Running,
            AgentChatStatus::PlanReady,
            AgentChatStatus::Running,
            AgentChatStatus::Cancelling,
            AgentChatStatus::Idle,
            AgentChatStatus::Running,
            AgentChatStatus::Failed,
        ];
        for (index, status) in statuses.into_iter().enumerate() {
            session.status = status;
            let event = tracker.record(id, 1, Some(&session), ChatChangeCategories::CONTENT);
            assert!(event.categories.navigation, "{status:?}");
            assert_eq!(event.sequence, (index * 2 + 1) as u64);
            let repeated = tracker.record(id, 1, Some(&session), ChatChangeCategories::CONTENT);
            assert!(
                !repeated.categories.navigation,
                "identical status is presentation-inert"
            );
        }
        session.chat_session_id = Some("resumed-session".into());
        let identity = tracker.record(id, 2, Some(&session), ChatChangeCategories::CONTENT);
        assert_eq!(identity.generation, 2);
        assert!(identity.categories.identity);
        session.status = AgentChatStatus::Idle;
        tracker.record(id, 2, Some(&session), ChatChangeCategories::CONTENT);
        session.last_activity_at += 1;
        assert!(
            tracker
                .record(id, 2, Some(&session), ChatChangeCategories::CONVERSATION)
                .categories
                .navigation,
            "late text after completion must still update unread attention"
        );
    }

    #[test]
    fn approval_and_question_presence_publish_even_without_a_status_transition() {
        let id = Uuid::new_v4();
        let mut tracker = ChatChangeTracker::default();
        let mut session = empty_session_for_test(id);
        session.status = AgentChatStatus::WaitingForUser;
        tracker.record(id, 1, Some(&session), ChatChangeCategories::CONTENT);
        session.pending_approval = Some(PendingApproval::new(
            "approval",
            PendingApprovalKind::Command,
            "Allow?",
            None,
        ));
        assert!(
            tracker
                .record(id, 1, Some(&session), ChatChangeCategories::CONTROLS)
                .categories
                .navigation
        );
        assert!(
            !tracker
                .record(id, 1, Some(&session), ChatChangeCategories::CONTROLS)
                .categories
                .navigation
        );
        session.pending_approval = None;
        session.pending_user_input = Some(PendingUserInput::new("question", vec![]));
        assert!(
            tracker
                .record(id, 1, Some(&session), ChatChangeCategories::CONTENT)
                .categories
                .navigation
        );
        session.pending_user_input = None;
        assert!(
            tracker
                .record(id, 1, Some(&session), ChatChangeCategories::CONTENT)
                .categories
                .navigation
        );
    }

    #[test]
    fn streaming_is_conversation_only_after_the_initial_snapshot() {
        let id = Uuid::new_v4();
        let mut tracker = ChatChangeTracker::default();
        let mut session = empty_session_for_test(id);
        session.status = AgentChatStatus::Running;
        let first = tracker.record(id, 1, Some(&session), ChatChangeCategories::CONTENT);
        assert!(first.categories.navigation);
        for _ in 0..1_000 {
            session.last_activity_at += 1;
            session.composer_text.push('a');
            let change = tracker.record(id, 1, Some(&session), ChatChangeCategories::CONVERSATION);
            assert!(!change.categories.navigation);
            assert!(!change.categories.identity);
        }
        session.status = AgentChatStatus::Idle;
        let done = tracker.record(id, 1, Some(&session), ChatChangeCategories::CONTENT);
        assert!(done.categories.navigation);
        assert_eq!(done.sequence, 1_002);
        assert!(
            tracker
                .record(id, 2, None, ChatChangeCategories::CONTENT)
                .categories
                .navigation
        );
    }

    fn empty_session_for_test(agent_id: Uuid) -> AgentChatSession {
        AgentChatSession {
            agent_id,
            title: "Agent".into(),
            chat_session_id: None,
            cli_session_id: None,
            hidden_from_notifications: false,
            status: AgentChatStatus::Idle,
            is_compacting: false,
            interaction_mode: AgentInteractionMode::Default,
            composer_text: String::new(),
            messages: vec![],
            timeline: vec![],
            queued_turns: vec![],
            work_log: vec![],
            pending_user_input: None,
            pending_approval: None,
            proposed_plan: None,
            latest_plan: None,
            changed_files: ChangedFilesSummary::default(),
            usage: None,
            started_running_at: None,
            last_activity_at: 0,
        }
    }
}
