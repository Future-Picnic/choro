#![allow(dead_code, reason = "retained agent-chat hydration controls")]

use super::*;

const AGENT_CHAT_HISTORY_PAGE_SIZE: usize = 200;

fn shift_agent_chat_indices(indices: &mut HashSet<(Uuid, usize)>, agent_id: Uuid, delta: usize) {
    if delta == 0 {
        return;
    }
    let shifted = indices
        .iter()
        .filter_map(|(id, index)| (*id == agent_id).then_some((*id, index.saturating_add(delta))))
        .collect::<Vec<_>>();
    indices.retain(|(id, _)| *id != agent_id);
    indices.extend(shifted);
}

impl CenterArea {
    /// Starts or resumes an app-owned agent terminal. Fresh starts send the
    /// agent doc as the first prompt; resumes only attach to the saved CLI
    /// session id.
    pub fn start_agent(&mut self, agent_id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.start_agent_in_mode(agent_id, CenterMode::Agents, window, cx);
    }

    pub(super) fn start_agent_in_mode(
        &mut self,
        agent_id: Uuid,
        target_mode: CenterMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((project, _)) = self.active_project(cx) else {
            return false;
        };
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return false;
        };
        if agent.project_id != project {
            return false;
        }
        // A Solo whose lane was torn down (e.g. after Ship PR) recreates it
        // first; the start continues from the lane-setup callback.
        if agent.is_active_solo()
            && (agent.lane_path.is_none()
                || !ide_core::lanes::lane_path_for(agent.project_id, agent.id)
                    .join(".git")
                    .exists())
        {
            return self.ensure_solo_lane_then_start(agent_id, target_mode, cx);
        }
        if agent.runtime == AgentRuntimeKind::Chat {
            return self.start_chat_agent_in_mode(agent, target_mode, cx);
        }

        let existing = {
            let manager = self.terminals.read(cx);
            manager
                .agent_record_terminal(project, agent_id)
                .or_else(|| {
                    agent
                        .cli_session_id
                        .as_deref()
                        .and_then(|session_id| manager.agent_session_terminal(project, session_id))
                })
        };
        if let Some(id) = existing {
            self.agent_detail_tabs
                .insert(agent_id, AgentDetailTab::Terminal);
            self.focus_agent_terminal_in_mode(project, id, target_mode, window, cx);
            return true;
        }

        // The agent's own working directory — a Solo's lane, not the project
        // root.
        let runtime_cwd = agent.runtime_path().to_path_buf();

        let resume_command = agent.resume_command();
        let connected_context = self.agent_connected_context_extras(&agent, cx);
        let command = resume_command
            .clone()
            .unwrap_or_else(|| agent.start_command_with_connected_context(&connected_context));
        let cli_session_id = if resume_command.is_some() {
            agent.cli_session_id.clone()
        } else {
            None
        };
        let spawned = self.terminals.update(cx, |manager, cx| {
            manager.spawn_agent_record(
                project,
                runtime_cwd,
                agent.id,
                agent.provider,
                agent.title.clone(),
                command,
                cli_session_id.clone(),
                cx,
            )
        });
        if spawned.is_ok() {
            self.agent_start_errors.remove(&agent_id);
            self.agents.update(cx, |agents, cx| {
                agents.mark_started(agent_id, cli_session_id, cx)
            });
            self.agent_detail_tabs
                .insert(agent_id, AgentDetailTab::Terminal);
            self.set_view_mode(target_mode, cx);
        }
        match spawned {
            Ok(id) => {
                self.focus_terminal_view(id, window, cx);
                true
            }
            Err(error) => {
                self.agent_start_errors
                    .insert(agent_id, format!("failed to spawn terminal: {error:#}"));
                eprintln!("failed to spawn terminal: {error:#}");
                cx.notify();
                false
            }
        }
    }

    pub(super) fn sync_chat_session_ids(&mut self, cx: &mut Context<Self>) {
        let (session_updates, changed_file_updates) = {
            let sessions = self.agent_chats.read(cx);
            let session_updates = sessions
                .sessions
                .iter()
                .filter_map(|(agent_id, session)| {
                    if session.chat_session_id.is_none() && session.cli_session_id.is_none() {
                        None
                    } else {
                        Some((
                            *agent_id,
                            session.chat_session_id.clone(),
                            session.cli_session_id.clone(),
                        ))
                    }
                })
                .collect::<Vec<_>>();
            let changed_file_updates = sessions
                .sessions
                .iter()
                .map(|(agent_id, session)| {
                    let mut files = session.changed_files.files.clone();
                    files.extend(session.changed_files.observed_files.clone());
                    (*agent_id, files)
                })
                .collect::<Vec<_>>();
            (session_updates, changed_file_updates)
        };

        if session_updates.is_empty() && changed_file_updates.is_empty() {
            return;
        }
        let mut refresh_projects: Vec<ProjectId> = Vec::new();
        self.agents.update(cx, |agents, cx| {
            for (agent_id, chat_session_id, cli_session_id) in session_updates {
                if let Some(chat_session_id) = chat_session_id {
                    agents.set_chat_session_id(agent_id, chat_session_id, cx);
                }
                if let Some(cli_session_id) = cli_session_id {
                    agents.set_cli_session_id(agent_id, cli_session_id, cx);
                }
            }
            for (agent_id, files) in changed_file_updates {
                // Refresh Preview only when the chat ledger's net projection
                // actually changed. Replacement also removes reverted paths.
                let project = agents.agent(agent_id).map(|agent| agent.project_id);
                if agents.replace_changed_files(agent_id, &files, cx) {
                    if let Some(project) = project {
                        if !refresh_projects.contains(&project) {
                            refresh_projects.push(project);
                        }
                    }
                }
            }
        });
        for project in refresh_projects {
            *self.project_preview_refresh.entry(project).or_insert(0) += 1;
        }
    }

    pub(super) fn load_chat_session_hydration(agent: &AgentRecord) -> Option<AgentChatHydration> {
        if let Ok(store) = ide_core::local_store::LocalStore::open_default() {
            if let Ok(page) =
                store.load_timeline_events_page(agent.id, None, AGENT_CHAT_HISTORY_PAGE_SIZE)
            {
                let mut timeline = page
                    .events
                    .iter()
                    .filter_map(timeline_item_from_store_event)
                    .collect::<Vec<_>>();
                if !timeline.is_empty() {
                    let session = Self::transient_hydration_session(agent);
                    let normalized = Self::normalize_code_review_blocks(&mut timeline);
                    let normalized =
                        Self::normalize_verification_blocks(&mut timeline) || normalized;
                    // Repairing from the provider transcript parses the entire
                    // raw conversation. Keep that legacy repair for small,
                    // fully loaded histories only; it would defeat paging for
                    // long resumed chats.
                    let repaired = !page.has_more
                        && Self::repair_timeline_messages_from_transcript(
                            &mut timeline,
                            &session,
                            agent,
                        );
                    if repaired || normalized {
                        if let Err(error) = persist_timeline_snapshot(agent.id, &timeline) {
                            eprintln!("failed to persist repaired chat timeline: {error:#}");
                        }
                    }
                    return Some(AgentChatHydration {
                        timeline,
                        proposed_plan: None,
                        oldest_sequence: page.oldest_sequence,
                        has_more: page.has_more,
                    });
                }
            }
            if let Ok(messages) = store.load_chat_messages(agent.id) {
                if !messages.is_empty() {
                    let mut timeline = Vec::new();
                    for message in messages {
                        let item = match message.role.as_str() {
                            "user" => AgentChatMessage::User {
                                text: message.text,
                                display_text: None,
                                tags: Vec::new(),
                                created_at: message.created_at,
                            },
                            "thought" => AgentChatMessage::Thought {
                                message_id: message.backend_message_id,
                                text: message.text,
                                created_at: message.created_at,
                            },
                            _ => AgentChatMessage::Assistant {
                                message_id: message.backend_message_id,
                                text: message.text,
                                created_at: message.created_at,
                            },
                        };
                        timeline.push(AgentChatTimelineItem::Message(item));
                    }
                    Self::normalize_code_review_blocks(&mut timeline);
                    Self::normalize_verification_blocks(&mut timeline);
                    if let Err(error) = persist_timeline_snapshot(agent.id, &timeline) {
                        eprintln!("failed to backfill chat timeline from messages: {error:#}");
                    }
                    return Some(AgentChatHydration {
                        timeline,
                        proposed_plan: None,
                        oldest_sequence: None,
                        has_more: false,
                    });
                }
            }
        }
        let session = Self::transient_hydration_session(agent);
        let messages = Self::agent_transcript_messages(&session, agent);
        if messages.is_empty() {
            return None;
        }
        let created_at = unix_now_secs();
        let mut timeline: Vec<AgentChatTimelineItem> = Vec::new();
        let mut last_plan: Option<crate::state::agent_chat::ProposedPlan> = None;
        for (index, message) in messages.into_iter().enumerate() {
            match message.role {
                DocAssistantRole::User => {
                    let msg = AgentChatMessage::User {
                        text: message.text,
                        display_text: None,
                        tags: Vec::new(),
                        created_at,
                    };
                    if timeline.last().is_some_and(|previous| {
                        matches!(previous, AgentChatTimelineItem::Message(previous) if previous == &msg)
                    }) {
                        continue;
                    }
                    timeline.push(AgentChatTimelineItem::Message(msg));
                }
                DocAssistantRole::Assistant => {
                    // Resumed transcripts carry the raw `<proposed_plan>` and
                    // `<code_review>` blocks as text; rebuild the cards and drop the
                    // tags from the prose.
                    let (cleaned, plan) =
                        crate::state::agent_chat::split_proposed_plan(&message.text);
                    let (cleaned, review) = crate::state::agent_chat::split_code_review(&cleaned);
                    let (cleaned, verification) =
                        crate::state::agent_chat::split_verification(&cleaned);
                    if !cleaned.trim().is_empty() {
                        let msg = AgentChatMessage::Assistant {
                            message_id: None,
                            text: cleaned,
                            created_at,
                        };
                        if !timeline.last().is_some_and(|previous| {
                            matches!(previous, AgentChatTimelineItem::Message(previous) if previous == &msg)
                        }) {
                            timeline.push(AgentChatTimelineItem::Message(msg));
                        }
                    }
                    if let Some(plan_markdown) = plan {
                        let mut plan = crate::state::agent_chat::ProposedPlan::new(
                            format!("resumed-plan-{index}"),
                            plan_markdown,
                        );
                        plan.mark_implemented();
                        last_plan = Some(plan.clone());
                        timeline.push(AgentChatTimelineItem::ProposedPlan(plan));
                    }
                    if let Some(review_markdown) = review {
                        timeline.push(AgentChatTimelineItem::CodeReview(
                            crate::state::agent_chat::CodeReview::new(
                                format!("resumed-review-{index}"),
                                review_markdown,
                            ),
                        ));
                    }
                    if let Some(verification_markdown) = verification {
                        timeline.push(AgentChatTimelineItem::Verification(
                            crate::state::agent_chat::Verification::new(
                                format!("resumed-verification-{index}"),
                                verification_markdown,
                            ),
                        ));
                    }
                }
            }
        }
        let resumed_changes: crate::state::agent_chat::ChangedFilesSummary =
            crate::state::agent_chat::ChangedFilesSummary {
                observed_files: agent
                    .changed_files
                    .iter()
                    .map(|file| {
                        crate::state::agent_chat::FileChangeStat::new(
                            file.path.clone(),
                            file.additions,
                            file.deletions,
                        )
                    })
                    .collect(),
                ..Default::default()
            };
        if !resumed_changes.is_empty() {
            timeline.push(AgentChatTimelineItem::ChangedFiles(resumed_changes));
        }
        if let Err(error) = persist_timeline_snapshot(agent.id, &timeline) {
            eprintln!("failed to persist hydrated chat timeline: {error:#}");
        }
        Some(AgentChatHydration {
            timeline,
            proposed_plan: last_plan,
            oldest_sequence: None,
            has_more: false,
        })
    }

    pub(super) fn transient_hydration_session(agent: &AgentRecord) -> AgentChatSession {
        AgentChatSession {
            agent_id: agent.id,
            title: agent.title.clone(),
            chat_session_id: agent.chat_session_id.clone(),
            cli_session_id: agent.cli_session_id.clone(),
            hidden_from_notifications: agent.hidden_doc_assistant,
            status: AgentChatStatus::Idle,
            interaction_mode: AgentInteractionMode::Default,
            composer_text: String::new(),
            messages: Vec::new(),
            timeline: Vec::new(),
            queued_turns: Vec::new(),
            work_log: Vec::new(),
            pending_user_input: None,
            pending_approval: None,
            proposed_plan: None,
            changed_files: Default::default(),
            usage: None,
            started_running_at: None,
            last_activity_at: unix_now_secs(),
        }
    }

    pub(super) fn agent_transcript_messages(
        session: &AgentChatSession,
        agent: &AgentRecord,
    ) -> Vec<DocAssistantTranscriptMessage> {
        let Some(session_id) = session
            .chat_session_id
            .as_deref()
            .or(agent.chat_session_id.as_deref())
            .or(session.cli_session_id.as_deref())
            .or(agent.cli_session_id.as_deref())
        else {
            return Vec::new();
        };
        let messages = doc_assistant::read_chat_transcript_messages(
            agent.provider,
            agent.runtime_path(),
            session_id,
        );
        Self::dedupe_transcript_messages(messages)
    }

    pub(super) fn dedupe_transcript_messages(
        messages: Vec<DocAssistantTranscriptMessage>,
    ) -> Vec<DocAssistantTranscriptMessage> {
        let mut deduped: Vec<DocAssistantTranscriptMessage> = Vec::new();
        for message in messages {
            if let Some(previous) = deduped.last_mut() {
                if previous.role == message.role && previous.text == message.text {
                    if previous.backend_message_id.is_none() {
                        previous.backend_message_id = message.backend_message_id;
                    }
                    continue;
                }
            }
            deduped.push(message);
        }
        deduped
    }

    pub(super) fn repair_timeline_messages_from_transcript(
        timeline: &mut [AgentChatTimelineItem],
        session: &AgentChatSession,
        agent: &AgentRecord,
    ) -> bool {
        let transcript = Self::agent_transcript_messages(session, agent);
        if transcript.is_empty() {
            return false;
        }

        let mut transcript_index = 0;
        let mut changed = false;
        for item in timeline {
            let AgentChatTimelineItem::Message(message) = item else {
                continue;
            };
            match message {
                AgentChatMessage::User { text, .. } => {
                    if let Some(index) =
                        Self::find_transcript_user(&transcript, transcript_index, text)
                    {
                        transcript_index = index + 1;
                    }
                }
                AgentChatMessage::Assistant {
                    message_id, text, ..
                } => {
                    if let Some((index, id_matched)) = Self::find_transcript_assistant(
                        &transcript,
                        transcript_index,
                        message_id.as_deref(),
                        text,
                    ) {
                        let full_text = transcript[index].text.trim().to_string();
                        if should_replace_saved_message_text(text, &full_text, id_matched) {
                            *text = full_text;
                            changed = true;
                        }
                        transcript_index = index + 1;
                    }
                }
                AgentChatMessage::Thought { .. } => {}
            }
        }
        changed
    }

    pub(super) fn normalize_code_review_blocks(timeline: &mut Vec<AgentChatTimelineItem>) -> bool {
        let existing_reviews = timeline
            .iter()
            .filter_map(|item| match item {
                AgentChatTimelineItem::CodeReview(review) => {
                    Some(review.markdown.trim().to_string())
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        let mut normalized = Vec::with_capacity(timeline.len());
        let mut changed = false;
        let mut generated_count = 0usize;

        for item in std::mem::take(timeline) {
            match item {
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id,
                    text,
                    created_at,
                }) => {
                    let (cleaned, review) = split_code_review(&text);
                    let Some(review_markdown) = review else {
                        normalized.push(AgentChatTimelineItem::Message(
                            AgentChatMessage::Assistant {
                                message_id,
                                text,
                                created_at,
                            },
                        ));
                        continue;
                    };

                    changed = true;
                    if !cleaned.trim().is_empty() {
                        normalized.push(AgentChatTimelineItem::Message(
                            AgentChatMessage::Assistant {
                                message_id: message_id.clone(),
                                text: cleaned,
                                created_at,
                            },
                        ));
                    }

                    if !existing_reviews.contains(review_markdown.trim()) {
                        generated_count += 1;
                        let id = message_id
                            .as_deref()
                            .map(|id| format!("stored-review-{id}"))
                            .unwrap_or_else(|| {
                                format!("stored-review-{created_at}-{generated_count}")
                            });
                        normalized.push(AgentChatTimelineItem::CodeReview(CodeReview::new(
                            id,
                            review_markdown,
                        )));
                    }
                }
                item => normalized.push(item),
            }
        }

        *timeline = normalized;
        changed
    }

    pub(super) fn normalize_verification_blocks(timeline: &mut Vec<AgentChatTimelineItem>) -> bool {
        let mut existing_verifications = timeline
            .iter()
            .filter_map(|item| match item {
                AgentChatTimelineItem::Verification(verification) => {
                    Some(verification.markdown.trim().to_string())
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        let mut normalized = Vec::with_capacity(timeline.len());
        let mut changed = false;
        let mut generated_count = 0usize;

        for item in std::mem::take(timeline) {
            match item {
                AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                    message_id,
                    text,
                    created_at,
                }) => {
                    let (cleaned, verification) = split_verification(&text);
                    let Some(verification_markdown) = verification else {
                        normalized.push(AgentChatTimelineItem::Message(
                            AgentChatMessage::Assistant {
                                message_id,
                                text,
                                created_at,
                            },
                        ));
                        continue;
                    };

                    changed = true;
                    if !cleaned.trim().is_empty() {
                        normalized.push(AgentChatTimelineItem::Message(
                            AgentChatMessage::Assistant {
                                message_id: message_id.clone(),
                                text: cleaned,
                                created_at,
                            },
                        ));
                    }

                    if !existing_verifications.contains(verification_markdown.trim()) {
                        existing_verifications.insert(verification_markdown.trim().to_string());
                        generated_count += 1;
                        let id = message_id
                            .as_deref()
                            .map(|id| format!("stored-verification-{id}"))
                            .unwrap_or_else(|| {
                                format!("stored-verification-{created_at}-{generated_count}")
                            });
                        normalized.push(AgentChatTimelineItem::Verification(Verification::new(
                            id,
                            verification_markdown,
                        )));
                    }
                }
                item => normalized.push(item),
            }
        }

        // Repair duplicates produced by older paginated hydration: those
        // synthesized cards use the stored-verification-* namespace, while the
        // provider-emitted card has its original id. Never collapse two real
        // lifecycle cards merely because their wording happens to match.
        let canonical_markdown = normalized
            .iter()
            .filter_map(|item| match item {
                AgentChatTimelineItem::Verification(verification)
                    if !verification.id.starts_with("stored-verification-") =>
                {
                    Some(verification.markdown.trim().to_string())
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        let before_dedup = normalized.len();
        normalized.retain(|item| match item {
            AgentChatTimelineItem::Verification(verification)
                if verification.id.starts_with("stored-verification-") =>
            {
                !canonical_markdown.contains(verification.markdown.trim())
            }
            _ => true,
        });
        changed = changed || normalized.len() != before_dedup;

        *timeline = normalized;
        changed
    }

    pub(super) fn find_transcript_user(
        transcript: &[DocAssistantTranscriptMessage],
        start: usize,
        text: &str,
    ) -> Option<usize> {
        let text = text.trim();
        transcript
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, message)| {
                message.role == DocAssistantRole::User && message.text.trim() == text
            })
            .map(|(index, _)| index)
    }

    pub(super) fn find_transcript_assistant(
        transcript: &[DocAssistantTranscriptMessage],
        start: usize,
        message_id: Option<&str>,
        text: &str,
    ) -> Option<(usize, bool)> {
        if let Some(message_id) = message_id {
            if let Some((index, _)) =
                transcript
                    .iter()
                    .enumerate()
                    .skip(start)
                    .find(|(_, message)| {
                        message.role == DocAssistantRole::Assistant
                            && message.backend_message_id.as_deref() == Some(message_id)
                    })
            {
                return Some((index, true));
            }
        }

        transcript
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, message)| {
                message.role == DocAssistantRole::Assistant
                    && (message.text.trim() == text.trim()
                        || should_replace_saved_message_text(text, &message.text, false))
            })
            .map(|(index, _)| (index, false))
    }

    pub(super) fn hydrate_chat_session_from_timeline(
        session: &mut AgentChatSession,
        _agent: &AgentRecord,
        timeline: Vec<AgentChatTimelineItem>,
    ) {
        let mut messages = Vec::new();
        let mut work_log = Vec::new();
        let mut pending_user_input = None;
        // The chat ledger is rebuilt exclusively from persisted per-turn
        // receipts. `agent.changed_files` is a project/worktree cache and can
        // contain paths produced by another concurrent chat.
        let mut changed_files = crate::state::agent_chat::ChangedFilesSummary::default();
        for item in &timeline {
            match item {
                AgentChatTimelineItem::Message(message) => messages.push(message.clone()),
                AgentChatTimelineItem::WorkLog(entry) => work_log.push(entry.clone()),
                AgentChatTimelineItem::FileChangeActivity(_) => {}
                AgentChatTimelineItem::PendingUserInput(pending) => {
                    pending_user_input = Some(pending.clone());
                }
                AgentChatTimelineItem::ProposedPlan(_) => {}
                AgentChatTimelineItem::ChangedFiles(summary) => changed_files.merge_turn(summary),
                AgentChatTimelineItem::CodeReview(_) => {}
                AgentChatTimelineItem::Verification(_) => {}
                AgentChatTimelineItem::ShipResult(_) => {}
                AgentChatTimelineItem::Rejoined(_) => {}
                AgentChatTimelineItem::RejoinConflict(_) => {}
                AgentChatTimelineItem::Memorized(_) => {}
                AgentChatTimelineItem::MemoryProposal(_) => {}
                AgentChatTimelineItem::AgentSummary(_) => {}
                AgentChatTimelineItem::AgentMessage(_) => {}
            }
        }
        changed_files = prefer_newest_hydrated_file_ledger(
            changed_files,
            crate::state::agent_chat::load_persisted_file_ledger(session.agent_id),
        );
        session.messages = messages;
        session.work_log = work_log;
        session.pending_user_input = pending_user_input;
        session.proposed_plan = None;
        session.changed_files = changed_files;
        session.timeline = timeline;
    }

    pub(super) fn start_chat_agent_in_mode(
        &mut self,
        mut agent: AgentRecord,
        target_mode: CenterMode,
        cx: &mut Context<Self>,
    ) -> bool {
        let agent_id = agent.id;
        let needs_hydration = self.agent_chats.update(cx, |chats, cx| {
            let session = chats.ensure_session(agent_id, agent.title.clone(), cx);
            if session.chat_session_id.is_none() {
                session.chat_session_id = agent.chat_session_id.clone();
            }
            if session.cli_session_id.is_none() {
                session.cli_session_id = agent.cli_session_id.clone();
            }
            let has_saved_backend_session = session.chat_session_id.is_some()
                || session.cli_session_id.is_some()
                || agent.chat_session_id.is_some()
                || agent.cli_session_id.is_some();
            let needs_hydration = has_saved_backend_session && session.messages.is_empty();
            if !has_saved_backend_session
                && session.messages.is_empty()
                && !agent.doc.trim().is_empty()
            {
                session.messages.push(AgentChatMessage::User {
                    text: agent.doc.clone(),
                    display_text: None,
                    tags: Vec::new(),
                    created_at: unix_now_secs(),
                });
            }
            session.status = AgentChatStatus::Idle;
            needs_hydration
        });
        self.agent_chat_selected_commands.remove(&agent_id);
        self.apply_live_chat_session_ids(&mut agent, cx);
        if agent.started_at.is_some()
            && !agent_has_backend_resume_id(&agent)
            && self.agent_chat_has_persisted_history(agent_id, cx)
        {
            self.agent_start_errors.insert(
                agent_id,
                "Cannot resume this chat safely because no Claude/Codex resume id was captured. Reset only if you intentionally want a new backend session."
                    .to_string(),
            );
            cx.notify();
            return false;
        }
        let initial_mode = self
            .agent_chats
            .read(cx)
            .session(agent_id)
            .map(|session| session.interaction_mode)
            .unwrap_or(AgentInteractionMode::Default);
        let connected_context = self.agent_connected_context_extras(&agent, cx);
        agent.doc = ide_core::prompt_with_connected_context(&agent.doc, &agent, &connected_context);
        if let Err(error) = self.agent_chats.update(cx, |chats, cx| {
            chats.start_backend(agent.clone(), initial_mode, cx)
        }) {
            self.agent_start_errors
                .insert(agent_id, format!("failed to start chat: {error:#}"));
            cx.notify();
            return false;
        }
        self.agent_start_errors.remove(&agent_id);
        self.agents
            .update(cx, |agents, cx| agents.mark_started(agent_id, None, cx));
        self.agent_detail_tabs
            .insert(agent_id, AgentDetailTab::Terminal);
        self.set_view_mode(target_mode, cx);
        if needs_hydration {
            self.schedule_agent_chat_hydration(agent, cx);
        }
        cx.notify();
        true
    }

    pub(super) fn schedule_agent_chat_hydration(
        &mut self,
        agent: AgentRecord,
        cx: &mut Context<Self>,
    ) {
        let agent_id = agent.id;
        let generation = self
            .agent_chat_hydration_generations
            .entry(agent_id)
            .and_modify(|generation| *generation = generation.wrapping_add(1))
            .or_insert(1);
        let generation = *generation;
        self.agent_chat_hydrating.insert(agent_id);
        self.agent_chat_history.remove(&agent_id);
        self.agent_chat_prepended_rows.remove(&agent_id);
        self.agent_chat_scrolled_up.insert(agent_id, false);
        self.agent_start_errors.remove(&agent_id);
        self.agent_detail_tabs
            .insert(agent_id, AgentDetailTab::Terminal);
        cx.notify();

        cx.spawn({
            let agent = agent.clone();
            async move |this, cx| {
                let hydration_agent = agent.clone();
                let hydration = cx
                    .background_executor()
                    .spawn(async move { Self::load_chat_session_hydration(&hydration_agent) })
                    .await;

                let _ = this.update(cx, |this, cx| {
                    if this
                        .agent_chat_hydration_generations
                        .get(&agent_id)
                        .copied()
                        != Some(generation)
                    {
                        return;
                    }
                    this.agent_chat_hydrating.remove(&agent_id);
                    this.agent_chat_scrolled_up.insert(agent_id, false);
                    let mut hydration = hydration;
                    let history_state = this.agent_chats.update(cx, |chats, cx| {
                        let session = chats.ensure_session(agent_id, agent.title.clone(), cx);
                        if session.chat_session_id.is_none() {
                            session.chat_session_id = agent.chat_session_id.clone();
                        }
                        if session.cli_session_id.is_none() {
                            session.cli_session_id = agent.cli_session_id.clone();
                        }
                        if session.messages.is_empty() {
                            if let Some(hydration) = hydration.take() {
                                let history_state = AgentChatHistoryState {
                                    oldest_sequence: hydration.oldest_sequence,
                                    has_more: hydration.has_more,
                                    loading: false,
                                    failed: false,
                                };
                                Self::hydrate_chat_session_from_timeline(
                                    session,
                                    &agent,
                                    hydration.timeline,
                                );
                                session.proposed_plan = hydration.proposed_plan;
                                return Some(history_state);
                            }
                        }
                        None
                    });
                    if let Some(history_state) = history_state {
                        this.agent_chat_history.insert(agent_id, history_state);
                    }
                    if let Some(state) = this.agent_chat_list_states.get(&agent_id) {
                        let top_down = this.workspace.read(cx).conversation_layout
                            == ide_core::config::ConversationLayout::TopDown;
                        Self::scroll_agent_chat_list_to_latest(state, state.item_count(), top_down);
                    }
                    let pending_submissions = this
                        .agent_chat_post_hydration_submissions
                        .remove(&agent_id)
                        .unwrap_or_default();
                    for submission in pending_submissions {
                        this.dispatch_agent_chat_submission_with_agent(
                            &agent,
                            submission.text,
                            submission.display_text,
                            submission.tags,
                            submission.mode,
                            cx,
                        );
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub(super) fn load_older_agent_chat_history(&mut self, agent_id: Uuid, cx: &mut Context<Self>) {
        let Some(agent) = self.agents.read(cx).agent(agent_id).cloned() else {
            return;
        };
        let Some(history) = self.agent_chat_history.get_mut(&agent_id) else {
            return;
        };
        if !history.has_more || history.loading || history.failed {
            return;
        }
        let Some(before_sequence) = history.oldest_sequence else {
            history.has_more = false;
            return;
        };

        history.loading = true;
        let generation = self
            .agent_chat_hydration_generations
            .get(&agent_id)
            .copied()
            .unwrap_or_default();
        cx.notify();

        cx.spawn(async move |this, cx| {
            let page = cx
                .background_executor()
                .spawn(async move {
                    let store = ide_core::local_store::LocalStore::open_default()?;
                    store.load_timeline_events_page(
                        agent_id,
                        Some(before_sequence),
                        AGENT_CHAT_HISTORY_PAGE_SIZE,
                    )
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if this
                    .agent_chat_hydration_generations
                    .get(&agent_id)
                    .copied()
                    .unwrap_or_default()
                    != generation
                {
                    return;
                }

                let page = match page {
                    Ok(page) => page,
                    Err(error) => {
                        if let Some(history) = this.agent_chat_history.get_mut(&agent_id) {
                            history.loading = false;
                            history.failed = true;
                        }
                        eprintln!("failed to load older chat history: {error:#}");
                        cx.notify();
                        return;
                    }
                };
                let mut older_timeline = page
                    .events
                    .iter()
                    .filter_map(timeline_item_from_store_event)
                    .collect::<Vec<_>>();
                Self::normalize_code_review_blocks(&mut older_timeline);
                // Verification cards are persisted after their raw tagged
                // assistant message. A history-page boundary can split the two;
                // defer verification normalization until the older and current
                // pages are combined so the raw block cannot synthesize a
                // duplicate card.
                let timeline_index_delta = older_timeline.len();
                let message_index_delta = older_timeline
                    .iter()
                    .filter(|item| matches!(item, AgentChatTimelineItem::Message(_)))
                    .count();

                let artifact_filter =
                    VisualizationArtifactFilter::new(agent_id, agent.runtime_path());
                let row_delta = this.agent_chats.update(cx, |chats, cx| {
                    let Some(session) = chats.session_mut(agent_id) else {
                        return None;
                    };
                    let include_activity = matches!(
                        session.status,
                        AgentChatStatus::Running | AgentChatStatus::Cancelling
                    );
                    let include_saved_resume = session.chat_session_id.is_some()
                        || session.cli_session_id.is_some()
                        || agent.chat_session_id.is_some()
                        || agent.cli_session_id.is_some();
                    let old_row_count = agent_chat_rows(
                        session,
                        include_activity,
                        include_saved_resume,
                        &artifact_filter,
                    )
                    .len();
                    let proposed_plan = session.proposed_plan.clone();
                    older_timeline.append(&mut session.timeline);
                    Self::normalize_code_review_blocks(&mut older_timeline);
                    Self::normalize_verification_blocks(&mut older_timeline);
                    Self::hydrate_chat_session_from_timeline(session, &agent, older_timeline);
                    session.proposed_plan = proposed_plan;
                    let new_row_count = agent_chat_rows(
                        session,
                        include_activity,
                        include_saved_resume,
                        &artifact_filter,
                    )
                    .len();
                    cx.notify();
                    Some(new_row_count.saturating_sub(old_row_count))
                });
                let Some(row_delta) = row_delta else {
                    this.agent_chat_history.remove(&agent_id);
                    return;
                };

                if row_delta > 0 {
                    *this.agent_chat_prepended_rows.entry(agent_id).or_default() += row_delta;
                }
                shift_agent_chat_indices(
                    &mut this.agent_chat_expanded_work_log_groups,
                    agent_id,
                    timeline_index_delta,
                );
                shift_agent_chat_indices(
                    &mut this.agent_chat_expanded_work_log_entries,
                    agent_id,
                    timeline_index_delta,
                );
                shift_agent_chat_indices(
                    &mut this.agent_chat_expanded_thoughts,
                    agent_id,
                    message_index_delta,
                );
                shift_agent_chat_indices(
                    &mut this.agent_chat_expanded_user_messages,
                    agent_id,
                    message_index_delta,
                );
                if let Some((hovered_agent_id, index)) = this.agent_chat_hovered_message.as_mut() {
                    if *hovered_agent_id == agent_id {
                        *index = index.saturating_add(message_index_delta);
                    }
                }
                this.agent_chat_history.insert(
                    agent_id,
                    AgentChatHistoryState {
                        oldest_sequence: page.oldest_sequence,
                        has_more: page.has_more,
                        loading: false,
                        failed: false,
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cancel_agent_chat_hydration(&mut self, agent_id: Uuid) {
        self.agent_chat_hydrating.remove(&agent_id);
        self.agent_chat_post_hydration_submissions.remove(&agent_id);
        self.agent_chat_history.remove(&agent_id);
        self.agent_chat_prepended_rows.remove(&agent_id);
        self.agent_chat_hydration_generations
            .entry(agent_id)
            .and_modify(|generation| *generation = generation.wrapping_add(1))
            .or_insert(1);
    }

    /// Selects and focuses an open terminal-backed agent.
    pub fn focus_agent_terminal(
        &mut self,
        project: ProjectId,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_agent_terminal_in_mode(project, id, CenterMode::Agents, window, cx);
    }

    pub(super) fn focus_agent_terminal_in_mode(
        &mut self,
        project: ProjectId,
        id: SessionId,
        mode: CenterMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminals
            .update(cx, |manager, cx| manager.set_active(project, id, cx));
        self.set_view_mode(mode, cx);
        self.focus_terminal_view(id, window, cx);
    }

    pub(super) fn focus_terminal_view(
        &self,
        id: SessionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let manager = self.terminals.read(cx);
        if let Some(session) = manager.sessions.iter().find(|s| s.id == id) {
            session.view.read(cx).focus_handle().clone().focus(window);
        }
        cx.notify();
    }
}

fn prefer_newest_hydrated_file_ledger(
    rebuilt: crate::state::agent_chat::ChangedFilesSummary,
    persisted: Option<crate::state::agent_chat::ChangedFilesSummary>,
) -> crate::state::agent_chat::ChangedFilesSummary {
    persisted
        .filter(|persisted| persisted.ledger_revision >= rebuilt.ledger_revision)
        .unwrap_or(rebuilt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepending_history_shifts_only_the_target_agents_indices() {
        let target = Uuid::new_v4();
        let other = Uuid::new_v4();
        let mut indices = HashSet::from([(target, 0), (target, 7), (other, 3)]);

        shift_agent_chat_indices(&mut indices, target, 4);

        assert_eq!(
            indices,
            HashSet::from([(target, 4), (target, 11), (other, 3)])
        );
    }

    #[test]
    fn prepending_no_history_keeps_indices_stable() {
        let agent = Uuid::new_v4();
        let mut indices = HashSet::from([(agent, 2)]);

        shift_agent_chat_indices(&mut indices, agent, 0);

        assert_eq!(indices, HashSet::from([(agent, 2)]));
    }

    #[test]
    fn stale_persisted_ledger_does_not_override_newer_turn_receipts() {
        let rebuilt = crate::state::agent_chat::ChangedFilesSummary {
            files: vec![crate::state::agent_chat::FileChangeStat::new(
                "src/newest.rs",
                1,
                0,
            )],
            ledger_revision: 2,
            ..Default::default()
        };
        let persisted = crate::state::agent_chat::ChangedFilesSummary {
            files: vec![crate::state::agent_chat::FileChangeStat::new(
                "src/stale.rs",
                1,
                0,
            )],
            ledger_revision: 1,
            ..Default::default()
        };

        let selected = prefer_newest_hydrated_file_ledger(rebuilt, Some(persisted));

        assert_eq!(selected.files[0].path, PathBuf::from("src/newest.rs"));
        assert_eq!(selected.ledger_revision, 2);
    }

    #[test]
    fn verification_normalization_does_not_duplicate_a_persisted_card() {
        let markdown = "## Met\n- **requirement** — implemented";
        let mut timeline = vec![
            AgentChatTimelineItem::Message(AgentChatMessage::Assistant {
                message_id: Some("assistant-1".to_string()),
                text: format!("<verification>\n{markdown}\n</verification>"),
                created_at: 1,
            }),
            AgentChatTimelineItem::Verification(Verification::new("provider-1", markdown)),
        ];

        assert!(CenterArea::normalize_verification_blocks(&mut timeline));
        assert_eq!(
            timeline
                .iter()
                .filter(|item| matches!(item, AgentChatTimelineItem::Verification(_)))
                .count(),
            1
        );
    }

    #[test]
    fn verification_normalization_repairs_old_synthetic_duplicate() {
        let markdown = "## Met\n- **requirement** — implemented";
        let mut timeline = vec![
            AgentChatTimelineItem::Verification(Verification::new(
                "stored-verification-assistant-1",
                markdown,
            )),
            AgentChatTimelineItem::Verification(Verification::new("provider-1", markdown)),
        ];

        assert!(CenterArea::normalize_verification_blocks(&mut timeline));
        assert_eq!(timeline.len(), 1);
        assert!(matches!(
            &timeline[0],
            AgentChatTimelineItem::Verification(verification) if verification.id == "provider-1"
        ));
    }

    #[test]
    fn verification_normalization_keeps_real_lifecycle_history() {
        let markdown = "## Met\n- **requirement** — implemented";
        let mut timeline = vec![
            AgentChatTimelineItem::Verification(Verification::new("provider-1", markdown)),
            AgentChatTimelineItem::Verification(Verification::new("provider-2", markdown)),
        ];

        assert!(!CenterArea::normalize_verification_blocks(&mut timeline));
        assert_eq!(timeline.len(), 2);
    }
}
